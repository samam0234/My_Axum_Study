# study6 — DB · API 연동 (CRUD)와 공통 API 스펙 v1

> 🎯 **목표**: DB(sqlx + SQLite)를 연결해 게시글 **목록·상세·등록·수정·삭제**와 댓글 CRUD를 만들고, 경로·응답·오류 모양을 **공통 API 스펙 v1**에 맞추며, 연결 정보를 환경변수로 분리한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | 단계 예제 [lab/examples/s06_db.rs](lab/examples/s06_db.rs)(포트 3006), 실제 프로젝트: [demo/src/db.rs](demo/src/db.rs), [demo/src/posts/](demo/src/posts/), [demo/src/comments/](demo/src/comments/), [demo/src/response.rs](demo/src/response.rs), [demo/src/pagination.rs](demo/src/pagination.rs), [demo/src/error.rs](demo/src/error.rs), [demo/src/middleware.rs](demo/src/middleware.rs), [demo/src/ratelimit.rs](demo/src/ratelimit.rs), [demo/src/config.rs](demo/src/config.rs), [demo/migrations/](demo/migrations/) |
| 이 단계의 코드 변경 | `/api` 경로 · `{ data, meta }` 응답 · 스펙 v1 오류 본문 · camelCase · 이메일 로그인 · 댓글 · 정렬 · 호출 제한(429) · `CORS_ORIGIN` (단위 12 + 통합 31 테스트 통과) |
| 버전 | axum 0.8.9, **sqlx 0.9** (`query`/`query_as`는 리터럴 SQL만 받음) |

---

## ① 목표

1. DB 연결 설정(`create_if_missing`, WAL, `busy_timeout`)과 마이그레이션을 이해한다.
2. DB 행(`PostRow`)과 API 응답(`Post`)을 **별개 타입**으로 두고 경계에서 변환한다.
3. `INSERT … RETURNING`, `COALESCE` 부분 수정, `rows_affected` 삭제를 쓴다.
4. 오류를 `Result<T, AppError>`로 다루고, 응답 본문을 스펙 v1 모양으로 만든다.
5. **스펙 v1의 경로·응답 껍데기(`{ data, meta }`)·쿼리 규칙(`limit` 상한, `sort`)을 코드로 옮긴다.**
6. 연결 정보·비밀 값을 환경변수로 분리한다.

---

## ② 📖 이론

### sqlx는 ORM이 아니다

SQL을 **직접 쓰고**, 결과를 구조체로 받아 주는 얇은 도구다. 어떤 쿼리가 나가는지 100% 보이는 대신, 컬럼 매핑 실수는 **런타임**에 드러난다(`query!` 매크로는 컴파일 타임 검증이지만 DB 연결/`.sqlx` 캐시가 필요해 학습 단계에서는 런타임 함수 `query`/`query_as`를 쓴다).

### 3-타입 구조 — DB, 요청, 응답을 섞지 않는다

```
요청 JSON ──▶ CreatePost / UpdatePost   (검증 규칙이 붙은 입력 DTO)
                    │
              repo (SQL)  ◀──▶  PostRow   (DB 한 행 + JOIN·집계 결과: tags는 JSON 문자열)
                                   │ From
                                   ▼
                               Post  ──▶ Data<Post>  ──▶ 응답 JSON  { "data": { camelCase, tags는 배열 } }
```

비밀번호 해시 같은 필드는 **응답 타입에 아예 없게** 만들어 실수로 새어 나갈 일을 구조적으로 막는다(study7).

### 오류는 값이다

```rust
async fn get_one(...) -> AppResult<Json<Data<Post>>>        // Result<Json<Data<Post>>, AppError>
```

실패는 예외가 아니라 **반환 타입에 적힌 값**이다. `AppError`가 `IntoResponse`를 구현하므로 `?` 한 글자로 sqlx 오류가 500 JSON까지 흘러간다. 새 오류 갈래를 추가하면 응답 매핑 `match`가 **컴파일 에러로 누락을 알려 준다.**

### 공통 API 스펙 v1 — 프론트와의 계약

프론트가 백엔드를 바꿔 끼워도 코드를 고치지 않으려면 **경로·응답·오류 모양**이 같아야 한다. 스펙이 정한 것:

| 영역 | 스펙 v1의 약속 |
|---|---|
| 경로 | Base URL `/api` (헬스 체크 `/health`만 접두어 없이), 복수 명사, 동사 금지 |
| 성공 | 단건 `{ data }`, 목록 `{ data: [...], meta: { page, limit, total, totalPages, q } }` |
| 쿼리 | `page`(1부터), `limit`(기본 10, **최대 50 — 넘으면 50으로**), `q`, `sort`(`createdAt` · `-createdAt` · `-commentCount`) |
| 오류 | 모든 4xx·5xx가 `{ error: { code, message, fields, requestId } }` |
| 이름 | JSON 필드는 camelCase, 시각은 ISO 8601 UTC |

```json
{ "error": { "code": "VALIDATION_ERROR", "message": "입력값을 확인하세요.",
             "fields": { "title": ["제목은 1~100자여야 합니다"] },
             "requestId": "627d37a8-6fec-43e4-bff0-3cc52fab33cb" } }
```

---

## ③ 🛠️ 실기

### 3-0. 단계 예제 — [lab/examples/s06_db.rs](lab/examples/s06_db.rs) (포트 3006)

DB를 붙인 CRUD에서 **이 단계가 가르치는 것만** 한 파일에 담았다. 인증·댓글·호출 제한이 붙은 실제 모양은 아래 3-1부터의 `demo/`다. 이 예제는 `sqlx::migrate!`가 읽는 SQL 두 개([0001](lab/examples/s06_migrations/0001_create_posts.sql), [0002](lab/examples/s06_migrations/0002_add_tags.sql))와 함께 돈다.

```powershell
cd lab
cargo run --example s06_db                                              # 메모리 DB (끝나면 사라진다)
$env:DATABASE_URL = "sqlite://s06.db?mode=rwc"; cargo run --example s06_db   # 파일 DB (재시작해도 남는다)
cargo test --example s06_db                                             # 시험 3개
```

```rust
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
```

시험 코드는 같은 파일 아래쪽의 `#[cfg(test)] mod tests`에 있다: [s06_db.rs](lab/examples/s06_db.rs).

