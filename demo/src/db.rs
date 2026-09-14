use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;
use std::time::Duration;

pub type DbPool = Pool<Sqlite>;

/// 커넥션 풀을 만들고 마이그레이션을 적용한다.
///
/// 풀(pool)은 "미리 열어둔 DB 연결 묶음"이다. 요청마다 연결을 새로 여는 비용을 없애고,
/// 동시에 열리는 연결 수의 상한을 정해 DB를 보호한다.
pub async fn init(database_url: &str) -> anyhow::Result<DbPool> {
    // in-memory DB는 **연결마다 별개의 DB**가 만들어진다.
    // 풀이 2개 이상이면 "어떤 연결을 받았느냐"에 따라 테이블이 있기도 없기도 하다.
    // 그래서 테스트용 메모리 DB는 연결을 1개로 고정한다.
    let in_memory = database_url.contains(":memory:") || database_url.contains("mode=memory");

    let mut options = SqliteConnectOptions::from_str(database_url)?
        // 파일이 없으면 만든다. (기본값은 false라서 "unable to open database file"이 난다)
        .create_if_missing(true)
        // 쓰기 락을 기다리는 시간. 기본 0이면 동시 쓰기에서 바로 "database is locked"가 난다.
        .busy_timeout(Duration::from_secs(5));

    if !in_memory {
        // WAL 모드: 읽기와 쓰기가 서로를 막지 않는다. SQLite를 서버에서 쓸 때 사실상 필수.
        // (메모리 DB에는 의미가 없다)
        options = options.journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(if in_memory { 1 } else { 5 })
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;

    // migrations/ 폴더의 .sql 파일을 순서대로 적용한다.
    // 매크로라서 **컴파일 시점에** 폴더 내용을 바이너리에 박아 넣는다 → 배포 시 폴더가 없어도 동작.
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
