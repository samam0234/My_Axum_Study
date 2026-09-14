//! 게시글 서비스.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, ModelTrait,
    PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::Serialize;

use crate::entity::{post, user};
use crate::error::{DomainError, DomainResult};

/// 작성자 이름까지 합친 조회용 뷰 모델.
///
/// 엔티티(`post::Model`)에는 `author_id`만 있다. 화면에는 이름이 필요하므로
/// **조인 결과를 담을 별도 타입**을 둔다. sqlx에서 `PostRow` → `Post`를 나눈 것과 같은 이유.
#[derive(Debug, Clone, Serialize)]
pub struct PostView {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub author_id: i32,
    pub author_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl PostView {
    fn from_pair(post: post::Model, author: Option<user::Model>) -> Self {
        Self {
            id: post.id,
            title: post.title,
            body: post.body,
            author_id: post.author_id,
            author_name: author.map(|u| u.username).unwrap_or_else(|| "(삭제됨)".into()),
            created_at: post.created_at,
            updated_at: post.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PostPage {
    pub items: Vec<PostView>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub total_pages: u64,
}

/// 목록 조회 (검색 + 페이지네이션).
///
/// demo/에서는 `QueryBuilder`로 SQL 조각을 이어 붙였다.
/// SeaORM에서는 **`Condition`으로 조건 트리를 조립**한다. SQL 문자열이 아예 등장하지 않는다.
pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    per_page: u64,
    q: Option<&str>,
) -> DomainResult<PostPage> {
    let mut condition = Condition::all();
    if let Some(keyword) = q.map(str::trim).filter(|s| !s.is_empty()) {
        condition = condition.add(
            Condition::any()
                .add(post::Column::Title.contains(keyword))
                .add(post::Column::Body.contains(keyword)),
        );
    }

    let paginator = post::Entity::find()
        .filter(condition)
        .find_also_related(user::Entity) // LEFT JOIN users
        .order_by_desc(post::Column::Id)
        .paginate(db, per_page.max(1));

    let total = paginator.num_items().await?;
    let total_pages = paginator.num_pages().await?;

    // paginate는 0-based라 1-based 페이지 번호에서 1을 뺀다.
    let rows = paginator.fetch_page(page.saturating_sub(1)).await?;

    Ok(PostPage {
        items: rows
            .into_iter()
            .map(|(post, author)| PostView::from_pair(post, author))
            .collect(),
        page,
        per_page,
        total,
        total_pages,
    })
}

pub async fn find(db: &DatabaseConnection, id: i32) -> DomainResult<PostView> {
    let row = post::Entity::find_by_id(id)
        .find_also_related(user::Entity)
        .one(db)
        .await?;

    let (post, author) = row.ok_or(DomainError::NotFound {
        resource: "게시글",
        id,
    })?;

    Ok(PostView::from_pair(post, author))
}

pub async fn create(
    db: &DatabaseConnection,
    author_id: i32,
    title: &str,
    body: &str,
) -> DomainResult<PostView> {
    let now = Utc::now();
    let created = post::ActiveModel {
        title: Set(title.to_string()),
        body: Set(body.to_string()),
        author_id: Set(author_id),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    find(db, created.id).await
}

/// 수정. **소유권 검사를 서비스 안에서** 한다.
///
/// demo/에서는 핸들러가 `ensure_owner`를 부르고 있었다. 여기서는 도메인이 규칙을 지킨다.
/// → HTTP 계층에서 실수로 검사를 빠뜨리는 일이 생길 수 없다.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    actor_id: i32,
    title: Option<&str>,
    body: Option<&str>,
) -> DomainResult<PostView> {
    let found = post::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DomainError::NotFound {
            resource: "게시글",
            id,
        })?;

    if found.author_id != actor_id {
        return Err(DomainError::Forbidden(
            "본인이 작성한 글만 수정할 수 있습니다".to_string(),
        ));
    }

    let mut active: post::ActiveModel = found.into();
    if let Some(t) = title {
        active.title = Set(t.to_string());
    }
    if let Some(b) = body {
        active.body = Set(b.to_string());
    }
    active.updated_at = Set(Utc::now());

    let saved = active.update(db).await?;
    find(db, saved.id).await
}

pub async fn delete(db: &DatabaseConnection, id: i32, actor_id: i32) -> DomainResult<()> {
    let found = post::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DomainError::NotFound {
            resource: "게시글",
            id,
        })?;

    if found.author_id != actor_id {
        return Err(DomainError::Forbidden(
            "본인이 작성한 글만 삭제할 수 있습니다".to_string(),
        ));
    }

    found.delete(db).await?;
    Ok(())
}