| 코드 | 설명 |
|---|---|
| `create_if_missing(true)` · `busy_timeout` · `journal_mode(Wal)` | 기본값이 false / 0 / 롤백 저널이라 첫 실행부터 `unable to open database file`, 동시 쓰기에서 `database is locked`. WAL은 읽기·쓰기가 서로 막지 않게 한다 |
| `in_memory`면 `max_connections(1)` | 메모리 DB는 **연결마다 별개의 DB**. 풀이 둘 이상이면 시험이 랜덤하게 `no such table` |
| `sqlx::migrate!("./examples/s06_migrations")` | **컴파일 시점에** `.sql`을 바이너리에 박는다. `0002`는 `0001`을 고치지 않고 **새 파일로 쌓은** 것이다 (적용된 파일을 고치면 `VersionMismatch`) |
| `PostRow` → `Post` → `CreatePost`/`UpdatePost` | DB 행(tags가 JSON 문자열) / 응답(camelCase, tags 배열) / 입력(검증 규칙)을 **섞지 않는다**. 경계에서만 `From`으로 변환 |
| `deny_unknown_fields` | 모르는 필드(예: 클라이언트가 보낸 `author`)는 거절 |
| `UpdatePost`의 모든 필드가 `Option` | `None` = "안 보냈다" = 기존 값 유지 (PATCH 의미) |
| `AppError` + `#[from] sqlx::Error` + `IntoResponse` | 실패는 반환 타입에 적힌 **값**. `?` 한 글자로 sqlx 오류가 500 JSON까지 흐르고, 응답에는 고정 문구만 (원인은 서버 로그에) |
| `ValidatedJson` | `Json` 파싱 + `validator`를 한 번에. **본문 extractor라서 핸들러의 마지막 인자** |
| `query_as(..).bind(id).fetch_optional` | `$1` 바인딩 → SQL 인젝션 방어. 0건이면 `Ok(None)` → 404. sqlx 0.9는 `&'static str`만 받는다(`format!` 금지) |
| `QueryBuilder` + `push_bind` | 조건이 동적일 때. SQL 조각과 값을 따로 쌓아 값이 SQL에 섞이지 않는다. 같은 조건을 건수 쿼리와 목록 쿼리에 **둘 다** 적용 |
| `LOWER(col) LIKE LOWER(?)` | SQLite의 `LIKE`는 대소문자를 무시하지만 PostgreSQL은 구분 — 명시적으로 맞춰 DB를 바꿔도 같은 결과 |
| `q.limit.min(50)` | 스펙 §4: 50 초과는 **오류가 아니라 보정**. 반면 `limit=0`은 범위 밖이라 `VALIDATION_ERROR` |
| `INSERT … RETURNING` | INSERT 후 SELECT를 한 번 더 하지 않는다 |
| `COALESCE($1, title)` | 필드 조합마다 SQL을 따로 만들지 않아도 PATCH가 된다 |
| `rows_affected() == 0` → 404 | 실제로 지워졌는지 확인. 삭제 성공은 **204 + 본문 없음** |
| `DATABASE_URL` | 연결 정보는 코드가 아니라 환경변수. 없으면 개발용 기본값 |

**실행 결과** (직접 실행한 결과):

```
s06_db  http://127.0.0.1:3006/api/posts   (DATABASE_URL=sqlite::memory:)

$ curl -i -X POST .../api/posts -d '{"title":"Hello Axum","body":"Rust Axum SQLite","tags":["rust","axum"]}'
HTTP/1.1 201 Created
location: /api/posts/1
{"data":{"id":1,"title":"Hello Axum","body":"Rust Axum SQLite","tags":["rust","axum"],"createdAt":"2026-11-22T18:56:51.717254Z","updatedAt":"2026-11-22T18:56:51.717254Z"}}

$ curl -X PATCH .../api/posts/1 -d '{"title":"Hello Axum 2"}'                  # 제목만 보냈다
{"data":{"id":1,"title":"Hello Axum 2","body":"Rust Axum SQLite","tags":["rust","axum"],...,"updatedAt":"2026-11-22T18:56:51.964846800Z"}}   ← body·tags는 그대로, updatedAt만 갱신

$ curl ".../api/posts?q=AXUM&limit=999"
{'page': 1, 'limit': 50, 'total': 1, 'totalPages': 1, 'q': 'AXUM'}  ['Hello Axum 2']     ← 대소문자 무시, limit 999 → 50
$ curl ".../api/posts?q=%27%20OR%201%3D1%20--"                                            ← SQL 인젝션 시도
{'page': 1, 'limit': 10, 'total': 0, 'totalPages': 0, 'q': "' OR 1=1 --"}                ← 전체가 쏟아지지 않는다. 그냥 검색어다

$ curl -X POST .../api/posts -d '{"title":"","body":""}'
{"error":{"code":"VALIDATION_ERROR","message":"입력값을 확인하세요.","fields":{"title":["제목은 1~100자여야 합니다"],"body":["본문은 1~5000자여야 합니다"]}}}
$ curl .../api/posts/99                                         → 404 {"error":{"code":"NOT_FOUND","message":"post 99 not found"}}
$ curl -X DELETE .../api/posts/1  (두 번)                       → 204, 404

파일 DB(DATABASE_URL=sqlite://…?mode=rwc)로 글 1개 작성 → 서버 종료 → 다시 실행 → 목록 total 1   ← 재시작 후에도 남는다
```

시험 (`cargo test --example s06_db`):

```
test tests::crud_흐름과_부분_수정 ... ok
test tests::검색은_대소문자를_무시하고_인젝션은_검색어일_뿐이다 ... ok
test tests::limit은_50으로_보정되고_검증_실패는_fields를_준다 ... ok
test result: ok. 3 passed
```

아래 3-1부터는 이 개념이 실제 프로젝트(`demo/`)에서 어떤 모양이 되는지 이어서 본다.

### 3-1. 연결 설정 — [demo/src/db.rs](demo/src/db.rs) (SQLite 부분)

```rust
pub type Db = sqlx::Sqlite;
pub type DbPool = sqlx::Pool<Db>;

pub async fn init(database_url: &str) -> anyhow::Result<DbPool> {
    let in_memory = database_url.contains(":memory:") || database_url.contains("mode=memory");

    let mut options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5));

    if !in_memory {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(if in_memory { 1 } else { 5 })
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
```

| 코드 | 설명 |
|---|---|
| `create_if_missing(true)` | **기본값이 false**라서 켜지 않으면 첫 실행부터 `unable to open database file` |
| `busy_timeout(5s)` | 쓰기 락을 기다리는 시간. 기본 0이면 동시 쓰기에서 즉시 `database is locked` |
| `journal_mode(Wal)` | 읽기와 쓰기가 서로 막지 않는다. **SQLite를 서버에서 쓸 때 사실상 필수** |
| `in_memory`일 때 `max_connections(1)` | 메모리 DB는 **연결마다 별개의 DB**. 풀 연결이 둘 이상이면 테스트가 랜덤하게 `no such table` |
| `sqlx::migrate!("./migrations")` | **컴파일 시점에** `.sql` 파일을 바이너리에 박아 넣는다. 배포 이미지에 `migrations/` 폴더가 없어도 된다 |
| `pub type Db`, `DbPool` | **DB 종류가 나오는 유일한 자리.** 나머지 코드는 별칭만 쓴다 (study11에서 PostgreSQL로 바꿀 때 이득) |

### 3-2. 마이그레이션 — 기존 파일은 고치지 않고 **새 파일을 추가**

```sql
-- migrations/0001_create_posts.sql
CREATE TABLE IF NOT EXISTS posts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    author     TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',     -- SQLite에는 배열이 없다. JSON 문자열로 저장
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
```

