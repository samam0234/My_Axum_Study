//! 미니 게시판 웹사이트.
//!
//! 이 크레이트는 **HTTP 표현 계층**이다. 비즈니스 규칙은 전부 `domain` 크레이트에 있고,
//! 여기서는 요청을 풀고 → 서비스를 부르고 → 응답으로 옮기는 일만 한다.

use web::{app, init_tracing, state::AppState};

/// 개발용 기본 비밀키. 운영에서 이 값이 쓰이면 경고를 띄운다.
const DEV_JWT_SECRET: &str = "dev-only-insecure-secret-change-me";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let database_url = env_or("DATABASE_URL", "sqlite://board.db?mode=rwc");
    let jwt_secret = env_or("JWT_SECRET", DEV_JWT_SECRET);
    let bind_addr = env_or("BIND_ADDR", "127.0.0.1:4000");
    let static_dir = env_or("STATIC_DIR", "static");
    let secure_cookie = env_or("SECURE_COOKIE", "false") == "true";
    // IP당 분당 허용 요청 수. 0이면 제한하지 않는다.
    let rate_limit: u32 = env_or("RATE_LIMIT_PER_MINUTE", "300")
        .parse()
        .map_err(|_| anyhow::anyhow!("RATE_LIMIT_PER_MINUTE는 0 이상의 정수여야 합니다"))?;

    if jwt_secret.len() < 16 {
        anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)");
    }
    if jwt_secret == DEV_JWT_SECRET {
        tracing::warn!("기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요.");
    }

    let db = domain::connect(&database_url).await?;
    tracing::info!(database_url = %domain::redact_database_url(&database_url), "database ready");

    let state = AppState::new(db, jwt_secret, secure_cookie).with_rate_limit_per_minute(rate_limit);
    let app = app(state, &static_dir);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    // `ConnectInfo`를 켜야 호출 제한 미들웨어가 요청자의 IP를 볼 수 있다.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Ctrl+C(SIGINT)와 SIGTERM(`docker stop`)을 모두 기다린다. SIGTERM을 처리하지 않으면
/// 컨테이너가 10초 뒤 강제 종료된다. (study10)
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(%e, "Ctrl+C 신호를 기다리지 못했습니다");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                tracing::error!(%e, "SIGTERM 핸들러를 등록하지 못했습니다");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("종료 신호(Ctrl+C)를 받았습니다"),
        _ = terminate => tracing::info!("종료 신호(SIGTERM)를 받았습니다"),
    }
}
