//! 요청 ID, 오류 본문 마무리, 패닉 처리.
//!
//! demo/의 `middleware.rs`와 같은 역할이다. (study8)

use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{HeaderName, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::{ErrorEnvelope, respond};

/// 요청 ID 헤더 이름. 요청에 있으면 그대로, 없으면 `SetRequestIdLayer`가 UUID를 만든다.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// 오류 본문을 읽을 때 메모리에 올리는 최대 크기. 오류 JSON은 작다.
const ERROR_BODY_LIMIT: usize = 64 * 1024;

/// 오류 본문을 **공통 API 스펙 v1 형태로 마무리**하는 미들웨어. `/api` 라우터에만 건다.
///
/// 1. `{ error: { code, message } }`에 **`requestId`를 채운다.** (응답 헤더 `X-Request-Id`와 같은 값)
/// 2. 우리 코드를 거치지 않고 나간 4xx·5xx(경로 id가 숫자가 아닐 때 axum이 만드는 평문 400 등)도
///    같은 모양으로 감싼다.
///
/// ⚠️ `CatchPanicLayer`보다 **바깥**(그래야 패닉 응답도 본다), `CompressionLayer`보다 **안쪽**
/// (압축된 바이트는 JSON으로 읽을 수 없다).
pub async fn error_envelope(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let response = next.run(request).await;
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let bytes = to_bytes(body, ERROR_BODY_LIMIT).await.unwrap_or_default();

    // 이미 `{ "error": { ... } }`이면 requestId만 채우고, 아니면 새로 감싼다.
    let mut value = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .filter(|v| v.get("error").is_some_and(|e| e.is_object()))
        .unwrap_or_else(|| {
            let text = String::from_utf8_lossy(&bytes);
            let message = if text.trim().is_empty() {
                default_message(status).to_string()
            } else {
                text.trim().chars().take(200).collect()
            };
            serde_json::to_value(ErrorEnvelope::new(code_for_status(status), message))
                .unwrap_or_default()
        });

    if let (Some(id), Some(error)) = (request_id, value.get_mut("error")) {
        error["requestId"] = serde_json::Value::String(id);
    }

    // 본문이 바뀌었으므로 길이/타입 헤더를 다시 맞춘다.
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    Response::from_parts(parts, Body::from(value.to_string()))
}

fn code_for_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "UNAUTHORIZED",
        StatusCode::FORBIDDEN => "FORBIDDEN",
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::METHOD_NOT_ALLOWED => "METHOD_NOT_ALLOWED",
        StatusCode::REQUEST_TIMEOUT => "REQUEST_TIMEOUT",
        StatusCode::PAYLOAD_TOO_LARGE => "PAYLOAD_TOO_LARGE",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "UNSUPPORTED_MEDIA_TYPE",
        StatusCode::TOO_MANY_REQUESTS => "RATE_LIMITED",
        s if s.is_client_error() => "BAD_REQUEST",
        _ => "INTERNAL",
    }
}

fn default_message(status: StatusCode) -> &'static str {
    match status {
        StatusCode::NOT_FOUND => "찾을 수 없습니다.",
        s if s.is_server_error() => "서버 내부 오류가 발생했습니다.",
        _ => "요청을 처리할 수 없습니다.",
    }
}

/// 핸들러가 패닉했을 때 연결을 끊는 대신 500 JSON을 돌려준다. (study7)
pub fn on_panic(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic".to_string()
    };

    tracing::error!(panic = %detail, "핸들러가 패닉했습니다");

    respond(
        StatusCode::INTERNAL_SERVER_ERROR,
        "INTERNAL",
        "서버 내부 오류가 발생했습니다.",
    )
    .into_response()
}
