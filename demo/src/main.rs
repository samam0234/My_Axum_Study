mod error;
mod extract;
mod posts;
mod state;

use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde::{Deserialize, Serialize};

use crate::error::{AppResult, handle_404, handle_405};
use crate::extract::{Path, Query};
use crate::state::AppState;

#[tokio::main]
async fn main() {
    let state = AppState::new();
    let app = app(state);

    // 0.7부터 hyper Server가 아니라 tokio TcpListener + axum::serve 조합을 쓴다.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("3000 포트를 열 수 없습니다");

    println!("listening on http://{}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

/// 라우터 조립만 따로 함수로 빼두면 나중에 통합 테스트에서 그대로 재사용할 수 있다. (study9)
fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/hello/{name}", get(hello))
        .route("/echo", get(echo))
        .route("/boom", get(boom))
        // 게시판 라우터를 /posts 아래에 장착한다.
        .nest("/posts", posts::router())
        // 매칭되는 경로가 없을 때 (404)
        .fallback(handle_404)
        // 경로는 있는데 메서드가 다를 때 (405)
        .method_not_allowed_fallback(handle_405)
        // 여기서 상태를 주입하면 Router<AppState> → Router<()> 가 된다.
        .with_state(state)
}

/// 가장 단순한 핸들러: &'static str도 IntoResponse를 구현하므로 그대로 반환할 수 있다.
/// → 200 OK, Content-Type: text/plain; charset=utf-8
async fn root() -> &'static str {
    "Hello, Axum!"
}

/// (StatusCode, Json<T>) 튜플로 상태 코드와 본문을 함께 지정한다.
async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

/// 경로 파라미터: axum 0.8부터 `:name`이 아니라 `{name}` 문법을 쓴다.
async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

#[derive(Debug, Deserialize)]
struct EchoParams {
    /// 필수 파라미터. 없으면 400.
    message: String,
    /// Option<T>로 선언하면 선택 파라미터가 된다.
    times: Option<u8>,
}

#[derive(Debug, Serialize)]
struct EchoResponse {
    original: String,
    repeated: Vec<String>,
}

/// 쿼리스트링: GET /echo?message=hi&times=3
async fn echo(Query(params): Query<EchoParams>) -> Json<EchoResponse> {
    let times = params.times.unwrap_or(1);
    let repeated = (0..times).map(|_| params.message.clone()).collect();

    Json(EchoResponse {
        original: params.message,
        repeated,
    })
}

/// 500 응답 확인용 엔드포인트.
/// `anyhow::Error`가 `?`로 `AppError::Internal`로 자동 변환되는 것을 보여준다.
async fn boom() -> AppResult<&'static str> {
    let raw = "not-a-number";
    // ParseIntError → anyhow::Error → AppError::Internal
    let _n: i32 = raw
        .parse()
        .map_err(|e: std::num::ParseIntError| anyhow::anyhow!("설정 값 파싱 실패: {e}"))?;
    Ok("unreachable")
}
