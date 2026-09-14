//! 회원/세션 API.
//!
//! 핸들러가 **아주 얇다.** 규칙은 전부 domain 크레이트에 있고,
//! 여기서는 "요청을 풀고, 서비스를 부르고, 응답을 만든다"만 한다.

use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
};
use domain::service::user as user_service;

use crate::auth::{self, AuthUser};
use crate::dto::{LoginRequest, RegisterRequest, UserResponse};
use crate::error::ApiResult;
use crate::extract::ValidatedJson;
use crate::state::AppState;

/// POST /api/auth/register
pub async fn register(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<RegisterRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = user_service::register(&state.db, &body.username, &body.email, body.password).await?;

    Ok((StatusCode::CREATED, Json(UserResponse::from(user))))
}

/// POST /api/auth/login — 성공하면 HttpOnly 쿠키를 굽는다.
pub async fn login(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = user_service::authenticate(&state.db, &body.username, body.password).await?;

    let token = auth::encode_token(&state.jwt_secret, user.id, &user.username)
        .map_err(domain::DomainError::Internal)?;

    Ok((
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            auth::session_cookie(&token, state.secure_cookie),
        )],
        Json(UserResponse::from(user)),
    ))
}

/// POST /api/auth/logout — 쿠키를 즉시 만료시킨다.
pub async fn logout() -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::clear_cookie())],
    )
}

/// GET /api/auth/me — 로그인 필요
pub async fn me(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<UserResponse>> {
    let model = user_service::find(&state.db, user.id).await?;
    Ok(Json(UserResponse::from(model)))
}