`0002_create_users.sql`(회원 + `posts.author_id`), `0003_add_thumbnail.sql`(`thumbnail`)이 이어지고, **스펙 v1에 맞추는 변경은 새 파일 `0004`로 쌓는다.** 적용된 마이그레이션을 수정하면 sqlx가 체크섬 불일치(`VersionMismatch`)로 거부한다.

```sql
-- migrations/0004_spec_v1.sql  (SQLite 3.25+의 RENAME COLUMN — 데이터는 그대로 남는다)
ALTER TABLE posts RENAME COLUMN content TO body;          -- 스펙의 필드 이름은 body
ALTER TABLE users RENAME COLUMN username TO nickname;     -- 로그인은 이메일, 화면 이름은 닉네임

CREATE TABLE IF NOT EXISTS comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    post_id    INTEGER NOT NULL REFERENCES posts (id) ON DELETE CASCADE,   -- 글을 지우면 댓글도
    author_id  INTEGER NOT NULL REFERENCES users (id),
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_comments_post_id ON comments (post_id);
```

`migrations_pg/0004_spec_v1.sql`은 같은 의미이고 `IDENTITY`·`TIMESTAMPTZ`만 다르다. 이미 글 25개·회원 1명이 들어 있던 **옛 `data.db`로 서버를 띄워** 시작 시 이 마이그레이션이 적용되고 글이 `body`·작성자 닉네임과 함께 그대로 나오는 것을 확인했다.

### 3-3. 모델 — [demo/src/posts/model.rs](demo/src/posts/model.rs)

```rust
/// API 응답으로 나가는 게시글.  (스펙 v1 §3)
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,                  // 예전 이름은 content
    pub tags: Vec<String>,
    pub author: Author,                // { id, nickname }  — 예전엔 문자열 + authorId
    pub comment_count: i64,            // commentCount
    pub thumbnail: Option<String>,     // 스펙 밖 확장 (study8)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 조회 쿼리의 한 행: posts + 작성자 닉네임(JOIN) + 댓글 수(서브쿼리). tags가 JSON 문자열이라 API 모양과 다르다.
#[derive(Debug, Clone, FromRow)]
pub struct PostRow { /* id, title, body, author_id, author_nickname, tags: String, thumbnail, comment_count, created_at, updated_at */ }

impl From<PostRow> for Post {
    fn from(row: PostRow) -> Self {
        Self { /* ... */
            tags: serde_json::from_str(&row.tags).unwrap_or_default(),   // 깨진 JSON이어도 500 대신 빈 배열
            author: Author { id: row.author_id, nickname: row.author_nickname },
        }
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,
    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub body: String,
    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}

/// 모든 필드가 Option → 부분 수정(PATCH)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePost { pub title: Option<String>, pub body: Option<String>, pub tags: Option<Vec<String>> /* + validate */ }
```

공용 응답 타입은 [demo/src/response.rs](demo/src/response.rs)에 있다.

```rust
pub struct Data<T> { pub data: T }                       // { "data": ... }
pub struct Author { pub id: Option<i64>, pub nickname: String }
```

| 코드 | 설명 |
|---|---|
| `#[serde(rename_all = "camelCase")]` | `comment_count → commentCount`, `created_at → createdAt`. **직렬화 경계에서만** 이름이 바뀐다. Rust 코드는 snake_case 그대로 |
| `Author.id`가 `Option` | 인증 도입 전에 쓴 글은 `author_id`가 NULL. 그런 글은 `{ "id": null, "nickname": "<그때 적은 이름>" }`으로 나간다 |
| `CreatePost`에 `author` 필드가 **없다** | 작성자는 클라이언트가 정하는 값이 아니라 **토큰에서 나온다** (study7) |
| `UpdatePost`의 `Option` | `None`이면 "안 보냈다" → 기존 값 유지 (PATCH 의미) |
| `ToSchema` | OpenAPI 문서 자동 생성용 (study14) |

### 3-4. 저장소(repo) — [demo/src/posts/repo.rs](demo/src/posts/repo.rs)

목록과 단건이 **같은 열**을 읽도록 SELECT를 한 곳(매크로)에 둔다. `query_as`는 리터럴만 받으므로 상수가 아니라 `macro_rules!`가 문자열을 만들고 `concat!`이 조건을 이어 붙인다.

```rust
macro_rules! select_post {
    () => {
        "SELECT p.id, p.title, p.body, p.author_id,
                COALESCE(u.nickname, p.author) AS author_nickname,
                p.tags, p.thumbnail,
                (SELECT COUNT(*) FROM comments c WHERE c.post_id = p.id) AS comment_count,
                p.created_at, p.updated_at
         FROM posts p LEFT JOIN users u ON u.id = p.author_id"
    };
}

/// 단건 조회. 없으면 Ok(None)
pub async fn find(db: &DbPool, id: i64) -> Result<Option<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(concat!(select_post!(), " WHERE p.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 생성. INSERT로 id를 받고, 같은 모양(JOIN·댓글 수 포함)으로 다시 읽어 돌려준다.
pub async fn insert(db: &DbPool, input: &CreatePost, author: &str, author_id: i64)
    -> Result<PostRow, sqlx::Error>
{
    let now: DateTime<Utc> = Utc::now();
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO posts (title, body, author, author_id, tags, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(&input.title).bind(&input.body).bind(author).bind(author_id)
    .bind(tags).bind(now).bind(now)
    .fetch_one(db)
    .await?;

    find(db, id).await?.ok_or(sqlx::Error::RowNotFound)
}

/// 부분 수정. COALESCE($1, title)는 "바인딩 값이 NULL이면 기존 값을 쓴다"
pub async fn update(db: &DbPool, id: i64, input: &UpdatePost) -> Result<Option<PostRow>, sqlx::Error> {
    // UPDATE ... SET title = COALESCE($1, title), body = COALESCE($2, body), tags = COALESCE($3, tags), updated_at = $4 WHERE id = $5
    // rows_affected() == 0 이면 Ok(None), 아니면 find(db, id)
}

pub async fn delete(db: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM posts WHERE id = $1").bind(id).execute(db).await?;
    Ok(result.rows_affected() > 0)       // 댓글은 FK의 ON DELETE CASCADE가 함께 지운다
}
```

| 코드 | 설명 |
|---|---|
| `$1, $2 …` | 바인딩 자리. **값을 SQL 문자열에 끼워 넣지 않는다 → SQL 인젝션 방어.** (SQLite도 `$1`을 받는다 — study11에서 PostgreSQL과 같은 SQL을 쓰는 이유) |
| `LEFT JOIN users` + `COALESCE(u.nickname, p.author)` | 가입 회원이면 **현재 닉네임**, 옛 글이면 그때 적은 이름 |
| 서브쿼리 `(SELECT COUNT(*) …) AS comment_count` | 스펙의 `commentCount`. 정렬(`-commentCount`)도 이 별칭으로 한다 |
| INSERT는 `RETURNING id`만 | JOIN·집계가 붙은 모양은 `RETURNING`으로 못 만든다 → 같은 `find`로 다시 읽는다 (단건과 목록이 같은 모양임을 보장) |
| `fetch_optional` | 0건이면 `Ok(None)` → 핸들러가 404로 번역 |
| `COALESCE($1, title)` | 필드 조합마다 SQL을 따로 만들지 않아도 PATCH가 된다 |
| `rows_affected() > 0` | 실제로 지워졌는지 확인 → 없는 id면 404 |

