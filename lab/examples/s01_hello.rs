//! study1 — Hello World와 기본 문법.
//!
//! 실행: `cargo run --example s01_hello`  →  http://127.0.0.1:3001
//!
//! 이 파일 하나에 "핸들러는 평범한 async 함수"라는 사실이 전부 들어 있다.

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .route("/hello/{name}", get(hello))
        .route("/greet", get(greet))
        .route("/page/{name}", get(page))
        .route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// 가장 단순한 핸들러. `&'static str`도 `IntoResponse`라서 그대로 돌려줄 수 있다.
async fn root() -> &'static str {
    "Hello, Axum!"
}

/// 경로 파라미터. axum 0.8부터 `:name`이 아니라 `{name}` 문법이다.
async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

#[derive(Deserialize)]
struct GreetQuery {
    /// 필수. 없으면 400.
    name: String,
    /// `Option`이면 선택 파라미터.
    times: Option<u8>,
}

#[derive(Serialize)]
struct Greeting {
    message: String,
    times: u8,
}

/// 쿼리스트링 + JSON 응답: GET /greet?name=mina&times=3
async fn greet(Query(q): Query<GreetQuery>) -> Json<Greeting> {
    let times = q.times.unwrap_or(1).min(10);
    Json(Greeting {
        message: format!("안녕하세요, {}님!", q.name),
        times,
    })
}

/// "템플릿 표현" — 가장 원시적인 형태는 `format!`로 HTML 문자열을 만드는 것이다.
///
/// ⚠️ 사용자 입력을 HTML에 넣을 때는 **반드시 이스케이프**한다. 안 하면 XSS다.
/// 실무에서는 askama·maud 같은 템플릿 엔진이 기본으로 이스케이프해 준다.
async fn page(Path(name): Path<String>) -> Html<String> {
    let safe = escape_html(&name);
    Html(format!(
        r#"<!doctype html>
<html lang="ko">
  <head><meta charset="utf-8"><title>Hello {safe}</title></head>
  <body>
    <h1>안녕하세요, {safe}님</h1>
    <p>지금 보는 것은 서버가 <code>format!</code>으로 만든 HTML입니다.</p>
  </body>
</html>"#
    ))
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `(StatusCode, Json<T>)` 튜플로 상태 코드와 본문을 함께 지정한다.
async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}
