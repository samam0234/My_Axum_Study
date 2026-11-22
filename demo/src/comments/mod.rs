//! 댓글. **공통 API 스펙 v1 §1**
//!
//! | 메서드 | 경로 | 인증 |
//! |---|---|---|
//! | GET | `/api/posts/{id}/comments` | — |
//! | POST | `/api/posts/{id}/comments` | 필요 |
//! | DELETE | `/api/comments/{id}` | 작성자 |
//!
//! 앞의 두 개는 "글 아래의 자원"이라 `posts::router()`에서 이 모듈의 핸들러를 가져다 붙인다.
//! 삭제만 `/api/comments/{id}`라서 이 모듈이 자기 라우터를 가진다.

pub mod handler;
pub mod model;
pub mod repo;

use axum::{Router, routing::delete};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/{id}", delete(handler::delete))
}
