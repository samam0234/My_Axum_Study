use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::extractor::AuthUser;
use super::model::{LoginData, LoginRequest, SignupRequest, UserPublic};
use super::{jwt, password, repo};
use crate::error::{AppError, AppResult};
use crate::extract::ValidatedJson;
use crate::response::Data;
use crate::state::AppState;

/// POST /api/auth/signup — 회원가입
#[utoipa::path(
    post,
    path = "/api/auth/signup",
    tag = "auth",
    request_body = SignupRequest,
    responses(
        (status = 201, description = "가입됨", body = Data<UserPublic>),
        (status = 409, description = "이메일 중복 (EMAIL_TAKEN) 또는 닉네임 중복 (CONFLICT)"),
        (status = 400, description = "검증 실패 (VALIDATION_ERROR)"),
    )
)]
pub async fn signup(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<SignupRequest>,
) -> AppResult<impl IntoResponse> {
    let password_hash = password::hash(body.password).await?;

    // 이메일은 대소문자를 구분하지 않는 것이 사용자 기대에 맞다. 저장도 조회도 소문자로 통일한다.
    let email = normalize_email(&body.email);

    let row = repo::insert(&state.db, body.nickname.trim(), &email, &password_hash)
        .await
        .map_err(map_unique_violation)?;

    Ok((StatusCode::CREATED, Json(Data::new(UserPublic::from(row)))))
}

/// POST /api/auth/login — 로그인 후 JWT 발급
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "토큰 발급", body = Data<LoginData>),
        (status = 401, description = "이메일 또는 비밀번호 불일치 (INVALID_CREDENTIALS)"),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> AppResult<Json<Data<LoginData>>> {
    let row = repo::find_by_email(&state.db, &normalize_email(&body.email)).await?;

    // ⚠️ "없는 이메일"과 "틀린 비밀번호"를 구분해서 알려주면 안 된다.
    //    공격자가 가입된 이메일 목록을 수집할 수 있게 되기 때문(user enumeration).
    let Some(user) = row else {
        return Err(AppError::InvalidCredentials);
    };

    if !password::verify(body.password, user.password_hash.clone()).await? {
        return Err(AppError::InvalidCredentials);
    }

    let token = jwt::encode_token(&state.jwt_secret, user.id, &user.nickname)?;

    Ok(Json(Data::new(LoginData {
        user: UserPublic::from(user),
        access_token: token,
    })))
}

/// POST /api/auth/logout — 로그아웃 (인증 필요, 204)
///
/// Bearer 토큰은 서버가 상태를 갖지 않는(stateless) 방식이라 **서버가 지울 것이 없다.**
/// 클라이언트가 토큰을 버리면 끝이다. 스펙이 이 경로를 두는 이유는 "쿠키 방식일 때 쿠키를 지우는 자리"를
/// 모든 백엔드가 똑같이 갖게 하려는 것이다. 토큰이 만료 전에 새 나갔을 때 즉시 무효화하려면
/// 서버에 "취소된 토큰 목록"을 두어야 한다.
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "로그아웃됨"),
        (status = 401, description = "토큰 없음/만료/위조 (UNAUTHORIZED)"),
    )
)]
pub async fn logout(_auth: AuthUser) -> StatusCode {
    StatusCode::NO_CONTENT
}

/// GET /api/auth/me — 내 정보 (인증 필요)
///
/// 인자에 `AuthUser`가 있다는 것만으로 이 라우트는 보호된다.
#[utoipa::path(
    get,
    path = "/api/auth/me",
    tag = "auth",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "내 정보", body = Data<UserPublic>),
        (status = 401, description = "토큰 없음/만료/위조 (UNAUTHORIZED)"),
    )
)]
pub async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Data<UserPublic>>> {
    repo::find_by_id(&state.db, auth.id)
        .await?
        .map(|row| Json(Data::new(UserPublic::from(row))))
        // 토큰은 유효한데 사용자가 삭제된 경우
        .ok_or_else(|| AppError::not_found("user", auth.id))
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// UNIQUE 제약 위반을 409로 바꾼다.
///
/// sqlx::Error를 그대로 `?`로 던지면 500이 된다. "이미 있는 이메일"은 서버 잘못이 아니므로
/// 여기서 의미 있는 상태 코드로 번역해 준다.
///
/// 이메일과 닉네임 둘 다 UNIQUE라서 **어느 쪽이 걸렸는지**를 오류 메시지에서 가려낸다.
/// SQLite는 `UNIQUE constraint failed: users.nickname`, PostgreSQL은 제약 이름
/// (`users_username_key`: 컬럼 이름을 바꾸기 전 이름이 그대로 남아 있다)이 들어 있다.
fn map_unique_violation(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.is_unique_violation() {
            let message = db_err.message();
            if message.contains("nickname") || message.contains("username") {
                return AppError::Conflict("이미 사용 중인 닉네임입니다".into());
            }
            return AppError::EmailTaken;
        }
    }
    AppError::Database(err)
}
