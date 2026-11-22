use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

use crate::response::Author;

/// API 응답으로 나가는 댓글.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: i64,
    pub post_id: i64,
    pub body: String,
    pub author: Author,
    pub created_at: DateTime<Utc>,
}

/// `comments`에 작성자 닉네임(JOIN)을 붙인 한 행.
#[derive(Debug, Clone, FromRow)]
pub struct CommentRow {
    pub id: i64,
    pub post_id: i64,
    pub body: String,
    pub author_id: i64,
    pub author_nickname: String,
    pub created_at: DateTime<Utc>,
}

impl From<CommentRow> for Comment {
    fn from(row: CommentRow) -> Self {
        Self {
            id: row.id,
            post_id: row.post_id,
            body: row.body,
            author: Author {
                id: Some(row.author_id),
                nickname: row.author_nickname,
            },
            created_at: row.created_at,
        }
    }
}

/// 댓글 작성 요청: `{ body }`
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateComment {
    #[validate(length(min = 1, max = 1000, message = "댓글은 1~1000자여야 합니다"))]
    pub body: String,
}
