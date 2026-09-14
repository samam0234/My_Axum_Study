//! tower 미들웨어 계층 조립.
//!
//! axum 자체에는 미들웨어 기능이 거의 없다. 전부 `tower` / `tower-http` 것을 가져다 쓴다.
//! `Layer`는 `Service`를 감싸 새 `Service`를 만드는 데코레이터라서 무한히 겹쳐 쌓을 수 있다.

use std::time::{Duration, Instant};

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tower_http::request_id::MakeRequestUuid;

/// 요청 처리에 걸린 시간을 응답 헤더에 붙이는 커스텀 미들웨어.
///
/// `axum::middleware::from_fn`에 넘기면 Layer가 된다.
/// tower의 `Service`를 직접 구현하는 것보다 훨씬 간단하다.
pub async fn response_time(request: Request, next: Next) -> Response {
    let started = Instant::now();

    // next.run(request)가 "아래 계층 전체 + 핸들러"를 실행한다.
    // Express의 next(), Nest 인터셉터의 handle()에 해당.
    let mut response = next.run(request).await;

    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    if let Ok(value) = HeaderValue::from_str(&format!("{elapsed_ms:.2}")) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-response-time-ms"), value);
    }

    response
}

/// 핸들러가 패닉했을 때 연결을 끊는 대신 500 JSON을 돌려준다.
///
/// study6에서 jsonwebtoken이 패닉했을 때 클라이언트는
/// "connection was closed unexpectedly"만 봤다. 이 계층이 그걸 막아준다.
pub fn on_panic(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    // 패닉 메시지를 꺼낸다 (panic!("...") 또는 panic!(String) 형태)
    let detail = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic".to_string()
    };

    tracing::error!(panic = %detail, "핸들러가 패닉했습니다");

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(json!({
            "error": "internal_error",
            "message": "서버 내부 오류가 발생했습니다.",
        })),
    )
        .into_response()
}

/// 요청 ID 헤더 이름.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// 요청마다 UUID를 만들어 붙이는 생성기.
pub fn request_id_maker() -> MakeRequestUuid {
    MakeRequestUuid
}

/// 느린 응답을 잘라내는 기준 시간.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// 요청 본문 최대 크기 (2MB). 기본값은 2MB지만 명시적으로 적어 둔다.
pub const BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;
