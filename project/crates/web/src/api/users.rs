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
use crate::dto::{Data, LoginRequest, LoginResponse, SignupRequest, UserResponse};
use crate::error::ApiResult;
use crate::extract::ValidatedJson;
use crate::state::AppState;

/// POST /api/auth/signup → 201 `{ data: { id, email, nickname, createdAt } }`
pub async fn signup(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<SignupRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = user_service::register(&state.db, &body.nickname, &body.email, body.password).await?;

    Ok((StatusCode::CREATED, Json(Data::new(UserResponse::from(user)))))
}

/// POST /api/auth/login — 성공하면 HttpOnly 쿠키를 굽고, 본문에도 `accessToken`을 준다.
///
/// 스펙 §6: 같은 도메인 풀스택은 쿠키를 써도 되고, **이때도 응답 본문 형식은 같다.**
pub async fn login(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> ApiResult<impl IntoResponse> {
    let user = user_service::authenticate(&state.db, &body.email, body.password).await?;

    let token = auth::encode_token(&state.jwt_secret, user.id, &user.nickname)
        .map_err(domain::DomainError::Internal)?;

    let cookie = auth::session_cookie(&token, state.secure_cookie);

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(Data::new(LoginResponse {
            user: UserResponse::from(user),
            access_token: token,
        })),
    ))
}

/// POST /api/auth/logout → 204. 인증이 필요하다. 쿠키를 즉시 만료시킨다.
pub async fn logout(_user: AuthUser) -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::clear_cookie())],
    )
}

/// GET /api/auth/me — 로그인 필요
pub async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Data<UserResponse>>> {
    let model = user_service::find(&state.db, user.id).await?;
    Ok(Json(Data::new(UserResponse::from(model))))
}
