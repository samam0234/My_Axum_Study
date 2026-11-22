//! study6 — DB · API 연동 (CRUD). 포트 3006.
//!
//! 한 단계에서 배우는 것만 담았다: sqlx + SQLite, 마이그레이션, 3-타입(요청 DTO / DB 행 / 응답),
//! `INSERT … RETURNING`, `COALESCE` 부분 수정, `rows_affected`, `QueryBuilder` 검색,
//! 오류를 값으로(`AppError` + `IntoResponse`), 연결 정보를 환경변수로.
//! (인증·댓글·호출 제한이 붙은 실제 모양은 demo/)
//!
//! 실행:  cargo run --example s06_db
//!        DATABASE_URL="sqlite://s06.db?mode=rwc" cargo run --example s06_db   # 파일에 저장 (재시작해도 남는다)

use std::{str::FromStr, time::Duration};

use axum::{
    Json, Router,
    extract::{FromRequest, Path, Query, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{
    FromRow, QueryBuilder, Sqlite, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use validator::Validate;

// ── 연결 + 마이그레이션 ────────────────────────────────────────────────────
async fn init_db(url: &str) -> anyhow::Result<SqlitePool> {
    // 메모리 DB는 연결마다 별개의 DB다 → 연결을 1개로 고정하지 않으면 시험이 랜덤하게 `no such table`
    let in_memory = url.contains(":memory:") || url.contains("mode=memory");

    let mut options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true) // 기본값 false → 첫 실행부터 "unable to open database file"
        .busy_timeout(Duration::from_secs(5)); // 기본 0 → 동시 쓰기에서 "database is locked"
    if !in_memory {
        options = options.journal_mode(SqliteJournalMode::Wal); // 읽기·쓰기가 서로를 막지 않는다
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(if in_memory { 1 } else { 5 })
        .connect_with(options)
        .await?;

    // 컴파일 시점에 .sql 파일을 바이너리에 박아 넣는다 → 배포 이미지에 폴더가 없어도 된다
    sqlx::migrate!("./examples/s06_migrations").run(&pool).await?;
    Ok(pool)
}

// ── 3-타입: DB 행 / 응답 / 요청 ────────────────────────────────────────────
/// DB의 한 행. tags가 JSON 문자열이라 API 모양과 다르다.
#[derive(Debug, FromRow)]
struct PostRow {
    id: i64,
    title: String,
    body: String,
    tags: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// API 응답. 직렬화 경계에서만 camelCase로 바뀐다.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Post {
    id: i64,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PostRow> for Post {
    fn from(row: PostRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            body: row.body,
            tags: serde_json::from_str(&row.tags).unwrap_or_default(), // 깨진 JSON이어도 500 대신 빈 배열
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    title: String,
    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    body: String,
    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    tags: Vec<String>,
}

/// 모든 필드가 Option → 부분 수정(PATCH). None = "안 보냈다" = 기존 값 유지
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct UpdatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    title: Option<String>,
    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    body: Option<String>,
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Validate)]
struct ListQuery {
    #[serde(default = "one")]
    #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    page: i64,
    #[serde(default = "ten")]
    #[validate(range(min = 1, message = "limit는 1 이상이어야 합니다"))]
    limit: i64, // 상한(50)은 오류가 아니라 보정이다 — 아래 list()
    q: Option<String>,
}
fn one() -> i64 {
    1
}
fn ten() -> i64 {
    10
}

// ── 오류는 값이다 ──────────────────────────────────────────────────────────
#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("post {0} not found")]
    NotFound(i64),
    #[error("{0}")]
    BadRequest(String),
    #[error("validation failed")]
    Validation(validator::ValidationErrors),
    // #[from] 덕분에 sqlx 오류가 `?` 한 글자로 여기로 들어온다
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message, fields) = match &self {
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND", self.to_string(), None),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", m.clone(), None),
            Self::Validation(errors) => {
                let fields: serde_json::Map<_, _> = errors
                    .field_errors()
                    .into_iter()
                    .map(|(f, errs)| {
                        let msgs: Vec<_> = errs.iter().filter_map(|e| e.message.as_ref().map(|m| m.to_string())).collect();
                        (f.to_string(), json!(msgs))
                    })
                    .collect();
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "입력값을 확인하세요.".into(), Some(fields))
            }
            // 500은 내부 사정을 밖으로 흘리지 않는다. 원인은 서버 로그에만.
            Self::Database(e) => {
                eprintln!("database error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "서버 내부 오류가 발생했습니다.".into(), None)
            }
        };
        let mut error = json!({ "code": code, "message": message });
        if let Some(fields) = fields {
            error["fields"] = fields.into();
        }
        (status, Json(json!({ "error": error }))).into_response()
    }
}

