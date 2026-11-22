//! 실행 파일 진입점.
//!
//! 앱 조립은 라이브러리 크레이트(`src/lib.rs`)에 있고, 여기서는
//! **설정 로딩 → 자원 준비 → 서버 실행**만 한다. 그래야 통합 테스트가 같은 라우터를 쓸 수 있다.

use demo::{app, config, config::Config, db, init_tracing, state::AppState, upload};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ⚠️ 순서 주의: `.env`의 RUST_LOG를 반영하려면 tracing 초기화보다 **먼저** 읽어야 한다.
    //    EnvFilter는 init_tracing() 시점의 환경변수를 보고 한 번만 만들어지기 때문.
    config::load_dotenv()?;

    init_tracing();

    let config = Config::from_env()?;
    tracing::info!(?config.bind_addr, "설정을 불러왔습니다");

    let pool = db::init(&config.database_url).await?;
    tracing::info!(database_url = %config.redacted_database_url(), "database ready");

    upload::ensure_dir(&config.upload_dir).await?;

    let state = AppState::new(pool, config.jwt_secret.clone(), config.upload_dir.clone())
        .with_cors_origin(config.cors_origin.clone())
        .with_rate_limit_per_minute(config.rate_limit_per_minute);
    let app = app(state);

    // 0.7부터 hyper Server가 아니라 tokio TcpListener + axum::serve 조합을 쓴다.
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    // Ctrl+C를 받으면 진행 중인 요청을 마치고 종료한다 (graceful shutdown).
    // `ConnectInfo`를 켜야 호출 제한 미들웨어가 요청자의 IP를 볼 수 있다.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("서버를 종료했습니다");
    Ok(())
}

/// 종료 신호를 기다린다.
///
/// - **Ctrl+C (SIGINT)**: 로컬 개발
/// - **SIGTERM**: `docker stop`, 쿠버네티스, systemd가 보내는 "정상 종료해 달라"는 신호
///
/// SIGTERM을 처리하지 않으면 컨테이너의 PID 1은 신호를 무시해서, `docker stop`이
/// **10초를 기다린 뒤 SIGKILL**로 죽인다. 진행 중이던 요청이 끊기고 배포가 느려진다. (study10)
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
    // Windows에는 SIGTERM이 없다. 영원히 끝나지 않는 future로 채워 둔다.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("종료 신호(Ctrl+C)를 받았습니다"),
        _ = terminate => tracing::info!("종료 신호(SIGTERM)를 받았습니다"),
    }
}
