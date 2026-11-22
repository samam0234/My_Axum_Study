//! study12 — 프론트(UI·상태관리·폼 라이브러리) 연동의 서버 쪽 준비.
//!
//! 실행: `cargo run --example s12_front`  →  http://127.0.0.1:3012
//!
//! 프론트와 맞물리는 지점은 세 곳이다.
//!   1. 정적 파일(빌드된 SPA) 서빙 + History 라우팅 fallback
//!   2. CORS — 개발 서버(Vite :5173)가 다른 출처에서 부를 때
//!   3. 쿠키 인증 — `credentials: "include"` + `SameSite` + 정확한 Origin
//!
//! 환경변수: CORS_ORIGIN (기본 http://localhost:5173)

use axum::{
    Json, Router,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::json;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};

#[tokio::main]
async fn main() {
    let origin = std::env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());

    let cors = CorsLayer::new()
        // ⚠️ 쿠키를 주고받는 CORS에서는 `Any`(*)를 쓸 수 없다. 출처를 정확히 적어야 한다.
        .allow_origin(origin.parse::<HeaderValue>().expect("CORS_ORIGIN 형식 오류"))
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    let api = Router::new()
        .route("/hello", get(|| async { Json(json!({ "message": "안녕하세요" })) }))
        .route("/login", post(login))
        .route("/me", get(me))
        .route("/logout", post(logout))
        // `/api` 아래의 없는 경로는 SPA(index.html)가 아니라 **JSON 404**여야 한다.
        // 이게 없으면 바깥의 SPA fallback이 `/api/nope`까지 삼켜서 200 + HTML이 나간다.
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "NOT_FOUND", "message": "없는 API 경로입니다." } })),
            )
        })
        .layer(cors);

    // SPA: 파일이 없으면 index.html을 200으로 돌려준다.
    // ⚠️ `not_found_service`는 상태를 404로 강제한다. SPA 진입점은 200이어야 하므로 `fallback`이다.
    let spa = ServeDir::new("static").fallback(ServeFile::new("static/index.html"));

    let app = Router::new().nest("/api", api).fallback_service(spa);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3012").await.unwrap();
    println!("http://{}  (CORS 허용 출처: {origin})", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// 로그인: 세션 쿠키를 발급한다.
/// - `HttpOnly`: JS(`document.cookie`)로 못 읽는다 → XSS로 토큰을 훔치기 어렵다
/// - `SameSite=Lax`: 다른 사이트가 만든 POST 요청에는 쿠키가 안 실린다 → CSRF 완화
/// - `Secure`: HTTPS에서만 전송 (운영에서는 반드시 붙인다. 로컬 HTTP에서는 빼야 동작한다)
async fn login() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::SET_COOKIE, "sid=demo-session-1; HttpOnly; SameSite=Lax; Path=/; Max-Age=3600")],
        Json(json!({ "data": { "nickname": "mina" } })),
    )
}

async fn me(headers: HeaderMap) -> impl IntoResponse {
    if cookie_value(&headers, "sid").as_deref() == Some("demo-session-1") {
        (StatusCode::OK, Json(json!({ "data": { "nickname": "mina" } })))
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "code": "UNAUTHORIZED", "message": "로그인이 필요합니다." } })),
        )
    }
}

/// 로그아웃: 같은 이름의 쿠키를 Max-Age=0으로 덮어써서 지운다.
async fn logout() -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, "sid=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")],
    )
}

/// `Cookie: a=1; sid=xyz` 에서 값을 꺼낸다. (실무에서는 axum-extra의 CookieJar를 쓴다)
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}
