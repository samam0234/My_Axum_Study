//! 도메인 크레이트.
//!
//! **HTTP를 전혀 모른다.** axum도, StatusCode도, extractor도 의존성에 없다.
//! 여기에 있는 것은 "게시판이라는 것이 무엇인가"뿐이다.
//!
//! 이 크레이트만 따로 가져다 CLI 도구나 배치 작업을 만들 수 있다.

pub mod entity;
pub mod error;
pub mod migration;
pub mod password;
pub mod service;

use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};

// `Migrator::up(...)`을 부르려면 trait이 스코프에 있어야 한다.
// Rust는 trait 메서드를 쓰려면 그 trait을 import해야 한다는 규칙이 있다.
use crate::migration::MigratorTrait;

pub use error::{DomainError, DomainResult};

/// DB에 연결하고 마이그레이션을 적용한다.
pub async fn connect(database_url: &str) -> anyhow::Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(database_url.to_string());
    options
        .max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(5))
        .acquire_timeout(Duration::from_secs(5))
        .sqlx_logging(false); // sqlx의 자체 쿼리 로그를 끄고 tracing으로 통일한다

    let db = Database::connect(options).await?;

    // sqlx는 `.sql` 파일을, SeaORM은 Rust 코드로 정의한 마이그레이션을 실행한다.
    // `up`은 연관 함수(self를 받지 않는다). `Migrator::up(&db, None)` 형태로 부른다.
    migration::Migrator::up(&db, None).await?;

    Ok(db)
}

/// 로그에 찍어도 되는 DB 주소. `scheme://user:비밀번호@host/db`의 **비밀번호를 `***`로 가린다.**
///
/// 시작 로그에 `DATABASE_URL`을 그대로 찍으면 비밀번호가 로그 수집 시스템에 남는다. (study10)
pub fn redact_database_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let (scheme, rest) = url.split_at(scheme_end + 3);

    // 사용자 정보는 첫 '/' 앞에 있고 마지막 '@'로 끝난다 (비밀번호에 '@'가 들어 있을 수 있다)
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let Some(at) = rest[..authority_end].rfind('@') else {
        return url.to_string();
    };
    match rest[..at].split_once(':') {
        Some((user, _password)) => format!("{scheme}{user}:***{}", &rest[at..]),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_database_url;

    #[test]
    fn 비밀번호만_가린다() {
        assert_eq!(redact_database_url("postgres://postgres:s3cret@db:5432/board"), "postgres://postgres:***@db:5432/board");
        assert_eq!(redact_database_url("mysql://root:p@ss@db/board"), "mysql://root:***@db/board");
        assert_eq!(redact_database_url("sqlite://board.db?mode=rwc"), "sqlite://board.db?mode=rwc");
    }
}
