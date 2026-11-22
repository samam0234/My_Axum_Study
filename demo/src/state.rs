use std::sync::Arc;

use crate::db::DbPool;
use crate::ratelimit::RateLimiter;

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
    /// 허용할 CORS 출처. 쉼표로 여러 개. (`CORS_ORIGIN`, 스펙 §7)
    pub cors_origin: Arc<String>,
    /// 호출 제한. 기본은 제한 없음. (`RATE_LIMIT_PER_MINUTE`)
    pub rate_limiter: Arc<RateLimiter>,
}

/// 스펙이 정한 개발용 기본 출처.
pub const DEFAULT_CORS_ORIGIN: &str = "http://localhost:5173";

impl AppState {
    pub fn new(db: DbPool, jwt_secret: String, upload_dir: String) -> Self {
        Self {
            db,
            jwt_secret: Arc::new(jwt_secret),
            upload_dir: Arc::new(upload_dir),
            cors_origin: Arc::new(DEFAULT_CORS_ORIGIN.to_string()),
            rate_limiter: Arc::new(RateLimiter::disabled()),
        }
    }

    /// 설정으로 CORS 출처를 바꾼다. (빌더 스타일: 기본값만 쓰는 호출자는 `new`만 쓰면 된다)
    pub fn with_cors_origin(mut self, origin: String) -> Self {
        self.cors_origin = Arc::new(origin);
        self
    }

    /// 분당 허용 횟수. `0`이면 제한하지 않는다.
    pub fn with_rate_limit_per_minute(mut self, limit: u32) -> Self {
        self.rate_limiter = Arc::new(RateLimiter::new(limit, std::time::Duration::from_secs(60)));
        self
    }
}
