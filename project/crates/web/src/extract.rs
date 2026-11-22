//! 검증까지 하는 extractor. demo/ study3~4와 같은 패턴이지만, 실패가 `Response`다.
//!
//! (도메인 오류가 아니라 **표현 계층의 오류**이므로 `DomainError`로 만들지 않는다)

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Request, rejection::JsonRejection},
    http::{StatusCode, request::Parts},
    response::Response,
};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error;

pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(json_rejection_to_response)?;

        value
            .validate()
            .map_err(|errors| error::validation_failed(&errors))?;

        Ok(ValidatedJson(value))
    }
}

pub struct ValidatedQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for ValidatedQuery<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let axum::extract::Query(value) =
            axum::extract::Query::<T>::from_request_parts(parts, state)
                .await
                .map_err(|r| error::bad_request(r.body_text()))?;

        value
            .validate()
            .map_err(|errors| error::validation_failed(&errors))?;

        Ok(ValidatedQuery(value))
    }
}

/// 본문 파싱 실패를 스펙의 오류 코드로 옮긴다.
///
/// axum은 "JSON 문법은 맞는데 타입이 틀림"을 422로 주지만 스펙은 400이다. (`BAD_REQUEST`)
fn json_rejection_to_response(rejection: JsonRejection) -> Response {
    let (status, code) = match rejection.status() {
        StatusCode::PAYLOAD_TOO_LARGE => (StatusCode::PAYLOAD_TOO_LARGE, "PAYLOAD_TOO_LARGE"),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            (StatusCode::UNSUPPORTED_MEDIA_TYPE, "UNSUPPORTED_MEDIA_TYPE")
        }
        _ => (StatusCode::BAD_REQUEST, "BAD_REQUEST"),
    };
    error::respond(status, code, rejection.body_text())
}
