use std::sync::Arc;

use sea_orm::DatabaseConnection;

/// 앱 상태.
///
/// demo/와 달리 필드가 `Arc`로 감싼 것 하나뿐이다.
/// 업로드 폴더 같은 것은 이 프로젝트에 없고, DB와 서명 키만 있으면 된다.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub jwt_secret: Arc<String>,
    /// HTTPS 환경인지 여부 (쿠키의 Secure 속성 결정)
    pub secure_cookie: bool,
}

impl AppState {
    pub fn new(db: DatabaseConnection, jwt_secret: String, secure_cookie: bool) -> Self {
        Self {
            db,
            jwt_secret: Arc::new(jwt_secret),
            secure_cookie,
        }
    }
}
