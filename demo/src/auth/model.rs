use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

/// DB의 `users` 한 행.
///
/// `password_hash`는 `Serialize`를 구현하지 않는 이 타입 안에만 존재한다.
/// 응답에 실수로 섞여 나갈 일이 구조적으로 없다.
#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub nickname: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

/// API 응답용 사용자 정보. **비밀번호 해시 필드가 아예 없다.**
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserPublic {
    pub id: i64,
    pub email: String,
    pub nickname: String,
    pub created_at: DateTime<Utc>,
}

impl From<UserRow> for UserPublic {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            nickname: row.nickname,
            created_at: row.created_at,
        }
    }
}

/// 회원가입 요청. **스펙 v1 §2**: `{ email, password, nickname }`
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SignupRequest {
    #[validate(email(message = "이메일 형식이 아닙니다"))]
    pub email: String,

    #[validate(length(min = 8, max = 72, message = "비밀번호는 8자 이상이어야 합니다"))]
    pub password: String,

    #[validate(length(min = 2, max = 20, message = "닉네임은 2~20자여야 합니다"))]
    pub nickname: String,
}

/// 로그인 요청. **스펙 v1 §2**: `{ email, password }`
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    #[validate(length(min = 1, message = "이메일을 입력하세요"))]
    pub email: String,

    #[validate(length(min = 1, message = "비밀번호를 입력하세요"))]
    pub password: String,
}

/// 로그인 성공 응답의 `data`. **스펙 v1 §3**: `{ data: { user, accessToken } }`
///
/// 예전에 있던 `tokenType`/`expiresIn`은 스펙에 없어서 뺐다.
/// 토큰 종류는 항상 Bearer이고, 만료 시각은 토큰 안의 `exp`에 들어 있다.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")] // access_token → accessToken
pub struct LoginData {
    pub user: UserPublic,
    pub access_token: String,
}
