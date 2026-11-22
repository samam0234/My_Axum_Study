use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;

/// 앱 전체에서 쓰는 단 하나의 오류 타입.
///
/// Spring의 `@RestControllerAdvice`, Nest의 전역 `ExceptionFilter`에 해당하는 자리.
/// 차이는 "전역에 등록해서 가로챈다"가 아니라 **핸들러 반환 타입에 적혀 있다**는 점이다.
#[derive(Debug, Error)]
pub enum AppError {
    /// 리소스를 찾지 못함 → 404 `NOT_FOUND`
    #[error("{resource} {id} not found")]
    NotFound { resource: &'static str, id: String },

    /// 요청 자체가 잘못됨(경로/쿼리 파싱 실패 등) → 400 `BAD_REQUEST`
    #[error("{0}")]
    BadRequest(String),

    /// 본문 파싱 실패 → axum이 판단한 상태 코드를 **스펙에 맞게 정규화**한다.
    /// 400/422 → 400 `BAD_REQUEST`, 413 → `PAYLOAD_TOO_LARGE`, 415 → `UNSUPPORTED_MEDIA_TYPE`
    #[error("{message}")]
    InvalidBody { status: StatusCode, message: String },

    /// 값 검증 실패 → 400 `VALIDATION_ERROR` (+ `fields`)
    #[error("validation failed")]
    Validation(validator::ValidationErrors),

    /// 토큰 없음·만료·위조 → 401 `UNAUTHORIZED`
    #[error("{0}")]
    Unauthorized(String),

    /// 로그인 실패 → 401 `INVALID_CREDENTIALS` (아이디/비밀번호 어느 쪽이 틀렸는지 숨긴다)
    #[error("아이디 또는 비밀번호가 올바르지 않습니다")]
    InvalidCredentials,

    /// 권한 없음 → 403 `FORBIDDEN` (소유권 검사)
    #[error("{0}")]
    Forbidden(String),

    /// 아이디·이메일 중복 → 409 `EMAIL_TAKEN`
    #[error("이미 사용 중인 아이디 또는 이메일입니다")]
    EmailTaken,

    /// 그 밖의 상태 충돌 → 409 `CONFLICT`
    #[error("{0}")]
    Conflict(String),

    /// 경로는 있는데 메서드가 다름 → 405 `METHOD_NOT_ALLOWED`
    #[error("이 경로에서 지원하지 않는 HTTP 메서드입니다")]
    MethodNotAllowed,

    /// DB 오류 → 500 `INTERNAL`. `#[from]` 덕분에 `?` 하나로 sqlx::Error가 여기로 들어온다.
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    /// 예상하지 못한 내부 오류 → 500 `INTERNAL`. 원인은 로그에만 남기고 클라이언트에는 숨긴다.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// 모든 4xx·5xx 응답이 갖는 공통 형태. **공통 API 스펙 v1 §5**
///
/// ```json
/// { "error": { "code": "VALIDATION_ERROR", "message": "...", "fields": { "title": ["..."] }, "requestId": "..." } }
/// ```
#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    /// 기계가 읽는 코드 (`NOT_FOUND`, `VALIDATION_ERROR` ...)
    pub code: &'static str,
    /// 사람이 읽는 메시지
    pub message: String,
    /// 필드별 상세 (검증 실패일 때만)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<HashMap<String, Vec<String>>>,
    /// `X-Request-Id`와 같은 값. 핸들러는 요청 헤더를 모르므로
    /// `middleware::error_envelope`가 응답을 내보내기 직전에 채운다.
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

impl AppError {
    /// 편의 생성자 — `AppError::not_found("post", id)`
    pub fn not_found(resource: &'static str, id: impl ToString) -> Self {
        Self::NotFound {
            resource,
            id: id.to_string(),
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::BadRequest(_) | Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::InvalidBody { status, .. } => match *status {
                // axum은 "JSON 문법은 맞는데 타입이 틀림"을 422로 준다. 스펙은 400이다.
                StatusCode::UNPROCESSABLE_ENTITY => StatusCode::BAD_REQUEST,
                other => other,
            },
            Self::Unauthorized(_) | Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::EmailTaken | Self::Conflict(_) => StatusCode::CONFLICT,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "NOT_FOUND",
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::InvalidBody { status, .. } => match *status {
                StatusCode::PAYLOAD_TOO_LARGE => "PAYLOAD_TOO_LARGE",
                StatusCode::UNSUPPORTED_MEDIA_TYPE => "UNSUPPORTED_MEDIA_TYPE",
                _ => "BAD_REQUEST",
            },
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::InvalidCredentials => "INVALID_CREDENTIALS",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::EmailTaken => "EMAIL_TAKEN",
            Self::Conflict(_) => "CONFLICT",
            Self::MethodNotAllowed => "METHOD_NOT_ALLOWED",
            Self::Database(_) | Self::Internal(_) => "INTERNAL",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();

        let message = match &self {
            // 500은 내부 사정을 밖으로 흘리지 않는다. 원인은 서버 로그에만.
            Self::Database(err) => {
                tracing::error!(error = ?err, "database error");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            Self::Internal(err) => {
                tracing::error!(error = ?err, "internal error");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            Self::Validation(_) => "입력값을 확인하세요.".to_string(),
            other => other.to_string(),
        };

        let mut envelope = ErrorEnvelope::new(self.code(), message);
        if let Self::Validation(errors) = &self {
            envelope.error.fields = Some(flatten_validation_errors(errors));
        }

        (status, Json(envelope)).into_response()
    }
}

/// `ValidationErrors` → `{ 필드명: [메시지, ...] }`
fn flatten_validation_errors(
    errors: &validator::ValidationErrors,
) -> HashMap<String, Vec<String>> {
    errors
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
        .collect()
}

/// 핸들러 반환 타입을 짧게 쓰기 위한 별칭.
/// `Result<Json<Post>, AppError>` → `AppResult<Json<Post>>`
pub type AppResult<T> = Result<T, AppError>;

/// 라우터에 매칭되는 경로가 없을 때 (404)
pub async fn handle_404() -> AppError {
    AppError::NotFound {
        resource: "route",
        id: "(unknown)".to_string(),
    }
}

/// 경로는 있는데 메서드가 다를 때 (405)
pub async fn handle_405() -> AppError {
    AppError::MethodNotAllowed
}
