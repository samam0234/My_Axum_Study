use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 게시글 엔티티. 응답으로도 그대로 나가므로 `Serialize`가 필요하다.
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 생성 요청 본문(DTO). 클라이언트가 id/시각을 정하지 못하도록 엔티티와 분리한다.
#[derive(Debug, Deserialize)]
pub struct CreatePost {
    pub title: String,
    pub content: String,
    pub author: String,
}

/// 수정 요청 본문. 모든 필드가 `Option`이라 부분 수정이 가능하다.
#[derive(Debug, Deserialize)]
pub struct UpdatePost {
    pub title: Option<String>,
    pub content: Option<String>,
}
