use crate::db::DbPool;

/// 앱 전역에서 공유되는 상태.
///
/// study2~4에서는 `Arc<RwLock<Vec<Post>>>`였지만, 이제 DB 커넥션 풀만 들고 있으면 된다.
/// `SqlitePool`은 **내부가 이미 `Arc`** 라서 `Clone`이 저렴하다. 따로 감쌀 필요가 없다.
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
}

impl AppState {
    pub fn new(db: DbPool) -> Self {
        Self { db }
    }
}
