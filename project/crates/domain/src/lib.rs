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