> ⚠️ **sqlx 0.9에서 `format!`으로 SQL을 만들면 컴파일 에러**(`SqlSafeStr`)다. 동적 조건(검색·필터)은 `QueryBuilder`로 쓴다. 정렬 기준도 사용자가 준 문자열을 붙이지 않는다. 아래 3-6의 `Sort` enum이 고른 **고정 문자열**만 `push`한다.

### 3-5. 핸들러 — [demo/src/posts/handler.rs](demo/src/posts/handler.rs)

```rust
pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Data<Post>>> {
    repo::find(&state.db, id).await?                       // sqlx 오류는 ? 로 AppError::Database → 500
        .map(|row| Json(Data::new(Post::from(row))))       // { "data": ... } 로 감싼다
        .ok_or_else(|| AppError::not_found("post", id))    // 없으면 404
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,                                        // 토큰이 없거나 틀리면 여기서 401 (study7)
    ValidatedJson(body): ValidatedJson<CreatePost>,        // 본문 extractor는 마지막
) -> AppResult<impl IntoResponse> {
    let row = repo::insert(&state.db, &body, &auth.nickname, auth.id).await?;
    let location = format!("/api/posts/{}", row.id);       // 스펙: Location: /api/posts/{id}
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(Data::new(Post::from(row)))))
}
```

`update`는 먼저 `ensure_owner`(없으면 404, 남의 글이면 403)를 거친 뒤 `repo::update`, `delete`는 성공 시 **204 + 본문 없음**.

**경로 조립** — [demo/src/lib.rs](demo/src/lib.rs). `/api` 아래 라우터에만 호출 제한을 건다.

```rust
let api = Router::new()
    .nest("/auth", auth::router())          // signup · login · logout · me
    .nest("/posts", posts::router())        // 목록 · 단건 · 작성 · 수정 · 삭제 · 썸네일 · /{id}/comments
    .nest("/comments", comments::router())  // DELETE /{id}
    .layer(axum::middleware::from_fn_with_state(state.clone(), ratelimit::rate_limit));

Router::new()
    .route("/health", get(routes::health))  // 스펙: 접두어 없이
    .nest("/api", api)
    .merge(SwaggerUi::new("/api-docs").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()))
```

### 3-6. 목록 쿼리 — 응답 껍데기와 `limit` 상한, `sort` — [demo/src/pagination.rs](demo/src/pagination.rs)

```rust
pub const MAX_LIMIT: u32 = 50;

#[derive(Debug, Deserialize, Validate, IntoParams)]
pub struct PageQuery {
    #[serde(default = "default_page")]  #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    pub page: u32,
    #[serde(default = "default_limit")] #[validate(range(min = 1, message = "limit는 1 이상이어야 합니다"))]
    pub limit: u32,                                         // 상한은 오류가 아니라 보정이다
    pub q: Option<String>,
    #[validate(custom(function = "validate_sort"))]
    pub sort: Option<String>,
    pub tag: Option<String>,                                // 스펙 밖 확장
}

impl PageQuery {
    pub fn effective_limit(&self) -> u32 { self.limit.clamp(1, MAX_LIMIT) }   // 1000을 보내면 50
    pub fn sort(&self) -> Sort { /* 기본 -createdAt */ }
}

pub enum Sort { CreatedAtDesc, CreatedAtAsc, CommentCountDesc }
impl Sort {
    pub fn order_by(self) -> &'static str {                 // 사용자 문자열이 아니라 이 세 가지 고정 SQL 조각만 나간다
        match self {
            Self::CreatedAtDesc => " ORDER BY p.created_at DESC, p.id DESC",
            Self::CreatedAtAsc => " ORDER BY p.created_at ASC, p.id ASC",
            Self::CommentCountDesc => " ORDER BY comment_count DESC, p.created_at DESC, p.id DESC",
        }
    }
}

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct PageMeta { pub page: u32, pub limit: u32, pub total: i64, pub total_pages: u32, pub q: Option<String> }
#[derive(Serialize)]
pub struct Paged<T> { pub data: Vec<T>, pub meta: PageMeta }
```

| 코드 | 설명 |
|---|---|
| `clamp(1, MAX_LIMIT)` | 스펙 §4 "50 초과는 50으로". **오류(400)가 아니라 조용한 보정**이다. 반면 `limit=0`은 범위 밖이라 `VALIDATION_ERROR` |
| `validate_sort` + `Sort::parse` | 허용 목록에 없는 값(`sort=title;drop`)은 `VALIDATION_ERROR`. SQL에는 닿지도 않는다 |
| `ORDER BY comment_count` | SELECT의 별칭으로 정렬한다. 댓글이 같으면 최신순 |
| `total_pages = ceil(total / limit)` | `(total + limit - 1) / limit`. 결과가 없으면 0 |

### 3-7. ⭐ 오류 응답을 스펙 v1 모양으로 — [demo/src/error.rs](demo/src/error.rs)

```rust
#[derive(Debug, Error)]
pub enum AppError {
    #[error("{resource} {id} not found")]          NotFound { resource: &'static str, id: String },
    #[error("{0}")]                                BadRequest(String),
    #[error("{message}")]                          InvalidBody { status: StatusCode, message: String },
    #[error("validation failed")]                  Validation(validator::ValidationErrors),
    #[error("{0}")]                                Unauthorized(String),
    #[error("이메일 또는 비밀번호가 올바르지 않습니다")] InvalidCredentials,
    #[error("{0}")]                                Forbidden(String),
    #[error("이미 사용 중인 이메일입니다")]           EmailTaken,
    #[error("{0}")]                                Conflict(String),
    #[error("요청이 너무 많습니다. 잠시 후 다시 시도하세요.")] RateLimited { retry_after: u64 },
    #[error("이 경로에서 지원하지 않는 HTTP 메서드입니다")] MethodNotAllowed,
    #[error(transparent)]                          Database(#[from] sqlx::Error),
    #[error(transparent)]                          Internal(#[from] anyhow::Error),
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope { pub error: ErrorDetail }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub fields: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub request_id: Option<String>,
}
```

변환표 (`status()` / `code()`):

