//! 게시글 서비스.
//!
//! 이 파일의 타입(`PostView`, `PostPage`)에는 **`Serialize`가 없다.** JSON 모양(camelCase, `author` 객체,
//! `{ data, meta }` 껍데기)은 HTTP 표현의 일이라 web 크레이트의 `dto.rs`가 정한다.
//! 도메인은 "글이 무엇인가"만 안다.

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Asterisk, Expr, Func, Query, SimpleExpr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, FromQueryResult,
    JoinType, ModelTrait, Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    RelationTrait, Select, Set,
};

use crate::entity::{comment, post, user};
use crate::error::{DomainError, DomainResult};

/// 작성자 이름과 댓글 수까지 합친 조회용 뷰 모델.
///
/// 엔티티(`post::Model`)에는 `author_id`만 있다. 화면에는 이름과 댓글 수가 필요하므로
/// **조인·집계 결과를 담을 별도 타입**을 둔다. sqlx에서 `PostRow` → `Post`를 나눈 것과 같은 이유.
#[derive(Debug, Clone)]
pub struct PostView {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub author_id: i32,
    pub author_nickname: String,
    pub comment_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// SELECT 한 행. 컬럼 이름이 쿼리의 별칭과 같아야 한다.
#[derive(Debug, FromQueryResult)]
struct PostRow {
    id: i32,
    title: String,
    body: String,
    tags: String,
    author_id: i32,
    author_nickname: Option<String>,
    comment_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PostRow> for PostView {
    fn from(row: PostRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            body: row.body,
            // 저장된 JSON이 깨져 있어도 500을 내지 않고 빈 배열로 처리한다.
            tags: serde_json::from_str(&row.tags).unwrap_or_default(),
            author_id: row.author_id,
            author_nickname: row.author_nickname.unwrap_or_else(|| "(삭제됨)".into()),
            comment_count: row.comment_count,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug)]
pub struct PostPage {
    pub items: Vec<PostView>,
    pub page: u64,
    pub limit: u64,
    pub total: u64,
    pub total_pages: u64,
}

/// 정렬 기준. 문자열을 SQL에 붙이지 않고 **enum으로 고른다.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    CreatedAtDesc,
    CreatedAtAsc,
    CommentCountDesc,
}

impl Sort {
    /// 스펙의 `sort` 파라미터 값(`createdAt`, `-createdAt`, `-commentCount`)을 해석한다.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "-createdAt" => Some(Self::CreatedAtDesc),
            "createdAt" => Some(Self::CreatedAtAsc),
            "-commentCount" => Some(Self::CommentCountDesc),
            _ => None,
        }
    }
}

/// 글 목록의 한 페이지를 읽는 SELECT. 목록과 단건이 **같은 열**을 읽도록 한 곳에 둔다.
///
/// - `LEFT JOIN users`: 작성자 닉네임
/// - 서브쿼리 `(SELECT COUNT(*) FROM comments WHERE post_id = posts.id)`: 댓글 수
fn select_view() -> Select<post::Entity> {
    let comment_count: SimpleExpr = SimpleExpr::SubQuery(
        None,
        Box::new(
            Query::select()
                .expr(Func::count(Expr::col(Asterisk)))
                .from(comment::Entity)
                .and_where(
                    Expr::col((comment::Entity, comment::Column::PostId))
                        .equals((post::Entity, post::Column::Id)),
                )
                .to_owned()
                .into_sub_query_statement(),
        ),
    );

    post::Entity::find()
        .select_only()
        .columns([
            post::Column::Id,
            post::Column::Title,
            post::Column::Body,
            post::Column::Tags,
            post::Column::AuthorId,
            post::Column::CreatedAt,
            post::Column::UpdatedAt,
        ])
        .column_as(user::Column::Nickname, "author_nickname")
        .column_as(comment_count, "comment_count")
        .join(JoinType::LeftJoin, post::Relation::Author.def())
}

