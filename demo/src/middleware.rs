//! tower 미들웨어 계층 조립.
//!
//! axum 자체에는 미들웨어 기능이 거의 없다. 전부 `tower` / `tower-http` 것을 가져다 쓴다.
//! `Layer`는 `Service`를 감싸 새 `Service`를 만드는 데코레이터라서 무한히 겹쳐 쌓을 수 있다.

use std::time::{Duration, Instant};

use axum::{
    body::{Body, to_bytes},
    extract::Request,
    http::{HeaderName, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tower_http::request_id::MakeRequestUuid;

use crate::error::ErrorEnvelope;

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
        axum::Json(ErrorEnvelope::new("INTERNAL", "서버 내부 오류가 발생했습니다.")),
    )
        .into_response()
}

/// 오류 본문을 **공통 API 스펙 v1 형태로 마무리**하는 미들웨어.
///
/// 하는 일이 두 가지다.
/// 1. `AppError`가 만든 `{ error: { code, message } }`에 **`requestId`를 채운다.**
///    핸들러는 요청 헤더를 모르기 때문에 응답이 나가는 길목에서 붙이는 게 가장 간단하다.
/// 2. `AppError`를 거치지 않은 4xx·5xx(타임아웃의 빈 본문, `ServeDir`의 빈 404 등)도
///    같은 모양으로 감싼다.
///
/// ⚠️ 배치 순서: `CatchPanicLayer`·`TimeoutLayer`보다 **바깥**(그래야 패닉·타임아웃 응답도 본다),
/// `CompressionLayer`보다 **안쪽**(압축된 바이트는 JSON으로 읽을 수 없다).
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

    // 본문이 바뀌었으므로 길이/타입 헤더를 다시 맞춘다. (Content-Length는 hyper가 다시 계산)
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );

    Response::from_parts(parts, Body::from(value.to_string()))
}

/// 오류 본문을 읽을 때 메모리에 올리는 최대 크기. 오류 JSON은 작다.
const ERROR_BODY_LIMIT: usize = 64 * 1024;

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
        StatusCode::REQUEST_TIMEOUT => "요청 처리 시간이 초과되었습니다.",
        StatusCode::NOT_FOUND => "찾을 수 없습니다.",
        s if s.is_server_error() => "서버 내부 오류가 발생했습니다.",
        _ => "요청을 처리할 수 없습니다.",
    }
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
