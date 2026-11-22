//! 설정 로딩.
//!
//! 지금까지는 `std::env::var(...).unwrap_or_else(...)`를 여기저기 흩어 놨는데,
//! 그러면 "이 앱이 어떤 환경변수를 쓰는가"를 한눈에 알 수 없다.
//! **설정은 한 곳에서, 시작할 때 한 번에** 읽고 검증한다.

use anyhow::Context;

use crate::state::DEFAULT_CORS_ORIGIN;

/// DB를 지정하지 않았을 때의 기본값. 빌드한 feature에 따라 달라진다.
#[cfg(feature = "sqlite")]
const DEFAULT_DATABASE_URL: &str = "sqlite://data.db?mode=rwc";
#[cfg(feature = "postgres")]
const DEFAULT_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/board";

/// 기본 JWT 시크릿. 운영에서 이 값이 쓰이면 경고를 띄운다.
const DEV_JWT_SECRET: &str = "dev-only-insecure-secret-change-me";

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub bind_addr: String,
    pub upload_dir: String,
    /// 허용할 CORS 출처(쉼표로 여러 개). 스펙 §7의 `CORS_ORIGIN`.
    pub cors_origin: String,
    /// IP당 분당 허용 요청 수. `0`이면 제한하지 않는다.
    pub rate_limit_per_minute: u32,
}

impl Config {
    /// 환경변수에서 설정을 읽는다. (`.env`는 [`load_dotenv`]로 미리 불러온 상태여야 한다)
    ///
    /// 우선순위: **실제 환경변수 > .env 파일 > 기본값**
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            database_url: env_or("DATABASE_URL", DEFAULT_DATABASE_URL),
            jwt_secret: env_or("JWT_SECRET", DEV_JWT_SECRET),
            bind_addr: env_or("BIND_ADDR", "127.0.0.1:3000"),
            upload_dir: env_or("UPLOAD_DIR", "uploads"),
            cors_origin: env_or("CORS_ORIGIN", DEFAULT_CORS_ORIGIN),
            rate_limit_per_minute: env_or("RATE_LIMIT_PER_MINUTE", "300")
                .parse()
                .context("RATE_LIMIT_PER_MINUTE는 0 이상의 정수여야 합니다")?,
        };

        config.validate()?;
        Ok(config)
    }

    /// 테스트용 설정 — SQLite는 in-memory DB, PostgreSQL은 `TEST_DATABASE_URL`.
    pub fn for_test() -> Self {
        #[cfg(feature = "sqlite")]
        let database_url = "sqlite::memory:".to_string();
        #[cfg(feature = "postgres")]
        let database_url = env_or("TEST_DATABASE_URL", DEFAULT_DATABASE_URL);

        Self {
            database_url,
            jwt_secret: "test-secret-test-secret-test-secret".to_string(),
            bind_addr: "127.0.0.1:0".to_string(),
            upload_dir: "uploads".to_string(),
            cors_origin: DEFAULT_CORS_ORIGIN.to_string(),
            rate_limit_per_minute: 0,
        }
    }

    /// 로그에 찍어도 되는 DB 주소. `postgres://user:비밀번호@host/db`의 **비밀번호를 가린다.**
    ///
    /// 시작 로그에 `DATABASE_URL`을 그대로 찍으면 비밀번호가 로그 수집 시스템에 남는다.
    pub fn redacted_database_url(&self) -> String {
        redact_url_password(&self.database_url)
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

        // 출처 문자열이 헤더 값으로 쓸 수 없으면 서버가 뜬 뒤가 아니라 지금 알려 준다.
        for origin in self.cors_origin.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if origin.parse::<axum::http::HeaderValue>().is_err() {
                anyhow::bail!("CORS_ORIGIN에 잘못된 값이 있습니다: {origin}");
            }
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

/// `scheme://user:password@host/...` 의 password를 `***`로 바꾼다. 비밀번호가 없으면 그대로 둔다.
fn redact_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let (scheme, rest) = url.split_at(scheme_end + 3);

    // 사용자 정보는 첫 '/' 앞에 있고 마지막 '@'로 끝난다 (비밀번호에 '@'가 들어 있을 수 있다)
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let Some(at) = rest[..authority_end].rfind('@') else {
        return url.to_string();
    };
    let userinfo = &rest[..at];
    match userinfo.split_once(':') {
        Some((user, _password)) => format!("{scheme}{user}:***{}", &rest[at..]),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_url_password;

    #[test]
    fn 비밀번호만_가린다() {
        assert_eq!(
            redact_url_password("postgres://postgres:s3cret@db:5432/board"),
            "postgres://postgres:***@db:5432/board"
        );
        // 비밀번호에 '@'가 들어 있어도 호스트 앞의 마지막 '@'를 기준으로 가린다
        assert_eq!(
            redact_url_password("postgres://u:p@ss@db/board"),
            "postgres://u:***@db/board"
        );
    }

    #[test]
    fn 비밀번호가_없으면_그대로_둔다() {
        assert_eq!(redact_url_password("sqlite://data.db?mode=rwc"), "sqlite://data.db?mode=rwc");
        assert_eq!(redact_url_password("postgres://db:5432/board"), "postgres://db:5432/board");
        assert_eq!(redact_url_password("sqlite::memory:"), "sqlite::memory:");
    }
}
