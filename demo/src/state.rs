use std::sync::Arc;

use crate::db::DbPool;

/// 앱 전역에서 공유되는 상태.
///
/// study2~4에서는 `Arc<RwLock<Vec<Post>>>`였지만, 이제 DB 커넥션 풀만 들고 있으면 된다.
/// `SqlitePool`은 **내부가 이미 `Arc`** 라서 `Clone`이 저렴하다. 따로 감쌀 필요가 없다.
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    /// JWT 서명 키. `String`은 Clone이 실제 복사라서 `Arc`로 감싼다. (study6)
    pub jwt_secret: Arc<String>,
    /// 업로드 파일을 저장할 폴더. (study9에서 설정으로 분리)
    pub upload_dir: Arc<String>,
}

impl AppState {
    pub fn new(db: DbPool, jwt_secret: String, upload_dir: String) -> Self {
        Self {
            db,
            jwt_secret: Arc::new(jwt_secret),
            upload_dir: Arc::new(upload_dir),
        }
    }
}
