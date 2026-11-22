//! 미니 게시판 웹사이트.
//!
//! 이 크레이트는 **HTTP 표현 계층**이다. 비즈니스 규칙은 전부 `domain` 크레이트에 있고,
//! 여기서는 요청을 풀고 → 서비스를 부르고 → 응답으로 옮기는 일만 한다.
//!
//! `main.rs`(실행 파일)와 `tests/`(통합 테스트)가 **같은 라우터를 공유**하도록
//! 앱 조립 로직을 라이브러리에 둔다. (demo/와 같은 구조, study8)

pub mod api;
pub mod auth;
pub mod dto;
pub mod error;
pub mod extract;
pub mod middleware;
pub mod ratelimit;
pub mod state;

use axum::{
    Json, Router,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::state::AppState;

/// 라우터 조립. `main`에서도, 통합 테스트에서도 **이 함수 하나**를 쓴다.
pub fn app(state: AppState, static_dir: &str) -> Router {
    // 정적 파일 서비스. 파일이 없으면 index.html을 돌려준다(SPA 라우팅 대응).
    //
    // ⚠️ `not_found_service`가 아니라 **`fallback`** 을 쓴다.
    //    `not_found_service`는 내부적으로 `SetStatus::new(svc, NOT_FOUND)`로 감싸서
    //    **응답 상태를 404로 강제**한다. SPA 진입점은 200이어야 하므로 `fallback`이 맞다.
    //    (지금은 해시 라우팅이라 티가 안 나지만, History API 라우팅으로 바꾸면 문제가 된다)
    let index_path = format!("{static_dir}/index.html");
    let spa = ServeDir::new(static_dir).fallback(get(move || {
        let path = index_path.clone();
        async move { serve_index(path).await }
    }));

    // `/api` 라우터에만 거는 계층. 아래에 적은 것이 안쪽이다.
    //   error_envelope → CatchPanic → 호출 제한 → 핸들러
    // error_envelope가 CatchPanic보다 바깥이어야 패닉 500에도 requestId가 붙는다.
    let api = api::router()
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            ratelimit::rate_limit,
        ))
        .layer(CatchPanicLayer::custom(middleware::on_panic))
        .layer(axum::middleware::from_fn(middleware::error_envelope));

    Router::new()
        // 스펙 §1: 헬스 체크만 `/api` 접두어 없이. 호출 제한에도 걸리지 않는다.
        .route("/health", get(health))
        // API가 먼저 매칭된다. /api로 시작하지 않는 것은 전부 정적 파일로 간다.
        .nest("/api", api)
        .with_state(state)
        // ⚠️ fallback_service는 `.with_state()` 뒤에 붙인다.
        //    ServeDir은 상태를 필요로 하지 않는 Service이기 때문.
        .fallback_service(spa)
        // 전체 앱에 거는 계층. **나중에 적은 것이 바깥**이다.
        // 요청 ID(가장 바깥) → 응답으로 되돌려 보내기 → 로그 → 압축
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(middleware::REQUEST_ID_HEADER))
        .layer(SetRequestIdLayer::new(
            middleware::REQUEST_ID_HEADER,
            MakeRequestUuid,
        ))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

/// SPA 진입점(index.html)을 200으로 돌려준다.
async fn serve_index(path: String) -> Response {
    match tokio::fs::read(&path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], bytes).into_response(),
        Err(e) => {
            tracing::error!(%path, %e, "index.html을 읽지 못했습니다");
            (StatusCode::INTERNAL_SERVER_ERROR, "index.html을 찾을 수 없습니다").into_response()
        }
    }
}

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("web=debug,domain=debug,tower_http=info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .try_init();
}