| `AppError` | 상태 | `error.code` |
|---|---|---|
| `NotFound` | 404 | `NOT_FOUND` |
| `BadRequest` | 400 | `BAD_REQUEST` |
| `InvalidBody` (axum의 400/422) | **400** (422는 400으로 정규화) | `BAD_REQUEST` |
| `InvalidBody` (413 / 415) | 413 / 415 | `PAYLOAD_TOO_LARGE` / `UNSUPPORTED_MEDIA_TYPE` |
| `Validation` | **400** | `VALIDATION_ERROR` (+ `fields`) |
| `Unauthorized` | 401 | `UNAUTHORIZED` |
| `InvalidCredentials` | 401 | `INVALID_CREDENTIALS` |
| `Forbidden` | 403 | `FORBIDDEN` |
| `EmailTaken` | 409 | `EMAIL_TAKEN` |
| `Conflict` (닉네임 중복 등) | 409 | `CONFLICT` |
| `RateLimited` | **429** (+ `Retry-After` 헤더) | `RATE_LIMITED` |
| `MethodNotAllowed` | 405 | `METHOD_NOT_ALLOWED` |
| `Database` / `Internal` | 500 | `INTERNAL` (원인은 **로그에만**, 응답 메시지는 고정 문구) |

`requestId`는 핸들러가 요청 헤더를 모르기 때문에 **응답이 나가는 길목의 미들웨어**가 채운다 ([demo/src/middleware.rs](demo/src/middleware.rs)):

```rust
pub async fn error_envelope(request: Request, next: Next) -> Response {
    let request_id = request.headers().get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok()).map(str::to_owned);

    let response = next.run(request).await;
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;                                   // 성공 응답은 건드리지 않는다
    }

    let (mut parts, body) = response.into_parts();
    let bytes = to_bytes(body, ERROR_BODY_LIMIT).await.unwrap_or_default();

    // 이미 { "error": { ... } } 이면 requestId만 채우고, 아니면(빈 본문 408, 경로 id가 숫자가 아닐 때 axum의 평문 400 …) 새로 감싼다
    let mut value = serde_json::from_slice::<Value>(&bytes).ok()
        .filter(|v| v.get("error").is_some_and(|e| e.is_object()))
        .unwrap_or_else(|| { /* ErrorEnvelope::new(code_for_status(status), message) */ });

    if let (Some(id), Some(error)) = (request_id, value.get_mut("error")) {
        error["requestId"] = Value::String(id);
    }

    parts.headers.remove(header::CONTENT_LENGTH);          // 본문이 바뀌었으니 길이를 다시 계산하게
    parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Response::from_parts(parts, Body::from(value.to_string()))
}
```

이 미들웨어는 [demo/src/lib.rs](demo/src/lib.rs)에서 **`CompressionLayer`보다 안쪽, `CatchPanicLayer`·`TimeoutLayer`보다 바깥**에 둔다 (이유는 study8의 순서 규칙).

### 3-8. 호출 제한 — 429 `RATE_LIMITED` — [demo/src/ratelimit.rs](demo/src/ratelimit.rs)

고정 창(fixed window) 방식이다. IP마다 "창 시작 시각 + 횟수"만 기억하고, 창(60초)이 지나면 0으로 되돌린다.

```rust
pub fn check(&self, key: &str) -> Result<(), u64> {        // Err(다시 시도할 때까지 남은 초)
    if self.limit == 0 { return Ok(()); }                  // 0이면 제한 없음 (기본값, 테스트)
    let now = Instant::now();
    let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
    let entry = hits.entry(key.to_string()).or_insert((now, 0));
    if now.duration_since(entry.0) >= self.window { *entry = (now, 0); }   // 새 창
    if entry.1 >= self.limit {
        return Err(self.window.saturating_sub(now.duration_since(entry.0)).as_secs().max(1));
    }
    entry.1 += 1;
    Ok(())
}

pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let key = request.extensions().get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_string())
        .unwrap_or_else(|| "local".to_string());           // 서버를 띄우지 않는 테스트(oneshot)에는 연결 정보가 없다
    match state.rate_limiter.check(&key) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => AppError::RateLimited { retry_after }.into_response(),
    }
}
```

| 코드 | 설명 |
|---|---|
| `std::sync::Mutex` | 락을 잡은 채로 `await`하지 않으므로 tokio Mutex가 아니어도 된다 |
| 키 = **연결의 원격 IP** | `X-Forwarded-For`는 클라이언트가 마음대로 적을 수 있어서 신뢰할 수 있는 프록시 뒤가 아니면 쓰지 않는다 |
| `into_make_service_with_connect_info::<SocketAddr>()` ([demo/src/main.rs](demo/src/main.rs)) | **이걸 안 켜면 `ConnectInfo`가 없어서 모든 요청이 키 `"local"` 하나를 공유한다.** 한 사람이 한도를 다 쓰면 모두가 429 |
| `MAX_TRACKED_KEYS` 도달 시 만료 항목 정리 | IP를 무한히 만들어 메모리를 채우는 공격 방지 |
| `/health`는 제한 밖 | 배포 점검이 제한에 걸려 실패하면 안 된다 (`/api` 라우터에만 건다) |
| 한계 | 프로세스 메모리에만 있다 → **서버가 여러 대면 대수만큼 허용량이 늘어난다.** 그때는 Redis 같은 공유 저장소 (study15) |

### 3-9. 환경변수로 분리 — [demo/src/config.rs](demo/src/config.rs)

```rust
pub struct Config {
    pub database_url: String, pub jwt_secret: String, pub bind_addr: String, pub upload_dir: String,
    pub cors_origin: String,             // 스펙 §7: 쉼표로 여러 개
    pub rate_limit_per_minute: u32,      // 0이면 제한 없음
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            database_url: env_or("DATABASE_URL", DEFAULT_DATABASE_URL),
            jwt_secret:   env_or("JWT_SECRET", DEV_JWT_SECRET),
            bind_addr:    env_or("BIND_ADDR", "127.0.0.1:3000"),
            upload_dir:   env_or("UPLOAD_DIR", "uploads"),
            cors_origin:  env_or("CORS_ORIGIN", DEFAULT_CORS_ORIGIN),                    // http://localhost:5173
            rate_limit_per_minute: env_or("RATE_LIMIT_PER_MINUTE", "300").parse()
                .context("RATE_LIMIT_PER_MINUTE는 0 이상의 정수여야 합니다")?,
        };
        config.validate()?;          // 너무 짧은 비밀키, 헤더로 쓸 수 없는 CORS 출처는 서버가 뜨기 전에 거절
        Ok(config)
    }
}
```

설정은 **한 곳에서, 시작할 때 한 번** 읽고 검증한다. CORS 출처는 `AppState::with_cors_origin`을 거쳐 `cors_layer(&origins)`가 `Vec<HeaderValue>`로 만든다. 키 이름 목록은 [demo/.env.example](demo/.env.example).

### 3-10. 실행 결과 (실제 서버 + curl, 새 SQLite DB)

