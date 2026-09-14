use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use validator::Validate;

/// API 응답으로 나가는 게시글.
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    pub author_id: Option<i64>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// DB의 `posts` 한 행을 그대로 담는 타입.
///
/// `Post`와 따로 두는 이유: SQLite에는 배열 타입이 없어서 `tags`가 **JSON 문자열**로 저장된다.
/// "DB의 모양"과 "API의 모양"이 다르므로 타입도 나누고, 경계에서 변환한다.
#[derive(Debug, Clone, FromRow)]
pub struct PostRow {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    /// study6에서 추가된 FK. 인증 도입 전에 만들어진 글은 NULL이다.
    pub author_id: Option<i64>,
    pub tags: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PostRow> for Post {
    fn from(row: PostRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            content: row.content,
            author: row.author,
            author_id: row.author_id,
            // 저장된 JSON이 깨져 있어도 500을 내지 않고 빈 배열로 처리한다.
            tags: serde_json::from_str(&row.tags).unwrap_or_default(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// 생성 요청 본문(DTO).
///
/// study5까지 있던 `author` 필드가 사라졌다.
/// **작성자는 클라이언트가 정하는 값이 아니라 토큰에서 나오는 값**이기 때문이다.
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub content: String,

    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}

/// 수정 요청 본문. 모든 필드가 `Option`이라 부분 수정이 가능하다.
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct UpdatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: Option<String>,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub content: Option<String>,

    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Option<Vec<String>>,
}
