use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use validator::Validate;

/// DB의 `users` 한 행.
///
/// `password_hash`는 `Serialize`를 구현하지 않는 이 타입 안에만 존재한다.
/// 응답에 실수로 섞여 나갈 일이 구조적으로 없다.
#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

/// API 응답용 사용자 정보. **비밀번호 해시 필드가 아예 없다.**
#[derive(Debug, Clone, Serialize)]
pub struct UserPublic {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}

impl From<UserRow> for UserPublic {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            username: row.username,
            email: row.email,
            created_at: row.created_at,
        }
    }
}

/// 회원가입 요청
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    #[validate(length(min = 2, max = 20, message = "아이디는 2~20자여야 합니다"))]
    pub username: String,

    #[validate(email(message = "이메일 형식이 아닙니다"))]
    pub email: String,

    #[validate(length(min = 8, max = 72, message = "비밀번호는 8자 이상이어야 합니다"))]
    pub password: String,
}

/// 로그인 요청
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    #[validate(length(min = 1, message = "아이디를 입력하세요"))]
    pub username: String,

    #[validate(length(min = 1, message = "비밀번호를 입력하세요"))]
    pub password: String,
}

/// 로그인 성공 응답
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    /// 만료까지 남은 초
    pub expires_in: i64,
    pub user: UserPublic,
}