type AppResult<T> = Result<T, AppError>;

/// Json<T> + validator를 한 번에. 본문 extractor라서 핸들러의 마지막 인자여야 한다.
struct ValidatedJson<T>(T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: serde::de::DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = AppError;
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e: JsonRejection| AppError::BadRequest(e.body_text()))?;
        value.validate().map_err(AppError::Validation)?;
        Ok(Self(value))
    }
}

// ── repo: SQL을 직접 쓴다 (sqlx는 ORM이 아니다) ─────────────────────────────
async fn find(db: &SqlitePool, id: i64) -> Result<Option<PostRow>, sqlx::Error> {
    // sqlx 0.9: query_as는 &'static str만 받는다. format!로 SQL을 조립하면 컴파일 에러(SqlSafeStr)
    sqlx::query_as::<_, PostRow>("SELECT id, title, body, tags, created_at, updated_at FROM posts WHERE id = $1")
        .bind(id) // 값은 SQL 문자열에 끼워 넣지 않고 바인딩한다 → SQL 인젝션 방어
        .fetch_optional(db) // 0건이면 Ok(None) → 핸들러가 404로
        .await
}

// ── 핸들러 ─────────────────────────────────────────────────────────────────
async fn list(State(db): State<SqlitePool>, Query(q): Query<ListQuery>) -> AppResult<Json<serde_json::Value>> {
    q.validate().map_err(AppError::Validation)?;
    let limit = q.limit.min(50); // 스펙: 50 초과는 오류가 아니라 50으로 보정
    let keyword = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());

    // 조건이 동적이면 QueryBuilder. SQL 조각과 바인딩 값을 따로 쌓아 값이 SQL에 섞이지 않는다.
    let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM posts WHERE 1 = 1");
    let mut rows = QueryBuilder::<Sqlite>::new("SELECT id, title, body, tags, created_at, updated_at FROM posts WHERE 1 = 1");
    for builder in [&mut count, &mut rows] {
        if let Some(k) = keyword {
            let pattern = format!("%{k}%");
            // LOWER(..) LIKE LOWER(..): SQLite는 LIKE가 대소문자를 무시하지만 PostgreSQL은 구분한다 → 명시적으로 맞춘다
            builder.push(" AND (LOWER(title) LIKE LOWER(").push_bind(pattern.clone());
            builder.push(") OR LOWER(body) LIKE LOWER(").push_bind(pattern).push("))");
        }
    }
    let total: i64 = count.build_query_scalar().fetch_one(&db).await?;
    rows.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    rows.push(" OFFSET ").push_bind((q.page - 1) * limit);
    let items: Vec<Post> = rows.build_query_as::<PostRow>().fetch_all(&db).await?.into_iter().map(Post::from).collect();

    Ok(Json(json!({
        "data": items,
        "meta": { "page": q.page, "limit": limit, "total": total, "totalPages": (total + limit - 1) / limit, "q": keyword },
    })))
}

async fn get_one(State(db): State<SqlitePool>, Path(id): Path<i64>) -> AppResult<Json<serde_json::Value>> {
    let row = find(&db, id).await?.ok_or(AppError::NotFound(id))?;
    Ok(Json(json!({ "data": Post::from(row) })))
}

async fn create(State(db): State<SqlitePool>, ValidatedJson(body): ValidatedJson<CreatePost>) -> AppResult<impl IntoResponse> {
    let now = Utc::now();
    let tags = serde_json::to_string(&body.tags).unwrap_or_else(|_| "[]".into());
    // RETURNING: INSERT와 조회를 한 번에 끝낸다
    let row: PostRow = sqlx::query_as(
        "INSERT INTO posts (title, body, tags, created_at, updated_at) VALUES ($1, $2, $3, $4, $4)
         RETURNING id, title, body, tags, created_at, updated_at",
    )
    .bind(&body.title)
    .bind(&body.body)
    .bind(tags)
    .bind(now)
    .fetch_one(&db)
    .await?;

    let location = format!("/api/posts/{}", row.id);
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(json!({ "data": Post::from(row) }))))
}