```
$ curl -X POST .../api/auth/signup -d '{"email":"alice@example.com","password":"supersecret123","nickname":"alice"}'
{"data":{"id":1,"email":"alice@example.com","nickname":"alice","createdAt":"2026-11-22T18:21:43.921447400Z"}}

$ ... 같은 이메일로 다시                                           # 중복 이메일
{"error":{"code":"EMAIL_TAKEN","message":"이미 사용 중인 이메일입니다","requestId":"ad5b91a8-382d-4a00-a48f-bbc64a76ea7d"}}
$ ... 같은 닉네임, 다른 이메일                                       # 중복 닉네임은 다른 코드
{"error":{"code":"CONFLICT","message":"이미 사용 중인 닉네임입니다","requestId":"d19f188d-d003-4d0e-8a82-f9a809cde088"}}

$ curl -X POST .../api/auth/login -d '{"email":"alice@example.com","password":"supersecret123"}'
{"data":{"user":{"id":1,"email":"alice@example.com","nickname":"alice","createdAt":"..."},"accessToken":"eyJ0eXAiOiJKV1QiLCJh..."}}

$ curl -i -X POST .../api/posts -H "authorization: Bearer $TOKEN" -d '{"title":"Hello Axum","body":"Rust Axum Postgres","tags":["rust","axum"]}'
HTTP/1.1 201 Created
location: /api/posts/1
x-request-id: 7ac320fd-e838-49df-8f83-078b78e7448e
{"data":{"id":1,"title":"Hello Axum","body":"Rust Axum Postgres","tags":["rust","axum"],"author":{"id":1,"nickname":"alice"},
 "commentCount":0,"thumbnail":null,"createdAt":"2026-11-22T18:21:45.809879600Z","updatedAt":"2026-11-22T18:21:45.809879600Z"}}

$ curl .../api/posts/1/comments                                    # 댓글 2개 작성 후
{"data":[{"id":1,"postId":1,"body":"comment a","author":{"id":1,"nickname":"alice"},"createdAt":"..."},{"id":2,"postId":1,"body":"comment b", ...}]}

$ curl ".../api/posts?q=HELLO"                                     # 대소문자 무시, meta.q에 그대로
{"data":[{"id":1,"title":"Hello Axum", ..., "commentCount":2, ...}],"meta":{"page":1,"limit":10,"total":1,"totalPages":1,"q":"HELLO"}}

$ curl ".../api/posts?sort=-commentCount&limit=999"
[('Hello Axum', 2), ('second', 0)]   {'page': 1, 'limit': 50, 'total': 2, 'totalPages': 1, 'q': None}      ← limit 999 → 50

$ curl -X POST .../api/posts -H "authorization: Bearer $TOKEN" -d '{"title":"","body":""}'                  # 검증 실패 → 400
{"error":{"code":"VALIDATION_ERROR","fields":{"body":["본문은 1~5000자여야 합니다"],"title":["제목은 1~100자여야 합니다"]},
          "message":"입력값을 확인하세요.","requestId":"627d37a8-6fec-43e4-bff0-3cc52fab33cb"}}

$ curl ".../api/posts?sort=bad"                                    → 400 VALIDATION_ERROR, fields.sort = ["sort는 createdAt, -createdAt, -commentCount 중 하나여야 합니다"]
$ curl -i .../api/posts/abc                                        → 400 {"error":{"code":"BAD_REQUEST","message":"Invalid URL: Cannot parse `abc` to a `i64`","requestId":"ec58af7f-..."}}
$ curl .../posts                                                   → 404 NOT_FOUND   (접두어 없는 옛 경로는 사라졌다)
$ curl -X DELETE .../api/posts/1 -H "authorization: Bearer $TOKEN"                                         → 204
$ curl .../api-docs/openapi.json   paths: /api/auth/{login,logout,me,signup}, /api/comments/{id}, /api/posts, /api/posts/{id}, /api/posts/{id}/comments, /api/posts/{id}/thumbnail
```

호출 제한과 CORS (`RATE_LIMIT_PER_MINUTE=3`, `CORS_ORIGIN=https://front.example.com`로 띄운 서버):

```
GET /api/posts  1~3번째 → 200 200 200      4번째 → 429
HTTP/1.1 429 Too Many Requests
retry-after: 59
{"error":{"code":"RATE_LIMITED","message":"요청이 너무 많습니다. 잠시 후 다시 시도하세요.","requestId":"99ef0d14-a78c-4843-a926-014c7ce00c44"}}
GET /health → 200                          ← 제한 밖
OPTIONS (Origin: https://front.example.com) → access-control-allow-origin: https://front.example.com
OPTIONS (Origin: http://localhost:5173)     → 허용 헤더 없음  (기본 출처는 설정으로 대체됐다)
```

AppError를 거치지 않는 4xx·5xx도 같은 모양이 된다 (`error_envelope`의 효과, 새 서버로 확인):

```
GET /nope  (accept-encoding: gzip)  → 404 {"error":{"code":"NOT_FOUND","message":"route (unknown) not found","requestId":"d93925ff-a593-4c6c-b0a4-67080aa23405"}}   (gzip 응답 + 본문 재작성 동작)
GET /panic  (x-request-id: panic-1) → 500 {"error":{"code":"INTERNAL","message":"서버 내부 오류가 발생했습니다.","requestId":"panic-1"}}
GET /slow                           → 408 {"error":{"code":"REQUEST_TIMEOUT","message":"요청 처리 시간이 초과되었습니다.","requestId":"69af6b50-705d-45b5-afb4-89b4b4439543"}}
DELETE /health                      → 405 {"error":{"code":"METHOD_NOT_ALLOWED","message":"이 경로에서 지원하지 않는 HTTP 메서드입니다","requestId":"f8df2ed6-..."}}
POST /api/auth/login  (본문 '{bad') → 400 {"error":{"code":"BAD_REQUEST","message":"Failed to parse the request body as JSON: key must be a string at line 1 column 2","requestId":"5cbbc9ca-..."}}
```

---

## ④ 🔗 연동 포인트 — 공통 API 스펙 v1과 맞추기

### 4-1. 맞춘 것

| 스펙 v1 | 상태 |
|---|---|
| 경로 표 §1 — `/api/auth/{signup,login,logout,me}`, `/api/posts[/{id}]`, `/api/posts/{id}/comments`, `/api/comments/{id}`, `/health` | ✅ |
| 요청 본문 §2 — `signup {email,password,nickname}`, `login {email,password}`, 글 `{title,body,tags}` | ✅ |
| 성공 응답 §3 — `{ data }`, `{ data, meta:{page,limit,total,totalPages,q} }`, 로그인 `{ data:{user,accessToken} }`, `author:{id,nickname}`, `commentCount` | ✅ |
| 쿼리 §4 — `page`, `limit`(기본 10·**최대 50으로 보정**), `q`(대소문자 무시), `sort` | ✅ |
| 오류 본문 §5 `{ error: { code, message, fields, requestId } }` + 코드표(`RATE_LIMITED` 429 포함) | ✅ (`AppError` + `error_envelope`) |
| 응답 JSON camelCase (§2) | ✅ (`#[serde(rename_all = "camelCase")]`) |
| 토큰 만료 1시간 (§6) | ✅ (`TOKEN_TTL_SECONDS = 3600`) |
| `X-Request-Id` 응답 헤더 = 오류 본문의 `requestId` (§7) | ✅ 같은 값 (테스트로 검증) |
| CORS 출처 환경변수 `CORS_ORIGIN` (§7) | ✅ (쉼표로 여러 개) |
| 문서 `/api-docs` (§7) | ✅ (Swagger UI는 `/api-docs/`, 명세는 `/api-docs/openapi.json`) |

