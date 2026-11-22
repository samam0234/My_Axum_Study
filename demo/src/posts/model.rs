use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use validator::Validate;

use crate::response::Author;

/// API 응답으로 나가는 게시글. **공통 API 스펙 v1 §3**
///
/// 응답 JSON 필드는 camelCase (`commentCount`, `createdAt`).
/// Rust 필드는 관례대로 snake_case로 두고 **직렬화 경계에서만** 이름을 바꾼다.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: i64,
    pub title: String,
    /// 본문. (스펙의 필드 이름은 `body`다. study6 이전에는 `content`였다)
    pub body: String,
    pub tags: Vec<String>,
    pub author: Author,
    pub comment_count: i64,
    /// 업로드된 썸네일 파일명. `/uploads/<파일명>`으로 접근한다. (스펙 밖 확장, study8)
    pub thumbnail: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 조회 쿼리의 한 행. `posts`에 작성자 닉네임(JOIN)과 댓글 수(서브쿼리)를 붙인 모양이다.
///
/// `Post`와 따로 두는 이유: SQLite에는 배열 타입이 없어서 `tags`가 **JSON 문자열**로 저장된다.
/// "DB의 모양"과 "API의 모양"이 다르므로 타입도 나누고, 경계에서 변환한다.
#[derive(Debug, Clone, FromRow)]
pub struct PostRow {
    pub id: i64,
    pub title: String,
    pub body: String,
    /// study6에서 추가된 FK. 인증 도입 전에 만들어진 글은 NULL이다.
    pub author_id: Option<i64>,
    /// `COALESCE(users.nickname, posts.author)` — 가입 회원이면 **현재 닉네임**, 옛 글이면 그때 적은 이름
    pub author_nickname: String,
    pub tags: String,
    pub thumbnail: Option<String>,
    pub comment_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PostRow> for Post {
    fn from(row: PostRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            body: row.body,
            // 저장된 JSON이 깨져 있어도 500을 내지 않고 빈 배열로 처리한다.
            tags: serde_json::from_str(&row.tags).unwrap_or_default(),
            author: Author {
                id: row.author_id,
                nickname: row.author_nickname,
            },
            comment_count: row.comment_count,
            thumbnail: row.thumbnail,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// 생성 요청 본문(DTO). **스펙 v1 §2**: `{ title, body, tags }`
///
/// **작성자는 클라이언트가 정하는 값이 아니라 토큰에서 나오는 값**이다. 그래서 `author` 필드가 없다.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub body: String,

    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}

/// 수정 요청 본문. 모든 필드가 `Option`이라 부분 수정이 가능하다.
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: Option<String>,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub body: Option<String>,

    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Option<Vec<String>>,
}
