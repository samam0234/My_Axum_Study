//! 초기 스키마.
//!
//! demo/는 `.sql` 파일을 직접 썼다. 여기서는 **Rust 코드로 스키마를 조립**한다.
//! - 장점: 타입 체크가 되고, DB 종류가 바뀌어도(SQLite → Postgres) 같은 코드가 동작한다.
//! - 단점: SQL을 아는 사람에게는 한 겹 더 번역해야 해서 오히려 읽기 어렵다.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(pk_auto(Users::Id))
                    .col(string_uniq(Users::Username))
                    .col(string_uniq(Users::Email))
                    .col(string(Users::PasswordHash))
                    .col(timestamp_with_time_zone(Users::CreatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Posts::Table)
                    .if_not_exists()
                    .col(pk_auto(Posts::Id))
                    .col(string(Posts::Title))
                    .col(text(Posts::Body))
                    .col(integer(Posts::AuthorId))
                    .col(timestamp_with_time_zone(Posts::CreatedAt))
                    .col(timestamp_with_time_zone(Posts::UpdatedAt))
                    // 외래키를 스키마 차원에서 건다. 작성자가 지워지면 글도 함께 지워진다.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_posts_author")
                            .from(Posts::Table, Posts::AuthorId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_posts_author_id")
                    .table(Posts::Table)
                    .col(Posts::AuthorId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    /// demo/의 sqlx 마이그레이션에는 없던 것: **되돌리기**.
    /// SeaORM은 up/down을 쌍으로 요구하므로 롤백이 가능하다.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Posts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await?;
        Ok(())
    }
}

/// 테이블/컬럼 이름을 문자열이 아니라 **enum**으로 적는다.
/// 오타가 컴파일 에러가 되고, 이름을 바꿀 때 IDE 리네임이 통한다.
#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Username,
    Email,
    PasswordHash,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    Title,
    Body,
    AuthorId,
    CreatedAt,
    UpdatedAt,
}
