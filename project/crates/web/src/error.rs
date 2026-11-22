//! ⭐ **번역 계층.**
//!
//! `DomainError`(도메인이 말하는 실패)를 HTTP 응답으로 옮긴다.
//! 이 매핑이 web 크레이트에만 있기 때문에, 도메인은 HTTP를 몰라도 된다.
//!
//! 오류 본문은 **공통 API 스펙 v1 §5**를 따른다.
//!
//! ```json
//! { "error": { "code": "VALIDATION_ERROR", "message": "...", "fields": { "title": ["..."] }, "requestId": "..." } }
//! ```
//!
//! `requestId`는 여기서 채우지 않는다. 핸들러는 요청 헤더를 모르므로
//! `middleware::error_envelope`가 응답이 나가는 길목에서 채운다.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use domain::DomainError;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ApiError(pub DomainError);

/// `?`로 DomainError를 그대로 던질 수 있게 한다.
impl<E: Into<DomainError>> From<E> for ApiError {
    fn from(value: E) -> Self {
        Self(value.into())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ErrorEnvelope {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code,
                message: message.into(),
                fields: None,
                request_id: None,
            },
        }
    }
}

/// 상태 코드 + 코드 + 메시지로 오류 응답을 만든다.
pub fn respond(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (status, Json(ErrorEnvelope::new(code, message))).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // 도메인 실패 → (HTTP 상태 코드, 스펙의 code). **이 표가 번역 규칙의 전부다.**
        let (status, code) = match &self.0 {
            DomainError::NotFound { .. } => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            DomainError::EmailTaken => (StatusCode::CONFLICT, "EMAIL_TAKEN"),
            DomainError::Conflict(_) => (StatusCode::CONFLICT, "CONFLICT"),
            DomainError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            DomainError::Forbidden(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            DomainError::Invalid(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            DomainError::Database(_) | DomainError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL")
            }
        };

        let message = match &self.0 {
            // 내부 사정은 밖으로 흘리지 않는다. 원인은 로그에만.
            DomainError::Database(e) => {
                tracing::error!(error = %e, "database error");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            DomainError::Internal(e) => {
                tracing::error!(error = ?e, "internal error");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            other => other.to_string(),
        };

        respond(status, code, message)
    }
}

/// web 계층에서만 발생하는 오류(토큰 없음, 본문 파싱 실패 등)를 만드는 헬퍼들.
pub fn unauthorized(message: &str) -> Response {
    respond(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
}

pub fn bad_request(message: String) -> Response {
    respond(StatusCode::BAD_REQUEST, "BAD_REQUEST", message)
}

pub fn validation_failed(errors: &validator::ValidationErrors) -> Response {
    let fields: HashMap<String, Vec<String>> = errors
        .field_errors()
        .into_iter()
        .map(|(field, errs)| {
            let messages = errs
                .iter()
                .map(|e| {
                    e.message
                        .as_ref()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| e.code.to_string())
                })
                .collect();
            (field.to_string(), messages)
        })
        .collect();

    let mut envelope = ErrorEnvelope::new("VALIDATION_ERROR", "입력값을 확인하세요.");
    envelope.error.fields = Some(fields);
    (StatusCode::BAD_REQUEST, Json(envelope)).into_response()
}

/// `/api` 아래에서 매칭되는 라우트가 없을 때 (404)
pub async fn not_found() -> Response {
    respond(StatusCode::NOT_FOUND, "NOT_FOUND", "없는 경로입니다.")
}

/// 경로는 있는데 메서드가 다를 때 (405)
pub async fn method_not_allowed() -> Response {
    respond(
        StatusCode::METHOD_NOT_ALLOWED,
        "METHOD_NOT_ALLOWED",
        "이 경로에서 지원하지 않는 HTTP 메서드입니다.",
    )
}
