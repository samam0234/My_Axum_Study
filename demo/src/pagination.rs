//! 페이지네이션 공통 타입.

use serde::{Deserialize, Serialize};
use validator::Validate;

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    10
}

/// `?page=1&per_page=20&q=검색어&tag=rust` 형태의 쿼리 파라미터.
#[derive(Debug, Deserialize, Validate)]
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
#[derive(Debug, Serialize)]
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
