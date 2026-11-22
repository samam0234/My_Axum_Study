//! study4 — 라우팅과 요청 처리.
//!
//! 실행: `cargo run --example s04_routing`  →  http://127.0.0.1:3004
//!
//! 정적 경로 · 동적 경로 · 와일드카드 · 쿼리 · 리다이렉트 · 404/405 · 요청 로깅 미들웨어.

use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, Query, Request},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    // 프론트(Vite 개발 서버)가 다른 포트에서 부를 수 있게 허용한다.
    // 브라우저는 "출처(origin) = 스킴+호스트+포트"가 다르면 CORS 검사를 한다.
    let cors = CorsLayer::new()
        .allow_origin("http://localhost:5173".parse::<header::HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST]);

    let api = Router::new()
        .route("/users", get(list_users))
        .route("/users/{id}", get(get_user))
        // {*path}는 남은 경로 전체를 잡는다: /api/files/a/b/c.txt → "a/b/c.txt"
        .route("/files/{*path}", get(get_file));

    let app = Router::new()
        .route("/", get(|| async { "라우팅 예제" }))
        // 경로를 옮겼을 때: 301/308은 영구, 302/307은 임시. 검색엔진과 캐시가 기억하는 방식이 다르다.
        .route("/old-users", get(|| async { Redirect::permanent("/api/users") }))
        .nest("/api", api)
        // 매칭되는 경로가 없으면 이 핸들러가 불린다. (404)
        .fallback(not_found)
        // 경로는 있는데 메서드가 다를 때 (405)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(cors)
        // 모든 요청의 "메서드 경로 → 상태 (소요 시간)"을 찍는다.
        .layer(middleware::from_fn(log_requests));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3004").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

#[derive(Deserialize)]
struct ListQuery {
    /// `#[serde(default)]`가 없으면 `page`가 빠진 요청은 400이 된다.
    #[serde(default = "one")]
    page: u32,
    #[serde(default = "ten")]
    limit: u32,
    q: Option<String>,
}
fn one() -> u32 {
    1
}
fn ten() -> u32 {
    10
}

/// GET /api/users?page=2&limit=5&q=mi
async fn list_users(Query(q): Query<ListQuery>) -> Json<serde_json::Value> {
    Json(json!({
        "page": q.page,
        "limit": q.limit.clamp(1, 50), // 상한을 서버가 강제한다
        "q": q.q,
    }))
}

/// GET /api/users/42 — `{id}`가 숫자가 아니면 `Path<u32>` 추출에서 400이 난다.
async fn get_user(Path(id): Path<u32>) -> Result<Json<serde_json::Value>, StatusCode> {
    if id == 42 {
        Ok(Json(json!({ "id": 42, "name": "mina" })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn get_file(Path(path): Path<String>) -> String {
    // ⚠️ 실제로 파일을 읽을 때는 `..`가 들어 있는지 반드시 검사해야 한다(경로 조작 공격).
    format!("요청한 경로: {path}")
}

async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": "NOT_FOUND", "message": "없는 경로입니다." } })),
    )
        .into_response()
}

async fn method_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({ "error": { "code": "METHOD_NOT_ALLOWED", "message": "지원하지 않는 메서드입니다." } })),
    )
        .into_response()
}

/// 요청 → (핸들러) → 응답 흐름 전체를 감싸는 미들웨어.
/// `next.run(request)` 앞은 "요청이 들어올 때", 뒤는 "응답이 나갈 때"다.
async fn log_requests(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let started = Instant::now();

    let response = next.run(request).await;

    println!(
        "{method} {uri} → {} ({:.1}ms)",
        response.status().as_u16(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    response
}
