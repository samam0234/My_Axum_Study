//! 게시판/인증과 무관한 데모용 라우트 모음.
//!
//! study1~7에서 만든 실습·확인용 엔드포인트들을 `main.rs`에서 분리했다.

use std::time::Duration;

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::extract::{Path, Query};

/// 가장 단순한 핸들러: &'static str도 IntoResponse를 구현하므로 그대로 반환할 수 있다.
pub async fn root() -> &'static str {
    "Hello, Axum!"
}

/// (StatusCode, Json<T>) 튜플로 상태 코드와 본문을 함께 지정한다.
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

/// 경로 파라미터: axum 0.8부터 `:name`이 아니라 `{name}` 문법을 쓴다.
pub async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

#[derive(Debug, Deserialize)]
pub struct EchoParams {
    /// 필수 파라미터. 없으면 400.
    message: String,
    /// Option<T>로 선언하면 선택 파라미터가 된다.
    times: Option<u8>,
}

#[derive(Debug, Serialize)]
pub struct EchoResponse {
    original: String,
    repeated: Vec<String>,
}

/// 쿼리스트링: GET /echo?message=hi&times=3
pub async fn echo(Query(params): Query<EchoParams>) -> Json<EchoResponse> {
    let times = params.times.unwrap_or(1);
    let repeated = (0..times).map(|_| params.message.clone()).collect();

    Json(EchoResponse {
        original: params.message,
        repeated,
    })
}

/// 500 응답 확인용. `anyhow::Error`가 `?`로 `AppError::Internal`이 되는 것을 보여준다.
pub async fn boom() -> AppResult<&'static str> {
    let raw = "not-a-number";
    let _n: i32 = raw
        .parse()
        .map_err(|e: std::num::ParseIntError| anyhow::anyhow!("설정 값 파싱 실패: {e}"))?;
    Ok("unreachable")
}

/// CatchPanicLayer 확인용. 패닉해도 연결이 끊기지 않고 500 JSON이 나가야 한다.
pub async fn panic_handler() -> &'static str {
    panic!("의도적으로 터뜨린 패닉");
}

/// TimeoutLayer 확인용. REQUEST_TIMEOUT보다 오래 걸린다.
pub async fn slow() -> &'static str {
    tokio::time::sleep(Duration::from_secs(10)).await;
    "이 응답은 도달하지 못한다"
}
