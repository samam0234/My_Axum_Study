//! 댓글 서비스.
//!
//! 글 서비스와 마찬가지로 **소유권 검사를 서비스 안에서** 한다. (`delete`의 `actor_id`)

use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, ModelTrait, QueryFilter,
    QueryOrder, Set,
};

use super::post;
use crate::entity::{comment, user};
use crate::error::{DomainError, DomainResult};

#[derive(Debug, Clone)]
pub struct CommentView {
    pub id: i32,
    pub post_id: i32,
    pub body: String,
    pub author_id: i32,
    pub author_nickname: String,
    pub created_at: DateTime<Utc>,
}

impl CommentView {
    fn from_pair(comment: comment::Model, author: Option<user::Model>) -> Self {
        Self {
            id: comment.id,
            post_id: comment.post_id,
            body: comment.body,
            author_id: comment.author_id,
            author_nickname: author.map(|u| u.nickname).unwrap_or_else(|| "(삭제됨)".into()),
            created_at: comment.created_at,
        }
    }
}

/// 글의 댓글을 오래된 순으로. 글이 없으면 "댓글 0개"가 아니라 `NotFound`다.
pub async fn list(db: &DatabaseConnection, post_id: i32) -> DomainResult<Vec<CommentView>> {
    post::ensure_exists(db, post_id).await?;

    let rows = comment::Entity::find()
        .filter(comment::Column::PostId.eq(post_id))
        .find_also_related(user::Entity)
        .order_by_asc(comment::Column::CreatedAt)
        .order_by_asc(comment::Column::Id)
        .all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(c, u)| CommentView::from_pair(c, u))
        .collect())
}

pub async fn create(
    db: &DatabaseConnection,
    post_id: i32,
    author_id: i32,
    body: &str,
) -> DomainResult<CommentView> {
    post::ensure_exists(db, post_id).await?;

    let created = comment::ActiveModel {
        post_id: Set(post_id),
        author_id: Set(author_id),
        body: Set(body.to_string()),
        created_at: Set(Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await?;

    find(db, created.id).await
}

pub async fn find(db: &DatabaseConnection, id: i32) -> DomainResult<CommentView> {
    comment::Entity::find_by_id(id)
        .find_also_related(user::Entity)
        .one(db)
        .await?
        .map(|(c, u)| CommentView::from_pair(c, u))
        .ok_or(DomainError::NotFound {
            resource: "댓글",
            id,
        })
}

pub async fn delete(db: &DatabaseConnection, id: i32, actor_id: i32) -> DomainResult<()> {
    let found = comment::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DomainError::NotFound {
            resource: "댓글",
            id,
        })?;

    if found.author_id != actor_id {
        return Err(DomainError::Forbidden(
            "본인이 작성한 댓글만 삭제할 수 있습니다".to_string(),
        ));
    }

    found.delete(db).await?;
    Ok(())
}
