//! 페이지네이션 공통 타입.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    10
}

/// `?page=1&per_page=20&q=검색어&tag=rust` 형태의 쿼리 파라미터.
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    #[serde(default = "default_page")]
    #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    pub page: u32,

    /// 상한을 두지 않으면 `per_page=1000000`으로 서버를 괴롭힐 수 있다.
    #[serde(default = "default_per_page")]
    #[validate(range(min = 1, max = 100, message = "per_page는 1~100이어야 합니다"))]
    pub per_page: u32,

    /// 제목/본문 검색어 (선택)
    pub q: Option<String>,

    /// 태그 필터 (선택)
    pub tag: Option<String>,
}

impl PageQuery {
    pub fn limit(&self) -> i64 {
        self.per_page as i64
    }

    pub fn offset(&self) -> i64 {
        ((self.page - 1) * self.per_page) as i64
    }
}

/// 페이지네이션 응답 껍데기.
///
/// 배열을 그대로 반환하면 "전체가 몇 건인지", "다음 페이지가 있는지"를 알려줄 자리가 없다.
// utoipa 5는 제네릭 스키마를 그대로 지원한다. (4까지 있던 #[aliases]는 제거됨)
#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
    pub total_pages: u32,
    pub has_next: bool,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, query: &PageQuery, total: i64) -> Self {
        let per_page = query.per_page.max(1);
        // 올림 나눗셈: (total + per_page - 1) / per_page
        let total_pages = ((total as u64 + per_page as u64 - 1) / per_page as u64) as u32;

        Self {
            items,
            page: query.page,
            per_page,
            total,
            total_pages,
            has_next: query.page < total_pages,
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

    fn query(page: u32, per_page: u32) -> PageQuery {
        PageQuery {
            page,
            per_page,
            q: None,
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
        let page = Page::new(Vec::<i32>::new(), &query(1, 10), 25);
        assert_eq!(page.total_pages, 3);
        assert!(page.has_next);
    }

    #[test]
    fn 마지막_페이지에서는_has_next가_false다() {
        let page = Page::new(Vec::<i32>::new(), &query(3, 10), 25);
        assert_eq!(page.total_pages, 3);
        assert!(!page.has_next);
    }

    #[test]
    fn 결과가_없으면_total_pages는_0이다() {
        let page = Page::new(Vec::<i32>::new(), &query(1, 10), 0);
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_next);
    }

    #[test]
    fn 검증_규칙이_범위를_막는다() {
        use validator::Validate;

        assert!(query(1, 10).validate().is_ok());
        assert!(query(0, 10).validate().is_err()); // page >= 1
        assert!(query(1, 0).validate().is_err()); // per_page >= 1
        assert!(query(1, 101).validate().is_err()); // per_page <= 100
    }
}