### 4-2. 확인 결과 기록 — 경로와 목록 응답 형식 (Notion "정리 영역"에 같은 표를 기록했다)

**`demo/` (게시판 API, 포트 3000)** — 이 단계 전에는 아래 "예전" 칸이었고, 지금은 모두 맞다.

| 항목 | 스펙 v1 | 예전 `demo/` | 지금 |
|---|---|---|---|
| Base URL | `/api` | 없음 (`/posts`, `/auth/*`) | ✅ |
| 회원가입 / 로그인 | `signup {email,password,nickname}` / `login {email,password}` | `register {username,email,password}` / `login {username,password}` | ✅ |
| 로그아웃 / 내 정보 | `POST /api/auth/logout`, `GET /api/auth/me` | `me`만 | ✅ (로그아웃은 204, 아래 참고) |
| 목록 | `{ data, meta }`, `page`·`limit`(≤50)·`sort`·`q` | `{ items, page, perPage, total, totalPages, hasNext }`, `per_page`(≤100) | ✅ |
| 단건·생성 | `{ data }`, `author:{id,nickname}`, `commentCount`, 필드 `body` | 객체 그대로, `author` 문자열 + `authorId`, 필드 `content` | ✅ |
| 댓글 | `/api/posts/{id}/comments`, `DELETE /api/comments/{id}` | 없음 | ✅ |
| 로그인 응답 | `{ data:{user,accessToken} }` | `{ accessToken, tokenType, expiresIn, user }` | ✅ |
| 오류 본문 · 코드 · `requestId` · camelCase | §5, §2, §7 | 맞춤 | ✅ |
| 헬스 · PATCH 200 · DELETE 204 · 201 `Location` | — | 맞춤 | ✅ (`Location`은 `/api/posts/{id}`) |
| CORS 출처 | 환경변수 `CORS_ORIGIN` | 코드에 고정 | ✅ |
| 429 `RATE_LIMITED` | 호출 제한 | 없음 | ✅ |
| 문서 | `/api-docs` | `/docs` | ✅ |

**스펙에서 한 걸음 벗어난 곳 (의도적 / 한계)**

| 곳 | 내용 |
|---|---|
| 확장 | 글의 `thumbnail`, 쿼리 `tag`, 썸네일 업로드 `/api/posts/{id}/thumbnail`은 스펙 밖의 **추가**다. 스펙이 정한 필드를 바꾸지는 않는다 |
| `author.id`가 `null` | 인증 도입 전(study5)에 쓴 글. 스펙은 숫자라고 하지만 옛 데이터를 지울 수는 없다 |
| 닉네임 UNIQUE | 스펙의 409 코드는 `EMAIL_TAKEN`뿐이라 닉네임 중복은 `CONFLICT`로 구분한다. 컬럼을 `username`에서 이름만 바꿨기 때문에 UNIQUE가 남았다 (SQLite는 제약을 떼려면 테이블을 다시 만들어야 한다) |
| 로그아웃 | JWT는 서버가 상태를 갖지 않아서 **서버가 지울 것이 없다** (그래도 인증은 요구하고 204를 준다). 만료 전 토큰을 즉시 무효화하려면 "취소된 토큰 목록"이 필요하다 |
| 댓글 `Location` | 스펙은 글 작성의 `Location`만 정한다. 댓글은 `/api/posts/{id}/comments`(목록)를 준다 |

**`project/` (최종 프로젝트, 포트 4000)** — 같은 스펙을 `domain`/`web` 두 크레이트로 맞췄다. 자세한 구조는 [studyFinal](studyFinal.md).

| 항목 | 스펙 v1 | 지금 `project/` | 맞춤 |
|---|---|---|---|
| Base URL · 헬스 | `/api` · `/health`(접두어 없이) | `/api` · `/health` | ✅ |
| 인증 | `signup`/`login`(이메일)/`logout`/`me` | 같음. Bearer 또는 httpOnly 쿠키 둘 다 받고 **본문 형식은 같다**(`accessToken`도 준다) | ✅ (스펙 §6) |
| 목록 · 단건 | `{ data, meta }`, `{ data }`, `limit`≤50, `sort`, `q` | 같음 | ✅ |
| 글 | `tags`, `author:{id,nickname}`, `commentCount` | 같음 (`tags` 컬럼과 `comments` 테이블을 마이그레이션 `m0002`로 추가) | ✅ |
| 댓글 | `/api/posts/{id}/comments`, `/api/comments/{id}` | 같음 | ✅ |
| 오류 | `{ error: { code, message, fields, requestId } }` | 같음 (`DomainError` → `ApiError`가 코드를 번역, `error_envelope`가 `requestId`) | ✅ |
| 429 | `RATE_LIMITED` | 같음 | ✅ |

### 4-3. 연결 정보는 환경변수로

