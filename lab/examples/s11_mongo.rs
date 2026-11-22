//! study11 — MongoDB(문서 DB)로 게시판 CRUD.
//!
//! 실행:  docker run -d --name axum-mongo -p 27017:27017 mongo:7
//!        cargo run --example s11_mongo        →  http://127.0.0.1:3011
//!
//! 환경변수: MONGODB_URI (기본 mongodb://127.0.0.1:27017)
//!
//! 관계형 DB와 달라지는 점
//!   - 스키마·마이그레이션이 없다. 컬렉션은 첫 쓰기 때 만들어진다.
//!   - `tags`는 JSON 문자열이 아니라 **진짜 배열**로 저장된다. (SQLite에서는 문자열로 우회했다)
//!   - id는 `ObjectId`(12바이트). API에서는 24자리 16진 문자열로 주고받는다.
//!   - 쿼리는 SQL 문자열이 아니라 **BSON 문서**(`doc! { ... }`)다.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::TryStreamExt;
use mongodb::{
    Client, Collection,
    bson::{DateTime, doc, oid::ObjectId},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// DB에 저장되는 모양. Rust 필드 `id` ↔ MongoDB의 `_id`.
#[derive(Debug, Serialize, Deserialize)]
struct PostDoc {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    id: Option<ObjectId>,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at: DateTime,
}

/// API로 나가는 모양. (DB 모양과 API 모양을 나누는 원칙은 SQL일 때와 같다)
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PostOut {
    id: String,
    title: String,
    body: String,
    tags: Vec<String>,
    created_at: String,
}

impl From<PostDoc> for PostOut {
    fn from(d: PostDoc) -> Self {
        Self {
            id: d.id.map(|o| o.to_hex()).unwrap_or_default(),
            title: d.title,
            body: d.body,
            tags: d.tags,
            created_at: d.created_at.try_to_rfc3339_string().unwrap_or_default(),
        }
    }
}

#[derive(Clone)]
struct AppState {
    posts: Collection<PostDoc>,
}

#[tokio::main]
async fn main() {
    let uri = std::env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".into());
    let client = Client::with_uri_str(&uri).await.expect("MongoDB URI 오류");
    let state = AppState { posts: client.database("board").collection::<PostDoc>("posts") };

    let app = Router::new()
        .route("/api/posts", get(list).post(create))
        .route("/api/posts/{id}", get(get_one).patch(update).delete(remove))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3011").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ── 오류: 공통 API 스펙 v1 모양 ──────────────────────────────────────────────

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    // 원인은 로그에만, 응답은 고정 문구
    eprintln!("[mongo error] {e}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "서버 내부 오류가 발생했습니다.")
}

/// 경로의 id 문자열 → ObjectId. 형식이 틀리면 404가 아니라 **400**이다.
fn parse_id(raw: &str) -> Result<ObjectId, Response> {
    ObjectId::parse_str(raw).map_err(|_| error(StatusCode::BAD_REQUEST, "BAD_REQUEST", "id 형식이 올바르지 않습니다."))
}

// ── 핸들러 ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NewPost {
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create(State(state): State<AppState>, Json(body): Json<NewPost>) -> Response {
    if body.title.trim().is_empty() || body.title.chars().count() > 200 {
        return error(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "제목은 1~200자여야 합니다.");
    }

    let post = PostDoc { id: None, title: body.title, body: body.body, tags: body.tags, created_at: DateTime::now() };
    let inserted = match state.posts.insert_one(&post).await {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    // insert_one은 저장된 `_id`만 돌려준다. (RETURNING이 없으므로 응답을 직접 만든다)
    let id = inserted.inserted_id.as_object_id().unwrap();
    let out = PostOut::from(PostDoc { id: Some(id), ..post });

    (StatusCode::CREATED, [(header::LOCATION, format!("/api/posts/{}", out.id))], Json(json!({ "data": out }))).into_response()
}

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
    page: Option<u64>,
    limit: Option<i64>,
}

async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let page = q.page.unwrap_or(1).max(1);
    let limit = q.limit.unwrap_or(10).clamp(1, 50);

    // 검색: 대소문자 무시 부분 일치. 사용자 입력을 정규식에 그대로 넣으면 **정규식 인젝션**이므로 escape한다.
    let filter = match q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            let pattern = regex::escape(s);
            doc! { "$or": [
                { "title": { "$regex": &pattern, "$options": "i" } },
                { "body":  { "$regex": &pattern, "$options": "i" } },
            ] }
        }
        None => doc! {},
    };

    let total = match state.posts.count_documents(filter.clone()).await {
        Ok(n) => n,
        Err(e) => return internal(e),
    };
    let cursor = match state
        .posts
        .find(filter)
        .sort(doc! { "created_at": -1 })
        .skip((page - 1) * limit as u64)
        .limit(limit)
        .await
    {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    let docs: Vec<PostDoc> = match cursor.try_collect().await {
        Ok(v) => v,
        Err(e) => return internal(e),
    };

    let data: Vec<PostOut> = docs.into_iter().map(PostOut::from).collect();
    Json(json!({
        "data": data,
        "meta": { "page": page, "limit": limit, "total": total, "totalPages": total.div_ceil(limit as u64) },
    }))
    .into_response()
}

async fn get_one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let oid = match parse_id(&id) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match state.posts.find_one(doc! { "_id": oid }).await {
        Ok(Some(d)) => Json(json!({ "data": PostOut::from(d) })).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "NOT_FOUND", "글을 찾을 수 없습니다."),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct PatchPost {
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
}

/// 부분 수정. 보낸 필드만 `$set`에 넣는다. (SQL의 `COALESCE($1, col)` 요령이 필요 없다)
async fn update(State(state): State<AppState>, Path(id): Path<String>, Json(patch): Json<PatchPost>) -> Response {
    let oid = match parse_id(&id) {
        Ok(o) => o,
        Err(r) => return r,
    };

    let mut set = doc! {};
    if let Some(t) = patch.title {
        set.insert("title", t);
    }
    if let Some(b) = patch.body {
        set.insert("body", b);
    }
    if let Some(tags) = patch.tags {
        set.insert("tags", tags);
    }
    if set.is_empty() {
        return error(StatusCode::BAD_REQUEST, "BAD_REQUEST", "수정할 필드가 없습니다.");
    }

    match state.posts.update_one(doc! { "_id": oid }, doc! { "$set": set }).await {
        Ok(r) if r.matched_count == 0 => error(StatusCode::NOT_FOUND, "NOT_FOUND", "글을 찾을 수 없습니다."),
        Ok(_) => get_one(State(state), Path(id)).await,
        Err(e) => internal(e),
    }
}

async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let oid = match parse_id(&id) {
        Ok(o) => o,
        Err(r) => return r,
    };
    match state.posts.delete_one(doc! { "_id": oid }).await {
        Ok(r) if r.deleted_count == 0 => error(StatusCode::NOT_FOUND, "NOT_FOUND", "글을 찾을 수 없습니다."),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}
