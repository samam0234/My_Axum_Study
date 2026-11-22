//! 공통 API 스펙 v1에 맞추는 스키마 변경.
//!
//! - `users.username` → `users.nickname` (로그인은 이메일로, 화면 이름은 닉네임)
//! - `posts.tags` 추가 (JSON 문자열)
//! - `comments` 테이블 추가
//!
//! 이미 적용된 `m0001`을 고치지 않고 **새 마이그레이션으로 쌓는다.** 이미 운영 중인 DB에도
//! 같은 순서로 적용되어야 하기 때문이다. 이 코드는 SQLite·PostgreSQL·MariaDB에서 모두 돈다.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 컬럼 이름 변경. 데이터는 그대로 남는다.
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .rename_column(Users::Username, Users::Nickname)
                    .to_owned(),
            )
            .await?;

        // 기존 글은 태그가 없으므로 기본값 '[]'
        manager
            .alter_table(
                Table::alter()
                    .table(Posts::Table)
                    .add_column(text(Posts::Tags).default("[]"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Comments::Table)
                    .if_not_exists()
                    .col(pk_auto(Comments::Id))
                    .col(integer(Comments::PostId))
                    .col(integer(Comments::AuthorId))
                    .col(text(Comments::Body))
                    .col(timestamp_with_time_zone(Comments::CreatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_comments_post")
                            .from(Comments::Table, Comments::PostId)
                            .to(Posts::Table, Posts::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_comments_author")
                            .from(Comments::Table, Comments::AuthorId)
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
                    .name("idx_comments_post_id")
                    .table(Comments::Table)
                    .col(Comments::PostId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Comments::Table).to_owned())
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Posts::Table)
                    .drop_column(Posts::Tags)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .rename_column(Users::Nickname, Users::Username)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Username,
    Nickname,
}

#[derive(DeriveIden)]
enum Posts {
    Table,
    Id,
    Tags,
}

#[derive(DeriveIden)]
enum Comments {
    Table,
    Id,
    PostId,
    AuthorId,
    Body,
    CreatedAt,
}
