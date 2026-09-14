mod m0001_init;

pub use sea_orm_migration::MigratorTrait;
use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    /// 적용할 마이그레이션을 **순서대로** 나열한다.
    /// sqlx가 파일 이름으로 순서를 정했던 것과 달리, 여기서는 코드가 순서를 명시한다.
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m0001_init::Migration)]
    }
}