| 키 | 용도 | 기본값(개발) | 운영 |
|---|---|---|---|
| `DATABASE_URL` | DB 주소 | `sqlite://data.db?mode=rwc` | 실행 시 주입 (PostgreSQL이면 `postgres://…`) |
| `JWT_SECRET` | 토큰 서명 키 | 개발용 문자열 (**운영에서 쓰면 경고**) | **이미지에 굽지 않고** 실행 시 주입 |
| `BIND_ADDR` | 바인딩 주소 | `127.0.0.1:3000` | 컨테이너는 `0.0.0.0:3000` |
| `UPLOAD_DIR` | 업로드 폴더 | `uploads` | 볼륨 경로 |
| `CORS_ORIGIN` | 허용할 프론트 출처 (쉼표로 여러 개) | `http://localhost:5173` | 실제 프론트 주소 |
| `RATE_LIMIT_PER_MINUTE` | IP당 분당 요청 수 (0 = 제한 없음) | `300` | 서비스 성격에 맞게 |
| `RUST_LOG` | 로그 레벨 | `demo=debug,…` | `info` |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| `unable to open database file` | `create_if_missing` 기본값이 **false** | `.create_if_missing(true)` 또는 URL에 `?mode=rwc` |
| `database is locked` (가끔, 동시 요청에서 랜덤) | 동시 쓰기 + `busy_timeout` 0 + 기본 저널 모드 | WAL + `busy_timeout`. 근본적으로 SQLite는 **동시에 쓰는 트랜잭션이 하나뿐** — 쓰기가 많으면 PostgreSQL (study11) |
| 테스트가 랜덤하게 `no such table` | in-memory SQLite는 **연결마다 별개의 DB**인데 풀이 2개 이상 | 메모리 DB는 `max_connections(1)` |
| `error[E0277]: dynamic SQL strings should be audited for possible injections` | sqlx 0.9는 `query`/`query_as`에 **`&'static str`만** 받음. `format!`로 SQL 조립 | 값은 `$1` 바인딩, 동적 조건은 `QueryBuilder`, **고정 문자열 조합은 `concat!` + `macro_rules!`**. 에러 메시지 자체가 안내문이다 |
| `ColumnNotFound("tags")` / `ColumnDecode { index: "created_at" }` | `query_as`는 **런타임에** 매핑. SELECT에 컬럼을 안 넣었거나 별칭이 `FromRow` 필드와 다름 | `SELECT *` 대신 컬럼을 명시하고 `AS author_nickname`처럼 구조체 필드와 이름을 맞춘다 |
| `VersionMismatch(1)` | **이미 적용된** 마이그레이션 파일을 수정함 (체크섬 불일치) | 개발 중이면 `data.db*` 삭제 후 재실행, 운영이면 **새 마이그레이션 파일 추가** (그래서 이번 변경도 `0004`로 쌓았다) |
| 검증 실패 응답이 422인 줄 알았는데 400 | 스펙 v1은 `VALIDATION_ERROR`를 400으로 정함. axum 기본은 422 | `AppError::status()`가 `InvalidBody`의 422를 400으로 정규화 (study5) |
| `limit=1000`이 400이 아니라 200 | 스펙 §4가 "50 초과는 **50으로**"라고 정함 — 오류가 아니라 보정 | `effective_limit()`이 `clamp`. 대신 `limit=0`은 범위 밖이라 400 |
| 오류 JSON에 `requestId`가 없음 | `error_envelope`가 `SetRequestIdLayer`보다 **바깥**에 있어서 요청에 아직 `x-request-id`가 없음 | 순서: `SetRequestId` → … → `Compression` → `error_envelope` → `CatchPanic` → `Timeout` (아래 study8) |
| gzip 요청에서만 오류 본문 파싱이 깨짐 | `error_envelope`가 **압축된 바이트**를 JSON으로 읽으려 함 | `CompressionLayer`를 `error_envelope`보다 **바깥**으로 |
| 한 사람이 한도를 쓰자 **모두** 429 | 호출 제한의 키가 전부 `"local"`. `into_make_service_with_connect_info`를 안 써서 `ConnectInfo`가 없다 | `axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())` |
| `RENAME COLUMN` 뒤에도 PostgreSQL 오류에 `users_username_key`가 보임 | 컬럼 이름을 바꿔도 **UNIQUE 제약의 이름**은 그대로 | 중복 닉네임을 가르는 코드가 `nickname`뿐 아니라 `username`도 본다. 새로 만든다면 제약 이름까지 같이 바꾼다 |
| 500 응답에 DB 오류 문구가 그대로 노출 | `Database(err)`를 `to_string()`으로 응답에 넣음 | 응답은 고정 문구, 원인은 `tracing::error!`로 **로그에만** |
| `.env`의 `RUST_LOG`가 안 먹음 | `EnvFilter`는 초기화 시점에 한 번만 읽음 | `load_dotenv()`를 `init_tracing()`보다 먼저 |
| 옛 `data.db`만 복사했더니 빈 DB | WAL 모드라 최근 쓰기는 `data.db-wal`에 있다 | `data.db`, `-wal`, `-shm`을 **같이** 복사 (study16의 백업 함정과 같다) |

---

## ⑥ 직접 해보기

1. **두 가지 409 구분**: 같은 이메일로, 그다음 같은 닉네임·다른 이메일로 가입해 `EMAIL_TAKEN`과 `CONFLICT`가 나오는지 `curl`로 확인한다. 그다음 [demo/src/auth/handler.rs](demo/src/auth/handler.rs)의 `map_unique_violation`에서 `message.contains("nickname")` 줄을 지우면 어떤 시험이 깨지는지 `cargo test`로 본다.
2. **정렬이 바뀌는 걸 눈으로**: 글 3개를 만들고 `?sort=-commentCount`로 목록을 본다. 가장 오래된 글에 댓글을 달면 순서가 어떻게 바뀌는지 확인하고, 같은 댓글 수일 때는 최신순인지도 본다.
3. **429 재현**: `RATE_LIMIT_PER_MINUTE=5`로 서버를 띄우고 `/api/posts`를 6번 부른다. `Retry-After`가 몇 초인지 보고, 그 시간만큼 기다린 뒤 다시 200이 되는지 확인한다. `into_make_service_with_connect_info` 줄을 지우면 어떻게 달라지는지도 해 본다.
4. **단계 예제로 실험**: `s06_db`에서 `s06_migrations/0001_create_posts.sql`의 컬럼 하나를 일부러 고치고 다시 실행해 `VersionMismatch`를 보고, 원래대로 되돌린 뒤 **새 파일** `0003_add_views.sql`로 컬럼을 추가한다 (적용된 마이그레이션은 고치지 않고 쌓는다는 규칙을 몸으로 확인).

---

## ⑦ ☐ 체크리스트

- ☐ `create_if_missing`, WAL, `busy_timeout`, in-memory `max_connections(1)`의 이유를 각각 설명할 수 있다
- ☐ 마이그레이션은 **새 파일 추가**가 원칙이고 적용된 파일 수정은 `VersionMismatch`라는 것을 안다
- ☐ `PostRow` ↔ `Post` ↔ `CreatePost`를 나누는 이유를 설명할 수 있다
- ☐ 목록·상세·등록·수정·삭제와 댓글을 `curl`로 직접 호출해 확인했다
- ☐ `INSERT … RETURNING`, `COALESCE`, `rows_affected`가 각각 무엇을 해결하는지 안다
- ☐ `AppError`의 갈래 → 상태 코드·`code` 표를 보고 새 갈래를 추가할 수 있다
- ☐ 연결 정보·비밀 값이 코드가 아니라 환경변수에 있다
- ☐ `AppError`의 JSON 본문만 `{ error: { code, message, fields, requestId } }`로 맞추면 된다 (이미 enum + `IntoResponse`)
- ☐ 경로와 목록 응답 형식을 확인해 이 페이지 정리 영역에 기록
- ☐ 응답 JSON은 `#[serde(rename_all = "camelCase")]`
- ☐ `{ data }` / `{ data, meta }` 껍데기와 `limit` 상한(50으로 **보정**)·`sort` 허용 목록이 코드로 옮겨져 있다
- ☐ 호출 제한이 IP마다 따로 세어지고, 429에 `Retry-After`가 붙는 것을 확인했다 (`ConnectInfo`가 필요한 이유를 안다)
- ☐ `cargo run --example s06_db`를 실행해 CRUD·검색·인젝션 시도·`limit` 보정·검증 실패를 `curl`로 직접 확인했다 (파일 DB로 재시작 후 데이터가 남는 것까지)

---

## ⑧ 다음 단계 연결

CRUD가 돌아가지만 **누구나** 글을 쓰고 지울 수 있다. [study7](study7.md)에서는 회원가입·로그인(Argon2 + JWT)과 `AuthUser` extractor로 쓰기 API를 보호하고, 본인 글만 고치게(403) 하며, CORS·XSS·CSRF의 기초를 다룬다.
