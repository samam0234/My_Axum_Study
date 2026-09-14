pub mod handler;
pub mod model;
pub mod repo;

use axum::{Router, extract::DefaultBodyLimit, routing::{get, post}};

use crate::state::AppState;
use crate::upload::MAX_UPLOAD_BYTES;

/// 게시판 관련 라우트만 모아 둔 하위 라우터.
///
/// 반환 타입이 `Router<AppState>`인 것에 주의. "아직 상태가 주입되지 않은 라우터"라는 뜻이고,
/// 상위에서 `.with_state(state)`를 호출하면 그때 `Router<()>`가 되어 서빙할 수 있게 된다.
pub fn router() -> Router<AppState> {
    Router::new()
        // nest("/posts", ...) 아래에 붙으므로 여기서는 "/"가 곧 "/posts"다.
        .route("/", get(handler::list).post(handler::create))
        .route(
            "/{id}",
            get(handler::get_one)
                .patch(handler::update)
                .delete(handler::delete),
        )
        .route(
            "/{id}/thumbnail",
            // 이 라우트만 본문 한도를 크게 잡는다.
            // 전역 DefaultBodyLimit(2MB)보다 **안쪽**에 붙은 값이 이긴다.
            post(handler::upload_thumbnail).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
}
