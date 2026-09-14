use axum::{
    Json,
    extract::{FromRequest, Request, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::collections::HashMap;
use validator::Validate;

/// `Json<T>`를 대체하는 커스텀 extractor.
///
/// 역직렬화(serde) → 검증(validator) 두 단계를 한 번에 처리하고,
/// 둘 중 어디서 실패하든 **같은 형식의 JSON 오류**를 돌려준다.
///
/// NestJS의 전역 `ValidationPipe`, Spring의 `@Valid`에 해당하는 자리다.
/// 차이점: 전역 설정이 아니라 **핸들러 시그니처에 타입으로 적힌다.**
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ValidatedJsonRejection;

    // axum 0.8은 trait 안에서 async fn을 그대로 쓴다. (예전엔 #[async_trait]이 필요했다)
    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // 1단계: 기본 Json extractor로 역직렬화
        let Json(value) = Json::<T>::from_request(req, state).await?;
        // 2단계: validator로 규칙 검사
        value.validate()?;
        Ok(ValidatedJson(value))
    }
}

/// 두 종류의 실패를 하나의 타입으로 모은다.
#[derive(Debug)]
pub enum ValidatedJsonRejection {
    /// JSON 파싱 실패 (문법 오류, 필드 누락, Content-Type 불일치 등)
    Json(JsonRejection),
    /// 파싱은 됐지만 값이 규칙을 어김
    Validation(validator::ValidationErrors),
}

// `?` 연산자가 자동 변환할 수 있도록 From을 구현해 둔다.
impl From<JsonRejection> for ValidatedJsonRejection {
    fn from(value: JsonRejection) -> Self {
        Self::Json(value)
    }
}

impl From<validator::ValidationErrors> for ValidatedJsonRejection {
    fn from(value: validator::ValidationErrors) -> Self {
        Self::Validation(value)
    }
}

impl IntoResponse for ValidatedJsonRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Json(rejection) => (
                // 파싱 실패는 axum이 정한 상태 코드를 그대로 살린다 (400 / 415 / 422)
                rejection.status(),
                Json(json!({
                    "error": "invalid_body",
                    "message": rejection.body_text(),
                })),
            )
                .into_response(),

            Self::Validation(errors) => {
                // ValidationErrors → { 필드명: [메시지, ...] } 형태로 평탄화
                // (validator 0.21의 키 타입은 &str이 아니라 Cow<str>이라 to_string()이 필요하다)
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
                                    // 커스텀 메시지를 안 달았으면 규칙 코드("length" 등)를 쓴다
                                    .unwrap_or_else(|| e.code.to_string())
                            })
                            .collect();
                        (field.to_string(), messages)
                    })
                    .collect();

                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({
                        "error": "validation_failed",
                        "message": "요청 값이 규칙에 맞지 않습니다.",
                        "details": details,
                    })),
                )
                    .into_response()
            }
        }
    }
}