/// 목록 조회 (검색 + 정렬 + 페이지네이션).
///
/// demo/에서는 `QueryBuilder`로 SQL 조각을 이어 붙였다.
/// SeaORM에서는 **`Condition`으로 조건 트리를 조립**한다. SQL 문자열이 아예 등장하지 않는다.
///
/// `limit`는 호출하는 쪽이 정한 상한을 지켜 넘겨야 한다. (스펙은 최대 50이고 web이 줄여서 넘긴다)
pub async fn list(
    db: &DatabaseConnection,
    page: u64,
    limit: u64,
    q: Option<&str>,
    sort: Sort,
) -> DomainResult<PostPage> {
    let limit = limit.max(1);
    let page = page.max(1);

    let mut condition = Condition::all();
    if let Some(keyword) = q.map(str::trim).filter(|s| !s.is_empty()) {
        // `contains()`는 그냥 `LIKE '%..%'`라서 SQLite에서는 대소문자를 무시하지만
        // PostgreSQL에서는 구분한다. DB를 바꿔도 같은 결과가 나오도록 양쪽을 소문자로 맞춘다.
        // (PostgreSQL 전용 `ILIKE`는 SQLite에서 쓸 수 없다)
        let pattern = format!("%{}%", keyword.to_lowercase());
        condition = condition.add(
            Condition::any()
                .add(Expr::expr(Func::lower(Expr::col(post::Column::Title))).like(pattern.clone()))
                .add(Expr::expr(Func::lower(Expr::col(post::Column::Body))).like(pattern)),
        );
    }

    let total = post::Entity::find().filter(condition.clone()).count(db).await?;
    let total_pages = total.div_ceil(limit);

    let mut query = select_view().filter(condition);
    query = match sort {
        Sort::CreatedAtDesc => query
            .order_by_desc(post::Column::CreatedAt)
            .order_by_desc(post::Column::Id),
        Sort::CreatedAtAsc => query
            .order_by_asc(post::Column::CreatedAt)
            .order_by_asc(post::Column::Id),
        // 별칭(comment_count)으로 정렬하고, 같으면 최신순
        Sort::CommentCountDesc => query
            .order_by(Expr::cust("comment_count"), Order::Desc)
            .order_by_desc(post::Column::CreatedAt)
            .order_by_desc(post::Column::Id),
    };

    let rows = query
        .limit(limit)
        .offset((page - 1) * limit)
        .into_model::<PostRow>()
        .all(db)
        .await?;

    Ok(PostPage {
        items: rows.into_iter().map(PostView::from).collect(),
        page,
        limit,
        total,
        total_pages,
    })
}

pub async fn find(db: &DatabaseConnection, id: i32) -> DomainResult<PostView> {
    select_view()
        .filter(post::Column::Id.eq(id))
        .into_model::<PostRow>()
        .one(db)
        .await?
        .map(PostView::from)
        .ok_or(DomainError::NotFound {
            resource: "게시글",
            id,
        })
}

/// 글이 있는지만 확인한다. (댓글 서비스가 "없는 글"을 404로 구분할 때 쓴다)
pub async fn ensure_exists(db: &DatabaseConnection, id: i32) -> DomainResult<()> {
    let found = post::Entity::find_by_id(id).count(db).await? > 0;
    if found {
        Ok(())
    } else {
        Err(DomainError::NotFound {
            resource: "게시글",
            id,
        })
    }
}

pub async fn create(
    db: &DatabaseConnection,
    author_id: i32,
    title: &str,
    body: &str,
    tags: &[String],
) -> DomainResult<PostView> {
    let now = Utc::now();
    let created = post::ActiveModel {
        title: Set(title.to_string()),
        body: Set(body.to_string()),
        tags: Set(encode_tags(tags)),
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
    tags: Option<&[String]>,
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
    if let Some(t) = tags {
        active.tags = Set(encode_tags(t));
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

    // 댓글은 FK의 ON DELETE CASCADE가 함께 지운다.
    found.delete(db).await?;
    Ok(())
}

fn encode_tags(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}
