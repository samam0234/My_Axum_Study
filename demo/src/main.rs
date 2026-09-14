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
    tracing::info!(database_url = %config.database_url, "database ready");

    upload::ensure_dir(&config.upload_dir).await?;

    let state = AppState::new(pool, config.jwt_secret.clone(), config.upload_dir.clone());
    let app = app(state);

    // 0.7부터 hyper Server가 아니라 tokio TcpListener + axum::serve 조합을 쓴다.
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    // Ctrl+C를 받으면 진행 중인 요청을 마치고 종료한다 (graceful shutdown).
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("서버를 종료했습니다");
    Ok(())
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("종료 신호를 받았습니다"),
        Err(e) => tracing::error!(%e, "종료 신호를 기다리지 못했습니다"),
    }
}
