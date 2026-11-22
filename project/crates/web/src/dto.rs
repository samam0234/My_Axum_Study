//! HTTP 요청/응답 전용 타입. **공통 API 스펙 v1 §2~§4**
//!
//! **도메인 타입이 그대로 밖으로 나가지 않는다.** 경계에서 한 번 옮겨 담는다.
//! JSON 모양(camelCase, `author` 객체, `{ data, meta }` 껍데기)을 정하는 것은 이 파일이다.
//! 도메인의 `PostView`에는 `Serialize`조차 없다.

use domain::entity::user;
use domain::service::comment::CommentView;
use domain::service::post::{PostPage, PostView, Sort};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

// ── 요청 ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct SignupRequest {
    #[validate(email(message = "이메일 형식이 아닙니다"))]
    pub email: String,

    #[validate(length(min = 8, max = 72, message = "비밀번호는 8자 이상이어야 합니다"))]
    pub password: String,

    #[validate(length(min = 2, max = 20, message = "닉네임은 2~20자여야 합니다"))]
    pub nickname: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    #[validate(length(min = 1, message = "이메일을 입력하세요"))]
    pub email: String,

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

    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct UpdatePostRequest {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: Option<String>,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub body: Option<String>,

    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct CreateCommentRequest {
    #[validate(length(min = 1, max = 1000, message = "댓글은 1~1000자여야 합니다"))]
    pub body: String,
}

fn default_page() -> u64 {
    1
}
fn default_limit() -> u64 {
    10
}

/// 스펙이 허용하는 `limit` 상한. 이보다 크게 보내면 **오류가 아니라 이 값으로 줄인다.** (스펙 §4)
pub const MAX_LIMIT: u64 = 50;

/// `?page=1&limit=10&q=검색어&sort=-createdAt`
#[derive(Debug, Deserialize, Validate)]
pub struct ListQuery {
    #[serde(default = "default_page")]
    #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    pub page: u64,

    /// 기본 10. 50을 넘기면 50으로 줄인다. (`effective_limit`)
    #[serde(default = "default_limit")]
    #[validate(range(min = 1, message = "limit는 1 이상이어야 합니다"))]
    pub limit: u64,

    pub q: Option<String>,

    /// `createdAt`, `-createdAt`(기본), `-commentCount`
    #[validate(custom(function = "validate_sort"))]
    pub sort: Option<String>,
}

fn validate_sort(raw: &str) -> Result<(), ValidationError> {
    if Sort::parse(raw).is_some() {
        Ok(())
    } else {
        Err(ValidationError::new("sort")
            .with_message("sort는 createdAt, -createdAt, -commentCount 중 하나여야 합니다".into()))
    }
}

impl ListQuery {
    pub fn effective_limit(&self) -> u64 {
        self.limit.clamp(1, MAX_LIMIT)
    }

    pub fn sort(&self) -> Sort {
        self.sort
            .as_deref()
            .and_then(Sort::parse)
            .unwrap_or(Sort::CreatedAtDesc)
    }
}

// ── 응답 ───────────────────────────────────────────────────────────────

/// `{ "data": T }` — 단건 응답의 껍데기.
#[derive(Debug, Serialize)]
pub struct Data<T> {
    pub data: T,
}

impl<T> Data<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

/// 목록 응답: `{ data: [...], meta: { page, limit, total, totalPages, q } }`
#[derive(Debug, Serialize)]
pub struct Paged<T> {
    pub data: Vec<T>,
    pub meta: PageMeta,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    pub page: u64,
    pub limit: u64,
    pub total: u64,
    pub total_pages: u64,
    /// 검색어를 그대로 돌려준다. 없으면 `null`.
    pub q: Option<String>,
}

impl Paged<PostResponse> {
    pub fn from_page(page: PostPage, q: Option<String>) -> Self {
        Self {
            data: page.items.into_iter().map(PostResponse::from).collect(),
            meta: PageMeta {
                page: page.page,
                limit: page.limit,
                total: page.total,
                total_pages: page.total_pages,
                q: q.filter(|s| !s.trim().is_empty()),
            },
        }
    }
}

/// 글·댓글의 작성자: `{ id, nickname }`
#[derive(Debug, Serialize)]
pub struct Author {
    pub id: i32,
    pub nickname: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostResponse {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub author: Author,
    pub comment_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<PostView> for PostResponse {
    fn from(view: PostView) -> Self {
        Self {
            id: view.id,
            title: view.title,
            body: view.body,
            tags: view.tags,
            author: Author {
                id: view.author_id,
                nickname: view.author_nickname,
            },
            comment_count: view.comment_count,
            created_at: view.created_at,
            updated_at: view.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentResponse {
    pub id: i32,
    pub post_id: i32,
    pub body: String,
    pub author: Author,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<CommentView> for CommentResponse {
    fn from(view: CommentView) -> Self {
        Self {
            id: view.id,
            post_id: view.post_id,
            body: view.body,
            author: Author {
                id: view.author_id,
                nickname: view.author_nickname,
            },
            created_at: view.created_at,
        }
    }
}

/// 가입·로그인·내 정보 응답용 사용자 정보. **비밀번호 해시 필드가 아예 없다.**
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    pub id: i32,
    pub email: String,
    pub nickname: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<user::Model> for UserResponse {
    fn from(model: user::Model) -> Self {
        Self {
            id: model.id,
            email: model.email,
            nickname: model.nickname,
            created_at: model.created_at,
        }
    }
}

/// 로그인 응답의 `data`: `{ user, accessToken }`.
///
/// 이 프로젝트는 세션 쿠키도 굽지만(같은 도메인 풀스택이라 스펙 §6이 허용한다) **본문 형식은 같다.**
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub user: UserResponse,
    pub access_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(limit: u64, sort: Option<&str>) -> ListQuery {
        ListQuery {
            page: 1,
            limit,
            q: None,
            sort: sort.map(str::to_string),
        }
    }

    #[test]
    fn limit이_50을_넘으면_50으로_줄인다() {
        assert_eq!(query(1000, None).effective_limit(), 50);
        assert_eq!(query(7, None).effective_limit(), 7);
    }

    #[test]
    fn sort는_허용한_값만_받는다() {
        assert!(query(10, Some("-commentCount")).validate().is_ok());
        assert!(query(10, Some("title; DROP TABLE posts")).validate().is_err());
        assert_eq!(query(10, None).sort(), Sort::CreatedAtDesc);
    }
}
