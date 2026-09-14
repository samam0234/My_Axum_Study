use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::extractor::AuthUser;
use super::model::{LoginRequest, RegisterRequest, TokenResponse, UserPublic};
use super::{jwt, password, repo};
use crate::error::{AppError, AppResult};
use crate::extract::ValidatedJson;
use crate::state::AppState;

/// POST /auth/register — 회원가입
#[utoipa::path(
    post,
    path = "/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "가입됨", body = UserPublic),
        (status = 409, description = "아이디/이메일 중복"),
        (status = 422, description = "검증 실패"),
    )
)]
pub async fn register(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<RegisterRequest>,
) -> AppResult<impl IntoResponse> {
    let password_hash = password::hash(body.password).await?;

    let row = repo::insert(&state.db, &body.username, &body.email, &password_hash)
        .await
        .map_err(map_unique_violation)?;

    Ok((StatusCode::CREATED, Json(UserPublic::from(row))))
}

/// POST /auth/login — 로그인 후 JWT 발급
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "토큰 발급", body = TokenResponse),
        (status = 401, description = "아이디 또는 비밀번호 불일치"),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> AppResult<Json<TokenResponse>> {
    let row = repo::find_by_username(&state.db, &body.username).await?;

    // ⚠️ "없는 아이디"와 "틀린 비밀번호"를 구분해서 알려주면 안 된다.
    //    공격자가 가입된 아이디 목록을 수집할 수 있게 되기 때문(user enumeration).
    let Some(user) = row else {
        return Err(AppError::Unauthorized(
            "아이디 또는 비밀번호가 올바르지 않습니다".into(),
        ));
    };

    if !password::verify(body.password, user.password_hash.clone()).await? {
        return Err(AppError::Unauthorized(
            "아이디 또는 비밀번호가 올바르지 않습니다".into(),
        ));
    }

    let token = jwt::encode_token(&state.jwt_secret, user.id, &user.username)?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "Bearer",
        expires_in: jwt::TOKEN_TTL_SECONDS,
        user: UserPublic::from(user),
    }))
}

/// GET /auth/me — 내 정보 (인증 필요)
///
/// 인자에 `AuthUser`가 있다는 것만으로 이 라우트는 보호된다.
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "내 정보", body = UserPublic),
        (status = 401, description = "토큰 없음/만료/위조"),
    )
)]
pub async fn me(State(state): State<AppState>, auth: AuthUser) -> AppResult<Json<UserPublic>> {
    repo::find_by_id(&state.db, auth.id)
        .await?
        .map(|row| Json(UserPublic::from(row)))
        // 토큰은 유효한데 사용자가 삭제된 경우
        .ok_or_else(|| AppError::not_found("user", auth.id))
}

/// UNIQUE 제약 위반을 409 Conflict로 바꾼다.
///
/// sqlx::Error를 그대로 `?`로 던지면 500이 된다. "이미 있는 아이디"는 서버 잘못이 아니므로
/// 여기서 의미 있는 상태 코드로 번역해 준다.
fn map_unique_violation(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.is_unique_violation() {
            return AppError::Conflict("이미 사용 중인 아이디 또는 이메일입니다".into());
        }
    }
    AppError::Database(err)
}
