//! 검증까지 하는 extractor. demo/ study3~4와 같은 패턴이지만, 실패가 `Response`다.
//!
//! (도메인 오류가 아니라 **표현 계층의 오류**이므로 `DomainError`로 만들지 않는다)

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Request, rejection::JsonRejection},
    http::request::Parts,
    response::{IntoResponse, Response},
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

fn json_rejection_to_response(rejection: JsonRejection) -> Response {
    let status = rejection.status();
    (
        status,
        Json(serde_json::json!({
            "error": "invalid_body",
            "message": rejection.body_text(),
        })),
    )
        .into_response()
}
