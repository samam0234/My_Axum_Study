//! 기본 extractor를 감싸서 **실패를 전부 `AppError`로 바꾸는** 래퍼들.
//!
//! axum 기본 extractor(`axum::extract::Path` 등)는 실패 시 각자의 rejection 타입을 반환하고,
//! 그 응답은 평문이다. 여기서 한 겹 감싸면 오류 응답 형식을 앱 전체에서 통일할 수 있다.

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Request, rejection::JsonRejection},
    http::request::Parts,
};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::AppError;

/// `Json<T>` + `validator` 검증을 한 번에 처리하는 extractor.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = AppError;

    // axum 0.8은 trait 안에서 async fn을 그대로 쓴다. (예전엔 #[async_trait]이 필요했다)
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // 1단계: 기본 Json extractor로 역직렬화
        let Json(value) = Json::<T>::from_request(req, state).await?;
        // 2단계: validator로 규칙 검사
        value.validate()?;
        Ok(ValidatedJson(value))
    }
}

/// `axum::extract::Path`와 같지만 실패가 `AppError::BadRequest`가 된다.
#[derive(Debug, Clone, Copy, Default)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Path(value))
            .map_err(|rejection| AppError::BadRequest(rejection.body_text()))
    }
}

/// `axum::extract::Query`와 같지만 실패가 `AppError::BadRequest`가 된다.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(value)| Query(value))
            .map_err(|rejection| AppError::BadRequest(rejection.body_text()))
    }
}

/// `Query<T>` + `validator` 검증. 페이지네이션 파라미터 검사에 쓴다. (study8)
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatedQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ValidatedQuery<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let axum::extract::Query(value) =
            axum::extract::Query::<T>::from_request_parts(parts, state)
                .await
                .map_err(|rejection| AppError::BadRequest(rejection.body_text()))?;

        value.validate()?;
        Ok(ValidatedQuery(value))
    }
}

// ---------------------------------------------------------------------------
// `?` 연산자가 자동 변환할 수 있도록 From 구현
// ---------------------------------------------------------------------------

impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        // axum이 이미 400/415/422를 적절히 판단해 두었으므로 그대로 살린다.
        AppError::InvalidBody {
            status: rejection.status(),
            message: rejection.body_text(),
        }
    }
}

impl From<validator::ValidationErrors> for AppError {
    fn from(errors: validator::ValidationErrors) -> Self {
        AppError::Validation(errors)
    }
}
