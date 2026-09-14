use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use thiserror::Error;

/// 앱 전체에서 쓰는 단 하나의 오류 타입.
///
/// Spring의 `@RestControllerAdvice`, Nest의 전역 `ExceptionFilter`에 해당하는 자리.
/// 차이는 "전역에 등록해서 가로챈다"가 아니라 **핸들러 반환 타입에 적혀 있다**는 점이다.
#[derive(Debug, Error)]
pub enum AppError {
    /// 리소스를 찾지 못함 → 404
    #[error("{resource} {id} not found")]
    NotFound { resource: &'static str, id: String },

    /// 요청 자체가 잘못됨(경로/쿼리 파싱 실패 등) → 400
    #[error("{0}")]
    BadRequest(String),

    /// 본문 파싱 실패 → axum이 판단한 상태 코드를 그대로 사용 (400 / 415 / 422)
    #[error("{message}")]
    InvalidBody { status: StatusCode, message: String },

    /// 값 검증 실패 → 422
    #[error("validation failed")]
    Validation(validator::ValidationErrors),

    /// 인증 실패 → 401 (study6에서 사용)
    #[error("{0}")]
    #[allow(dead_code)] // 아직 안 쓰지만 곧 쓴다
    Unauthorized(String),

    /// 권한 없음 → 403 (study7에서 사용)
    #[error("{0}")]
    #[allow(dead_code)] // 아직 안 쓰지만 곧 쓴다
    Forbidden(String),

    /// 중복 등 상태 충돌 → 409 (study6에서 사용)
    #[error("{0}")]
    #[allow(dead_code)] // 아직 안 쓰지만 곧 쓴다
    Conflict(String),

    /// DB 오류 → 500. `#[from]` 덕분에 `?` 하나로 sqlx::Error가 여기로 들어온다.
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    /// 예상하지 못한 내부 오류 → 500. 원인은 로그에만 남기고 클라이언트에는 숨긴다.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// 모든 오류 응답이 갖는 공통 형태.
#[derive(Debug, Serialize)]
struct ErrorBody {
    /// 기계가 읽는 코드 (`not_found`, `validation_failed` ...)
    error: &'static str,
    /// 사람이 읽는 메시지
    message: String,
    /// 필드별 상세 (검증 실패일 때만)
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<HashMap<String, Vec<String>>>,
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
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::InvalidBody { status, .. } => *status,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Database(_) | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::InvalidBody { .. } => "invalid_body",
            Self::Validation(_) => "validation_failed",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::Conflict(_) => "conflict",
            Self::Database(_) | Self::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();

        let details = match &self {
            Self::Validation(errors) => Some(flatten_validation_errors(errors)),
            _ => None,
        };

        let message = match &self {
            // 500은 내부 사정을 밖으로 흘리지 않는다. 원인은 서버 로그에만.
            Self::Database(err) => {
                eprintln!("[db error] {err:?}");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            Self::Internal(err) => {
                eprintln!("[internal error] {err:?}");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            Self::Validation(_) => "요청 값이 규칙에 맞지 않습니다.".to_string(),
            other => other.to_string(),
        };

        (
            status,
            Json(ErrorBody {
                error: code,
                message,
                details,
            }),
        )
            .into_response()
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
pub async fn handle_405() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "error": "method_not_allowed",
            "message": "이 경로에서 지원하지 않는 HTTP 메서드입니다.",
        })),
    )
        .into_response()
}
