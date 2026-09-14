pub mod posts;
pub mod users;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

/// `/api` 아래의 모든 라우트.
///
/// demo/는 기능별 모듈이 각자 `router()`를 내놓고 최상위에서 `nest`했다.
/// 여기서는 **api 모듈이 한 번에 조립**한다. 라우트 표가 한 화면에 들어와서 보기 좋다.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(users::register))
        .route("/auth/login", post(users::login))
        .route("/auth/logout", post(users::logout))
        .route("/auth/me", get(users::me))
        .route("/posts", get(posts::list).post(posts::create))
        .route(
            "/posts/{id}",
            get(posts::get_one)
                .patch(posts::update)
                .delete(posts::delete),
        )
        // /api 아래에서 매칭이 안 되면 JSON 404를 준다.
        // 이게 없으면 상위의 SPA fallback이 걸려서 **API 요청에 index.html이 응답**된다.
        .fallback(crate::error::not_found)
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
