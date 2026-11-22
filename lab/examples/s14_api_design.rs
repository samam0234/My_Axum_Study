//! study14 — API 설계: 버전 · 페이지네이션 · 오류 규칙 · 캐시 헤더(ETag).
//!
//! 실행: `cargo run --example s14_api_design`  →  http://127.0.0.1:3014
//!
//! - v1: 공통 API 스펙 v1 모양 — `{ data, meta }` + page/limit/sort/q (오프셋 페이지네이션)
//! - v2: 커서 페이지네이션 — 데이터가 자주 바뀌는 목록에서 중복·누락이 없다
//! - v1에는 `Deprecation`/`Sunset` 헤더를 붙여 "곧 사라진다"는 것을 기계가 읽게 알린다
//! - 단건 조회는 `ETag` + `If-None-Match` → 304

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Post {
    id: u32,
    title: String,
    comment_count: u32,
    created_at: String,
}

#[derive(Clone)]
struct AppState {
    posts: Arc<Vec<Post>>,
}

#[tokio::main]
async fn main() {
    // 예제용 가짜 데이터 37건 (id가 클수록 최신)
    let posts: Vec<Post> = (1..=37)
        .map(|i| Post {
            id: i,
            title: format!("글 {i}"),
            comment_count: (i * 7) % 5,
            created_at: format!("2026-11-{:02}T09:00:00Z", (i % 28) + 1),
        })
        .collect();
    let state = AppState { posts: Arc::new(posts) };

    // 버전은 경로에 넣는다(/api/v1). 브라우저·curl·로그에서 눈에 보이고 캐시 키도 자연스럽게 갈린다.
    let v1 = Router::new()
        .route("/posts", get(list_v1))
        .route("/posts/{id}", get(get_one))
        .layer(middleware::from_fn(deprecation_headers));
    let v2 = Router::new().route("/posts", get(list_v2));

    let app = Router::new()
        .nest("/api/v1", v1)
        .nest("/api/v2", v2)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3014").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ── 오류 규칙 ────────────────────────────────────────────────────────────────

fn error(status: StatusCode, code: &str, message: &str, fields: Option<Value>) -> Response {
    let mut body = json!({ "error": { "code": code, "message": message } });
    if let Some(f) = fields {
        body["error"]["fields"] = f;
    }
    (status, Json(body)).into_response()
}

// ── v1: 오프셋 페이지네이션 ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListV1 {
    page: Option<u32>,
    limit: Option<u32>,
    q: Option<String>,
    sort: Option<String>,
}

async fn list_v1(State(state): State<AppState>, Query(q): Query<ListV1>) -> Response {
    // 스펙: page 기본 1(1 이상) / limit 기본 10, 50 초과는 50으로 / sort 기본 -createdAt
    let page = q.page.unwrap_or(1);
    if page == 0 {
        return error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "입력값을 확인하세요.",
            Some(json!({ "page": ["1 이상이어야 합니다."] })),
        );
    }
    let limit = q.limit.unwrap_or(10).clamp(1, 50);

    // sort는 **허용 목록**으로만 받는다. 아무 문자열이나 ORDER BY에 넣으면 인젝션이 된다.
    let sort = q.sort.as_deref().unwrap_or("-createdAt");
    if !["createdAt", "-createdAt", "-commentCount"].contains(&sort) {
        return error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "입력값을 확인하세요.",
            Some(json!({ "sort": ["createdAt, -createdAt, -commentCount 중 하나여야 합니다."] })),
        );
    }

    let mut rows: Vec<&Post> = state
        .posts
        .iter()
        .filter(|p| {
            q.q.as_deref()
                .is_none_or(|s| p.title.to_lowercase().contains(&s.to_lowercase()))
        })
        .collect();
    match sort {
        "createdAt" => rows.sort_by_key(|p| p.id),
        "-commentCount" => {
            rows.sort_by(|a, b| b.comment_count.cmp(&a.comment_count).then(b.id.cmp(&a.id)))
        }
        _ => rows.sort_by_key(|p| std::cmp::Reverse(p.id)),
    }

    let total = rows.len() as u32;
    let total_pages = total.div_ceil(limit);
    let data: Vec<&Post> = rows
        .into_iter()
        .skip(((page - 1) * limit) as usize)
        .take(limit as usize)
        .collect();

    Json(json!({
        "data": data,
        "meta": { "page": page, "limit": limit, "total": total, "totalPages": total_pages, "q": q.q },
    }))
    .into_response()
}

// ── v2: 커서 페이지네이션 ─────────────────────────────────────────────────────
//
// 오프셋의 약점: 1페이지를 보는 사이 새 글이 올라오면 2페이지 첫 줄이 1페이지 마지막 줄과 겹친다.
// 커서는 "마지막으로 본 id보다 오래된 것부터"라서 그런 일이 없고, 깊은 페이지도 느려지지 않는다.

#[derive(Deserialize)]
struct ListV2 {
    cursor: Option<u32>,
    limit: Option<u32>,
}

async fn list_v2(State(state): State<AppState>, Query(q): Query<ListV2>) -> Json<Value> {
    let limit = q.limit.unwrap_or(10).clamp(1, 50) as usize;
    let mut rows: Vec<&Post> = state
        .posts
        .iter()
        .filter(|p| q.cursor.is_none_or(|c| p.id < c))
        .collect();
    rows.sort_by_key(|p| std::cmp::Reverse(p.id));

    // 한 건 더 읽어서 "다음이 있는가"를 알아낸다. COUNT(*)를 따로 세지 않아도 된다.
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    // 커서는 "이번 페이지에서 마지막으로 준 id"다. 다음 요청은 id < cursor 인 것부터 읽는다.
    let next_cursor = if has_more { rows.last().map(|p| p.id) } else { None };

    Json(json!({ "data": rows, "meta": { "limit": limit, "nextCursor": next_cursor, "hasMore": has_more } }))
}

// ── 단건 + ETag ───────────────────────────────────────────────────────────────

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    headers: HeaderMap,
) -> Response {
    let Some(post) = state.posts.iter().find(|p| p.id == id) else {
        return error(StatusCode::NOT_FOUND, "NOT_FOUND", "글을 찾을 수 없습니다.", None);
    };

    let body = json!({ "data": post });
    // 내용이 같으면 같은 값이 나오는 지문. (실무에서는 updatedAt이나 버전 컬럼을 쓰는 편이 싸다)
    let mut hasher = DefaultHasher::new();
    body.to_string().hash(&mut hasher);
    let etag = format!("\"{:x}\"", hasher.finish());

    // 클라이언트가 가진 것과 같으면 본문 없이 304. 대역폭과 파싱 비용을 아낀다.
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    ([(header::ETAG, etag)], Json(body)).into_response()
}

// ── 버전 종료 예고 헤더 ───────────────────────────────────────────────────────

async fn deprecation_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert("deprecation", HeaderValue::from_static("true"));
    h.insert("sunset", HeaderValue::from_static("Wed, 31 Dec 2026 23:59:59 GMT"));
    h.insert(
        header::LINK,
        HeaderValue::from_static("</api/v2/posts>; rel=\"successor-version\""),
    );
    response
}
