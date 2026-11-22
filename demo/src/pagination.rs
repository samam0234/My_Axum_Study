//! 페이지네이션 공통 타입.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::{Validate, ValidationError};

fn default_page() -> u32 {
    1
}
fn default_limit() -> u32 {
    10
}

/// 스펙이 허용하는 `limit` 상한. 이보다 크게 보내면 **오류가 아니라 이 값으로 줄인다.** (스펙 §4)
pub const MAX_LIMIT: u32 = 50;

/// `?page=1&limit=20&q=검색어&sort=-createdAt` 형태의 쿼리 파라미터. **공통 API 스펙 v1 §4**
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// 1부터 센다.
    #[serde(default = "default_page")]
    #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    pub page: u32,

    /// 기본 10, 최대 50. 50을 넘기면 50으로 줄인다. (`effective_limit`)
    #[serde(default = "default_limit")]
    #[validate(range(min = 1, message = "limit는 1 이상이어야 합니다"))]
    pub limit: u32,

    /// 제목·본문 부분 일치 (대소문자 무시)
    pub q: Option<String>,

    /// 정렬: `createdAt`, `-createdAt`(기본), `-commentCount`. `-`는 내림차순.
    #[validate(custom(function = "validate_sort"))]
    pub sort: Option<String>,

    /// 태그 필터 (스펙 밖 확장)
    pub tag: Option<String>,
}

/// 허용하는 정렬 기준. 문자열을 그대로 SQL에 붙이지 않고 **enum으로 바꿔서** 고정된 SQL 조각만 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    CreatedAtDesc,
    CreatedAtAsc,
    CommentCountDesc,
}

impl Sort {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "-createdAt" => Some(Self::CreatedAtDesc),
            "createdAt" => Some(Self::CreatedAtAsc),
            "-commentCount" => Some(Self::CommentCountDesc),
            _ => None,
        }
    }

    /// `ORDER BY` 뒤에 붙는 고정 문자열. (사용자 입력이 아니라 위 세 값 중 하나)
    pub fn order_by(self) -> &'static str {
        match self {
            Self::CreatedAtDesc => " ORDER BY p.created_at DESC, p.id DESC",
            Self::CreatedAtAsc => " ORDER BY p.created_at ASC, p.id ASC",
            Self::CommentCountDesc => " ORDER BY comment_count DESC, p.created_at DESC, p.id DESC",
        }
    }
}

fn validate_sort(raw: &str) -> Result<(), ValidationError> {
    if Sort::parse(raw).is_some() {
        Ok(())
    } else {
        Err(ValidationError::new("sort")
            .with_message("sort는 createdAt, -createdAt, -commentCount 중 하나여야 합니다".into()))
    }
}

impl PageQuery {
    /// 실제로 쓰는 개수. 상한(50)을 넘긴 값은 50으로 줄인다.
    pub fn effective_limit(&self) -> u32 {
        self.limit.clamp(1, MAX_LIMIT)
    }

    pub fn limit(&self) -> i64 {
        self.effective_limit() as i64
    }

    pub fn offset(&self) -> i64 {
        (self.page.saturating_sub(1) as i64) * self.effective_limit() as i64
    }

    pub fn sort(&self) -> Sort {
        self.sort
            .as_deref()
            .and_then(Sort::parse)
            .unwrap_or(Sort::CreatedAtDesc)
    }
}

/// 목록 응답의 `meta`. **공통 API 스펙 v1 §3**
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    pub page: u32,
    pub limit: u32,
    pub total: i64,
    pub total_pages: u32,
    /// 검색어를 그대로 돌려준다. 없으면 `null`.
    pub q: Option<String>,
}

/// 목록 응답: `{ data: [...], meta: {...} }`
// utoipa 5는 제네릭 스키마를 그대로 지원한다. (4까지 있던 #[aliases]는 제거됨)
#[derive(Debug, Serialize, ToSchema)]
pub struct Paged<T> {
    pub data: Vec<T>,
    pub meta: PageMeta,
}

impl<T> Paged<T> {
    pub fn new(data: Vec<T>, query: &PageQuery, total: i64) -> Self {
        let limit = query.effective_limit();
        // 올림 나눗셈: (total + limit - 1) / limit
        let total_pages = ((total as u64 + limit as u64 - 1) / limit as u64) as u32;

        Self {
            data,
            meta: PageMeta {
                page: query.page,
                limit,
                total,
                total_pages,
                q: query.q.clone().filter(|s| !s.trim().is_empty()),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// 유닛 테스트
//
// Rust는 테스트를 **같은 파일 안**에 둔다. `#[cfg(test)]`가 붙어 있어서
// 일반 빌드에는 포함되지 않고, `cargo test`에서만 컴파일된다.
// 같은 모듈이므로 비공개 항목에도 접근할 수 있다.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn query(page: u32, limit: u32) -> PageQuery {
        PageQuery {
            page,
            limit,
            q: None,
            sort: None,
            tag: None,
        }
    }

    #[test]
    fn offset은_페이지와_크기로_계산된다() {
        assert_eq!(query(1, 10).offset(), 0);
        assert_eq!(query(2, 10).offset(), 10);
        assert_eq!(query(3, 25).offset(), 50);
    }

    #[test]
    fn total_pages는_올림_계산이다() {
        // 25건을 10개씩 → 3페이지 (2.5가 아니라)
        let page = Paged::new(Vec::<i32>::new(), &query(1, 10), 25);
        assert_eq!(page.meta.total_pages, 3);
    }

    #[test]
    fn 결과가_없으면_total_pages는_0이다() {
        let page = Paged::new(Vec::<i32>::new(), &query(1, 10), 0);
        assert_eq!(page.meta.total_pages, 0);
    }

    #[test]
    fn limit이_50을_넘으면_50으로_줄인다() {
        let q = query(1, 1000);
        assert_eq!(q.effective_limit(), 50);
        assert_eq!(Paged::new(Vec::<i32>::new(), &q, 120).meta.limit, 50);
        // 줄인 값으로 offset도 계산한다
        assert_eq!(query(2, 1000).offset(), 50);
    }

    #[test]
    fn 검증_규칙이_범위를_막는다() {
        assert!(query(1, 10).validate().is_ok());
        assert!(query(0, 10).validate().is_err()); // page >= 1
        assert!(query(1, 0).validate().is_err()); // limit >= 1
    }

    #[test]
    fn sort는_허용한_값만_받는다() {
        let mut q = query(1, 10);
        assert_eq!(q.sort(), Sort::CreatedAtDesc); // 기본값
        for (raw, want) in [
            ("createdAt", Sort::CreatedAtAsc),
            ("-createdAt", Sort::CreatedAtDesc),
            ("-commentCount", Sort::CommentCountDesc),
        ] {
            q.sort = Some(raw.into());
            assert!(q.validate().is_ok());
            assert_eq!(q.sort(), want);
        }
        q.sort = Some("title; DROP TABLE posts".into());
        assert!(q.validate().is_err());
    }
}
