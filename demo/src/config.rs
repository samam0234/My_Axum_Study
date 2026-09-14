//! 설정 로딩.
//!
//! 지금까지는 `std::env::var(...).unwrap_or_else(...)`를 여기저기 흩어 놨는데,
//! 그러면 "이 앱이 어떤 환경변수를 쓰는가"를 한눈에 알 수 없다.
//! **설정은 한 곳에서, 시작할 때 한 번에** 읽고 검증한다.

use anyhow::Context;

/// 기본 JWT 시크릿. 운영에서 이 값이 쓰이면 경고를 띄운다.
const DEV_JWT_SECRET: &str = "dev-only-insecure-secret-change-me";

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub bind_addr: String,
    pub upload_dir: String,
}

impl Config {
    /// 환경변수에서 설정을 읽는다. (`.env`는 [`load_dotenv`]로 미리 불러온 상태여야 한다)
    ///
    /// 우선순위: **실제 환경변수 > .env 파일 > 기본값**
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            database_url: env_or("DATABASE_URL", "sqlite://data.db?mode=rwc"),
            jwt_secret: env_or("JWT_SECRET", DEV_JWT_SECRET),
            bind_addr: env_or("BIND_ADDR", "127.0.0.1:3000"),
            upload_dir: env_or("UPLOAD_DIR", "uploads"),
        };

        config.validate()?;
        Ok(config)
    }

    /// 테스트용 설정 — in-memory DB.
    pub fn for_test() -> Self {
        Self {
            database_url: "sqlite::memory:".to_string(),
            jwt_secret: "test-secret-test-secret-test-secret".to_string(),
            bind_addr: "127.0.0.1:0".to_string(),
            upload_dir: "uploads".to_string(),
        }
    }

    /// 시작 시점에 잘못된 설정을 잡는다. **서버가 뜬 뒤에 발견하는 것보다 낫다.**
    fn validate(&self) -> anyhow::Result<()> {
        if self.jwt_secret.len() < 16 {
            anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)");
        }

        if self.jwt_secret == DEV_JWT_SECRET {
            tracing::warn!(
                "기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요."
            );
        }

        Ok(())
    }
}

/// `.env` 파일을 환경변수로 읽어들인다.
///
/// **반드시 `init_tracing()`보다 먼저 호출해야 한다.** `EnvFilter`는 초기화 시점의
/// `RUST_LOG`를 한 번만 읽기 때문에, 순서가 반대면 `.env`의 `RUST_LOG`가 무시된다.
///
/// `dotenvy::dotenv()`는 **이미 설정된 환경변수를 덮어쓰지 않는다.**
/// 그래서 `$env:RUST_LOG=...`로 준 값이 `.env`보다 우선한다.
pub fn load_dotenv() -> anyhow::Result<()> {
    // .env가 없어도 에러가 아니다. 컨테이너 배포에서는 보통 없다.
    match dotenvy::dotenv() {
        Ok(_) => {}
        Err(e) if e.not_found() => {}
        Err(e) => return Err(e).context(".env 파일을 읽지 못했습니다"),
    }
    Ok(())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
