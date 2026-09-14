//! ⭐ **번역 계층.**
//!
//! `DomainError`(도메인이 말하는 실패)를 HTTP 응답으로 옮긴다.
//! 이 매핑이 web 크레이트에만 있기 때문에, 도메인은 HTTP를 몰라도 된다.
//!
//! demo/에서는 `AppError` 하나가 두 역할을 다 했다. 그래서 서비스 코드가 HTTP에 묶였다.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use domain::DomainError;
use serde::Serialize;
use serde_json::json;
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
struct ErrorBody {
    error: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<HashMap<String, Vec<String>>>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // 도메인 실패 → HTTP 상태 코드. **이 표가 번역 규칙의 전부다.**
        let (status, code) = match &self.0 {
            DomainError::NotFound { .. } => (StatusCode::NOT_FOUND, "not_found"),
            DomainError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            DomainError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "unauthorized"),
            DomainError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            DomainError::Invalid(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed"),
            DomainError::Database(_) | DomainError::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
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

        (
            status,
            Json(ErrorBody {
                error: code,
                message,
                details: None,
            }),
        )
            .into_response()
    }
}

/// web 계층에서만 발생하는 오류(토큰 없음, 본문 파싱 실패 등)를 만드는 헬퍼들.
pub fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized", "message": message })),
    )
        .into_response()
}

pub fn bad_request(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "bad_request", "message": message })),
    )
        .into_response()
}

pub fn validation_failed(errors: &validator::ValidationErrors) -> Response {
    let details: HashMap<String, Vec<String>> = errors
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

    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ErrorBody {
            error: "validation_failed",
            message: "요청 값이 규칙에 맞지 않습니다.".to_string(),
            details: Some(details),
        }),
    )
        .into_response()
}

/// 매칭되는 라우트가 없을 때
pub async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not_found", "message": "없는 경로입니다." })),
    )
        .into_response()
}
