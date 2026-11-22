//! 성공 응답 껍데기. **공통 API 스펙 v1 §3**
//!
//! - 단건: `{ "data": { ... } }`
//! - 목록: `{ "data": [ ... ], "meta": { "page", "limit", "total", "totalPages", "q" } }` (→ `pagination::Paged`)
//!
//! 값을 한 겹 감싸는 이유: 나중에 `meta`나 `links`를 **옆에 추가**해도 기존 클라이언트가 깨지지 않는다.

use serde::Serialize;
use utoipa::ToSchema;

/// `{ "data": T }`
#[derive(Debug, Serialize, ToSchema)]
pub struct Data<T> {
    pub data: T,
}

impl<T> Data<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

/// 글·댓글의 작성자. 스펙은 `{ id, nickname }`이다.
///
/// `id`가 `Option`인 이유: 인증을 도입하기 전(study5)에 만든 글은 `author_id`가 NULL이다.
/// 그런 글은 `{ "id": null, "nickname": "<당시 적은 이름>" }`으로 나간다.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Author {
    pub id: Option<i64>,
    pub nickname: String,
}
