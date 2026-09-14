//! 인증 extractor.
//!
//! **이게 Nest의 `Guard`, Spring Security의 필터에 해당하는 자리다.**
//! 차이는 "라우트에 데코레이터를 붙인다"가 아니라 **핸들러 인자로 적는다**는 점.
//!
//! ```ignore
//! // 인증이 필요한 핸들러 — 인자에 AuthUser가 있으면 그게 곧 가드다
//! async fn create(auth: AuthUser, ...) -> AppResult<...>
//!
//! // 인증이 필요 없는 핸들러 — 그냥 안 적으면 된다
//! async fn list(State(state): State<AppState>) -> AppResult<...>
//! ```

use axum::{
    RequestPartsExt,
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};

use super::jwt;
use crate::error::AppError;
use crate::state::AppState;

/// 인증을 통과한 사용자.
///
/// 핸들러 인자에 이 타입을 적기만 하면
/// "Authorization 헤더 확인 → Bearer 토큰 추출 → 서명·만료 검증 → 클레임 파싱"이 전부 일어난다.
/// 실패하면 핸들러는 **아예 호출되지 않고** 401이 나간다.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    // `AppState`를 S에서 꺼낼 수 있으면 된다는 뜻.
    // S == AppState일 때는 axum의 기본 구현(`impl<T: Clone> FromRef<T> for T`)이 알아서 맞춰준다.
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1) Authorization: Bearer <token> 헤더 파싱
        //    TypedHeader는 형식이 틀리면 알아서 거부한다. 직접 문자열을 자를 필요가 없다.
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| {
                AppError::Unauthorized("Authorization 헤더에 Bearer 토큰이 필요합니다".into())
            })?;

        // 2) 앱 상태에서 서명 키를 꺼낸다
        let app_state = AppState::from_ref(state);

        // 3) 서명 + 만료 검증
        let claims = jwt::decode_token(&app_state.jwt_secret, bearer.token()).map_err(|e| {
            use jsonwebtoken::errors::ErrorKind;
            match e.kind() {
                ErrorKind::ExpiredSignature => {
                    AppError::Unauthorized("토큰이 만료되었습니다".into())
                }
                _ => AppError::Unauthorized("토큰이 유효하지 않습니다".into()),
            }
        })?;

        Ok(AuthUser {
            id: claims.sub,
            username: claims.username,
        })
    }
}
