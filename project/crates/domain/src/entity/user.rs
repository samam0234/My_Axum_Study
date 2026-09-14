//! `users` 테이블 엔티티.
//!
//! sqlx에서는 `#[derive(FromRow)]`로 "쿼리 결과를 담는 구조체"를 만들었다.
//! SeaORM의 엔티티는 **테이블 정의 그 자체**다. 컬럼, 기본키, 관계가 전부 타입에 들어 있고
//! 여기서 쿼리 빌더와 관계 탐색이 파생된다.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    #[sea_orm(unique)]
    pub username: String,

    #[sea_orm(unique)]
    pub email: String,

    /// 응답에 절대 나가면 안 되는 필드.
    /// `#[serde(skip_serializing)]`로 직렬화 대상에서 제외한다.
    #[serde(skip_serializing)]
    pub password_hash: String,

    pub created_at: ChronoDateTimeUtc,
}

/// 관계 정의. "User 하나가 Post 여럿을 가진다."
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::post::Entity")]
    Post,
}

impl Related<super::post::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Post.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