async fn update(State(db): State<SqlitePool>, Path(id): Path<i64>, ValidatedJson(body): ValidatedJson<UpdatePost>) -> AppResult<Json<serde_json::Value>> {
    let tags = body.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".into()));
    // COALESCE($1, title): 바인딩 값이 NULL이면 기존 값을 쓴다 → 필드 조합마다 SQL을 따로 만들지 않아도 PATCH가 된다
    let row: Option<PostRow> = sqlx::query_as(
        "UPDATE posts SET title = COALESCE($1, title), body = COALESCE($2, body), tags = COALESCE($3, tags), updated_at = $4
         WHERE id = $5 RETURNING id, title, body, tags, created_at, updated_at",
    )
    .bind(body.title)
    .bind(body.body)
    .bind(tags)
    .bind(Utc::now())
    .bind(id)
    .fetch_optional(&db)
    .await?;
    let row = row.ok_or(AppError::NotFound(id))?;
    Ok(Json(json!({ "data": Post::from(row) })))
}

async fn delete(State(db): State<SqlitePool>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    let result = sqlx::query("DELETE FROM posts WHERE id = $1").bind(id).execute(&db).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(id)); // 실제로 지워졌는지 확인 → 없는 id면 404
    }
    Ok(StatusCode::NO_CONTENT) // 삭제는 204 + 본문 없음
}

fn app(db: SqlitePool) -> Router {
    Router::new()
        .route("/api/posts", get(list).post(create))
        .route("/api/posts/{id}", get(get_one).patch(update).delete(delete))
        .with_state(db)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 연결 정보는 코드가 아니라 환경변수로. 없으면 개발용 기본값(메모리 DB)
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".into());
    let db = init_db(&url).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3006").await?;
    println!("s06_db  http://127.0.0.1:3006/api/posts   (DATABASE_URL={url})");
    axum::serve(listener, app(db)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn call(app: &Router, method: &str, uri: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    async fn test_app() -> Router {
        app(init_db("sqlite::memory:").await.unwrap())
    }

    #[tokio::test]
    async fn crud_흐름과_부분_수정() {
        let app = test_app().await;
        let (s, created) = call(&app, "POST", "/api/posts", Some(json!({"title":"첫 글","body":"본문","tags":["rust"]}))).await;
        assert_eq!(s, StatusCode::CREATED);
        let id = created["data"]["id"].as_i64().unwrap();
        assert_eq!(created["data"]["tags"], json!(["rust"]));

        // 제목만 보냈으니 본문·태그는 그대로 (COALESCE)
        let (s, updated) = call(&app, "PATCH", &format!("/api/posts/{id}"), Some(json!({"title":"수정"}))).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!((updated["data"]["title"].as_str(), updated["data"]["body"].as_str()), (Some("수정"), Some("본문")));
        assert_eq!(updated["data"]["tags"], json!(["rust"]));

        assert_eq!(call(&app, "DELETE", &format!("/api/posts/{id}"), None).await.0, StatusCode::NO_CONTENT);
        assert_eq!(call(&app, "GET", &format!("/api/posts/{id}"), None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(call(&app, "DELETE", &format!("/api/posts/{id}"), None).await.0, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn 검색은_대소문자를_무시하고_인젝션은_검색어일_뿐이다() {
        let app = test_app().await;
        for t in ["Axum 입문", "다른 글"] {
            call(&app, "POST", "/api/posts", Some(json!({"title":t,"body":"x"}))).await;
        }
        let (_, found) = call(&app, "GET", "/api/posts?q=AXUM", None).await;
        assert_eq!(found["meta"]["total"], 1);
        let (_, evil) = call(&app, "GET", "/api/posts?q=%27%20OR%201%3D1%20--", None).await;
        assert_eq!(evil["meta"]["total"], 0, "인젝션이 먹히면 전부 나온다");
    }

    #[tokio::test]
    async fn limit은_50으로_보정되고_검증_실패는_fields를_준다() {
        let app = test_app().await;
        let (s, body) = call(&app, "GET", "/api/posts?limit=1000", None).await;
        assert_eq!((s, &body["meta"]["limit"]), (StatusCode::OK, &json!(50)));

        let (s, body) = call(&app, "POST", "/api/posts", Some(json!({"title":"","body":""}))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
        assert!(body["error"]["fields"]["title"].is_array() && body["error"]["fields"]["body"].is_array());
    }
}
