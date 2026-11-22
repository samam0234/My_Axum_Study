//! `comments` 테이블 엔티티.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "comments")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    /// 어느 글의 댓글인가. 글이 지워지면 댓글도 함께 지워진다. (FK `ON DELETE CASCADE`)
    pub post_id: i32,

    pub author_id: i32,

    #[sea_orm(column_type = "Text")]
    pub body: String,

    pub created_at: ChronoDateTimeUtc,
}

/// "댓글 하나는 글 하나, 회원 하나에 속한다."
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::post::Entity",
        from = "Column::PostId",
        to = "super::post::Column::Id",
        on_delete = "Cascade"
    )]
    Post,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::AuthorId",
        to = "super::user::Column::Id",
        on_delete = "Cascade"
    )]
    Author,
}

impl Related<super::post::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Post.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Author.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
