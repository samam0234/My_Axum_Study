//! `posts` 테이블 엔티티.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    pub title: String,

    #[sea_orm(column_type = "Text")]
    pub body: String,

    /// 작성자 FK. demo/와 달리 **처음부터 NOT NULL**이다.
    /// 인증을 나중에 붙인 게 아니라 설계에 포함되어 있었기 때문.
    pub author_id: i32,

    pub created_at: ChronoDateTimeUtc,
    pub updated_at: ChronoDateTimeUtc,
}

/// 관계 정의. "Post 하나는 User 하나에 속한다."
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::AuthorId",
        to = "super::user::Column::Id",
        on_delete = "Cascade"
    )]
    Author,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
