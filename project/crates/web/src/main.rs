//! 미니 게시판 웹사이트.
//!
//! 이 크레이트는 **HTTP 표현 계층**이다. 비즈니스 규칙은 전부 `domain` 크레이트에 있고,
//! 여기서는 요청을 풀고 → 서비스를 부르고 → 응답으로 옮기는 일만 한다.

mod api;
mod auth;
mod dto;
mod error;
mod extract;
mod state;

use axum::{
    Router,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use tower_http::{compression::CompressionLayer, services::ServeDir, trace::TraceLayer};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let database_url = env_or("DATABASE_URL", "sqlite://board.db?mode=rwc");
    let jwt_secret = env_or("JWT_SECRET", "dev-only-insecure-secret-change-me");
    let bind_addr = env_or("BIND_ADDR", "127.0.0.1:4000");
    let static_dir = env_or("STATIC_DIR", "static");
    let secure_cookie = env_or("SECURE_COOKIE", "false") == "true";

    if jwt_secret.len() < 16 {
        anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)");
    }

    let db = domain::connect(&database_url).await?;
    tracing::info!(%database_url, "database ready");

    let state = AppState::new(db, jwt_secret, secure_cookie);
    let app = app(state, &static_dir);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn app(state: AppState, static_dir: &str) -> Router {
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

    Router::new()
        // API가 먼저 매칭된다. /api로 시작하지 않는 것은 전부 정적 파일로 간다.
        .nest("/api", api::router())
        .with_state(state)
        // ⚠️ fallback_service는 `.with_state()` 뒤에 붙인다.
        //    ServeDir은 상태를 필요로 하지 않는 Service이기 때문.
        .fallback_service(spa)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
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

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("web=debug,domain=debug,tower_http=info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .try_init();
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("종료 신호를 받았습니다"),
        Err(e) => tracing::error!(%e, "종료 신호를 기다리지 못했습니다"),
    }
}
