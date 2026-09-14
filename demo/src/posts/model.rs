use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// 게시글 엔티티. 응답으로도 그대로 나가므로 `Serialize`가 필요하다.
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 생성 요청 본문(DTO).
///
/// - `deny_unknown_fields`: 모르는 필드가 오면 거부한다. 오타를 조용히 삼키지 않게 해준다.
/// - `validate(...)`: 파싱 이후의 "값이 말이 되는가" 검사.
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub content: String,

    #[validate(length(min = 2, max = 20, message = "작성자는 2~20자여야 합니다"))]
    pub author: String,

    /// 태그는 선택 항목. 없으면 빈 배열이 된다.
    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}

/// 수정 요청 본문. 모든 필드가 `Option`이라 부분 수정이 가능하다.
///
/// `Option<T>` 필드에 `validate`를 달면 **`Some`일 때만** 검사한다.
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
