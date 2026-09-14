use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::RwLock;

use crate::posts::model::Post;

/// 앱 전역에서 공유되는 상태.
///
/// - `Clone`이 반드시 필요하다. axum이 요청마다 이 값을 복제해서 핸들러에 넘기기 때문.
/// - 그래서 필드는 전부 `Arc`로 감싼다. 복제해도 포인터만 복사되고 실제 데이터는 하나만 존재한다.
#[derive(Clone)]
pub struct AppState {
    /// 게시글 저장소. DB 대신 쓰는 in-memory 벡터. (study5에서 sqlx로 교체)
    posts: Arc<RwLock<Vec<Post>>>,
    /// 다음에 부여할 id. 락 없이 원자적으로 증가시킨다.
    next_id: Arc<AtomicI64>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            posts: Arc::new(RwLock::new(Vec::new())),
            next_id: Arc::new(AtomicI64::new(1)),
        }
    }

    /// 읽기 락을 잡는다. 동시에 여러 요청이 함께 읽을 수 있다.
    pub async fn posts(&self) -> tokio::sync::RwLockReadGuard<'_, Vec<Post>> {
        self.posts.read().await
    }

    /// 쓰기 락을 잡는다. 이 락을 쥐고 있는 동안 다른 요청은 읽기조차 못 한다.
    pub async fn posts_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, Vec<Post>> {
        self.posts.write().await
    }

    /// 새 id를 발급한다. `fetch_add`는 "더하고 이전 값을 반환"이라 1부터 시작한다.
    pub fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
