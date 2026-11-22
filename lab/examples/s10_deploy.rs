//! study10 — 배포 · 정리. 포트 3010.
//!
//! 컨테이너·서버에서 실제로 문제가 되는 것만 담았다:
//!   ① `BIND_ADDR` — 컨테이너 안에서는 0.0.0.0이어야 밖에서 접속된다
//!   ② 시작 시 설정 검증 — 짧은/기본 비밀키는 부팅 거부·경고
//!   ③ 시작 로그에 **DB 비밀번호를 찍지 않는다** (`redact_url_password`)
//!   ④ **정상 종료** — Ctrl+C(SIGINT)와 **SIGTERM**(docker stop)을 모두 받고, 진행 중인 요청은 마친 뒤 종료
//! (Dockerfile·compose는 demo/Dockerfile, demo/compose.yaml)
//!
//! 실행:  cargo run --example s10_deploy
//!        BIND_ADDR=0.0.0.0:3010 JWT_SECRET="길고-무작위인-비밀키-32자-이상" DATABASE_URL="postgres://u:secret@db:5432/board" cargo run --example s10_deploy
//!        SHUTDOWN_AFTER_MS=500 cargo run --example s10_deploy        # 0.5초 뒤 스스로 종료 신호를 보낸다 (시험용)

use std::{future::Future, time::Duration};

use axum::{Json, Router, extract::Query, routing::get};
use serde::Deserialize;
use serde_json::json;

const DEV_JWT_SECRET: &str = "dev-only-insecure-secret-change-me";

#[derive(Debug, Clone)]
struct Config {
    bind_addr: String,
    jwt_secret: String,
    database_url: String,
}

impl Config {
    fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            bind_addr: env_or("BIND_ADDR", "127.0.0.1:3010"), // 로컬 기본은 내 컴퓨터만. 컨테이너는 0.0.0.0:포트
            jwt_secret: env_or("JWT_SECRET", DEV_JWT_SECRET),
            database_url: env_or("DATABASE_URL", "sqlite://data.db?mode=rwc"),
        };
        if config.jwt_secret.len() < 16 {
            anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)"); // 안전한 쪽으로 실패한다
        }
        if config.jwt_secret == DEV_JWT_SECRET {
            eprintln!("WARN 기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요.");
        }
        Ok(config)
    }

    /// 로그에 찍어도 되는 DB 주소. 비밀번호를 가린다.
    fn redacted_database_url(&self) -> String {
        redact_url_password(&self.database_url)
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// `scheme://user:password@host/...` 의 password를 `***`로 바꾼다. 비밀번호가 없으면 그대로 둔다.
fn redact_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else { return url.to_string() };
    let (scheme, rest) = url.split_at(scheme_end + 3);
    // 사용자 정보는 첫 '/' 앞에 있고 마지막 '@'로 끝난다 (비밀번호에 '@'가 들어 있을 수 있다)
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let Some(at) = rest[..authority_end].rfind('@') else { return url.to_string() };
    match rest[..at].split_once(':') {
        Some((user, _password)) => format!("{scheme}{user}:***{}", &rest[at..]),
        None => url.to_string(),
    }
}

// ── 핸들러 ─────────────────────────────────────────────────────────────────
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" })) // HEALTHCHECK가 찌르는 곳
}

#[derive(Deserialize)]
struct SlowQuery {
    secs: Option<u64>,
}

/// 오래 걸리는 요청. 종료 신호가 와도 **이 요청은 끝까지 응답을 받아야** 한다.
async fn slow(Query(q): Query<SlowQuery>) -> Json<serde_json::Value> {
    let secs = q.secs.unwrap_or(3).min(30);
    tokio::time::sleep(Duration::from_secs(secs)).await;
    Json(json!({ "finished": true, "tookSecs": secs }))
}

fn app() -> Router {
    Router::new().route("/health", get(health)).route("/slow", get(slow))
}

// ── 정상 종료 ──────────────────────────────────────────────────────────────
/// 종료 신호를 기다린다. Ctrl+C(SIGINT)와 SIGTERM을 **둘 다** 받는다.
///
/// SIGTERM을 처리하지 않으면 컨테이너의 PID 1은 신호를 무시해서 `docker stop`이 10초를 기다린 뒤
/// SIGKILL로 죽인다. 진행 중이던 요청이 그대로 끊긴다.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("Ctrl+C 신호를 기다리지 못했습니다: {e}");
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
                eprintln!("SIGTERM 핸들러를 등록하지 못했습니다: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    // Windows에는 SIGTERM이 없다. 영원히 끝나지 않는 future로 채워 둔다.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // 시험용: 환경변수가 있으면 그 시간 뒤에 스스로 종료 신호를 낸다. 실제 신호 없이도 종료 경로를 시험할 수 있다.
    let self_trigger = async {
        match std::env::var("SHUTDOWN_AFTER_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
            Some(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
            None => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = ctrl_c => println!("종료 신호(Ctrl+C)를 받았습니다"),
        _ = terminate => println!("종료 신호(SIGTERM)를 받았습니다"),
        _ = self_trigger => println!("종료 신호(SHUTDOWN_AFTER_MS)를 받았습니다"),
    }
}

/// main과 시험이 함께 쓰는 서버 실행기. 신호 future가 끝나면 **새 연결을 받지 않고 진행 중인 요청을 마친 뒤** 돌아온다.
async fn serve(listener: tokio::net::TcpListener, shutdown: impl Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    axum::serve(listener, app()).with_graceful_shutdown(shutdown).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    println!("s10_deploy  database_url={}  (비밀번호는 가려서 로그에)", config.redacted_database_url());
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    println!("listening on {}", listener.local_addr()?);
    serve(listener, shutdown_signal()).await?;
    println!("서버를 종료했습니다");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn 비밀번호만_가린다() {
        assert_eq!(redact_url_password("postgres://postgres:s3cret@db:5432/board"), "postgres://postgres:***@db:5432/board");
        // 비밀번호에 '@'가 들어 있어도 호스트 앞의 마지막 '@'를 기준으로 가린다
        assert_eq!(redact_url_password("postgres://u:p@ss@db/board"), "postgres://u:***@db/board");
    }

    #[test]
    fn 비밀번호가_없으면_그대로_둔다() {
        for url in ["sqlite://data.db?mode=rwc", "postgres://db:5432/board", "sqlite::memory:"] {
            assert_eq!(redact_url_password(url), url);
        }
    }

    #[tokio::test]
    async fn 종료_신호가_와도_진행_중인_요청은_끝까지_응답한다() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve(listener, async move {
            let _ = stop_rx.await;
        }));

        // 1초 걸리는 요청을 보내 두고
        let started = Instant::now();
        let in_flight = tokio::spawn(async move { reqwest::get(format!("http://{addr}/slow?secs=1")).await.unwrap().json::<serde_json::Value>().await.unwrap() });
        tokio::time::sleep(Duration::from_millis(200)).await;

        // 처리 도중에 종료 신호를 보낸다
        stop_tx.send(()).unwrap();

        // 요청은 끊기지 않고 정상 응답을 받는다
        let body = in_flight.await.unwrap();
        assert_eq!(body["finished"], true);
        assert!(started.elapsed() >= Duration::from_secs(1), "끊기지 않고 끝까지 처리했다");

        // 그리고 서버는 그 뒤에 종료한다
        tokio::time::timeout(Duration::from_secs(3), server).await.expect("종료하지 않았다").unwrap().unwrap();

        // 종료된 뒤에는 새 연결을 받지 않는다
        assert!(reqwest::get(format!("http://{addr}/health")).await.is_err());
    }
}
