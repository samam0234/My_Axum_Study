//! HTTP 요청/응답 전용 타입.
//!
//! **도메인 엔티티가 그대로 밖으로 나가지 않는다.** 경계에서 한 번 걸러 준다.

use serde::{Deserialize, Serialize};
use validator::Validate;

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

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    #[validate(length(min = 1, message = "아이디를 입력하세요"))]
    pub username: String,

    #[validate(length(min = 1, message = "비밀번호를 입력하세요"))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub body: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct UpdatePostRequest {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: Option<String>,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub body: Option<String>,
}

fn default_page() -> u64 {
    1
}
fn default_per_page() -> u64 {
    10
}

#[derive(Debug, Deserialize, Validate)]
pub struct ListQuery {
    #[serde(default = "default_page")]
    #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    pub page: u64,

    #[serde(default = "default_per_page")]
    #[validate(range(min = 1, max = 50, message = "per_page는 1~50이어야 합니다"))]
    pub per_page: u64,

    pub q: Option<String>,
}

/// 로그인/내 정보 응답용 사용자 정보.
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: i32,
    pub username: String,
    pub email: String,
}

impl From<domain::entity::user::Model> for UserResponse {
    fn from(model: domain::entity::user::Model) -> Self {
        Self {
            id: model.id,
            username: model.username,
            email: model.email,
        }
    }
}
