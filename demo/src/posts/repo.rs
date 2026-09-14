//! 게시글 DB 접근 계층.
//!
//! Spring의 `JpaRepository`, Nest의 `Repository<Post>`에 해당한다.
//! 다만 sqlx는 ORM이 아니라서 **SQL을 직접 쓴다.** 대신 무슨 쿼리가 나가는지 100% 명확하다.

use chrono::{DateTime, Utc};

use super::model::{CreatePost, PostRow, UpdatePost};
use crate::db::DbPool;

/// 최신순 전체 목록
pub async fn list(db: &DbPool) -> Result<Vec<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(
        r#"
        SELECT id, title, content, author, tags, created_at, updated_at
        FROM posts
        ORDER BY id DESC
        "#,
    )
    .fetch_all(db)
    .await
}

/// 단건 조회. 없으면 `Ok(None)`.
pub async fn find(db: &DbPool, id: i64) -> Result<Option<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(
        r#"
        SELECT id, title, content, author, tags, created_at, updated_at
        FROM posts
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// 생성. `RETURNING`으로 INSERT와 조회를 한 번에 끝낸다.
pub async fn insert(db: &DbPool, input: &CreatePost) -> Result<PostRow, sqlx::Error> {
    let now: DateTime<Utc> = Utc::now();
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());

    sqlx::query_as::<_, PostRow>(
        r#"
        INSERT INTO posts (title, content, author, tags, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id, title, content, author, tags, created_at, updated_at
        "#,
    )
    .bind(&input.title)
    .bind(&input.content)
    .bind(&input.author)
    .bind(tags)
    .bind(now)
    .bind(now)
    .fetch_one(db)
    .await
}

/// 부분 수정. 없으면 `Ok(None)`.
///
/// `COALESCE(?, column)`은 "바인딩 값이 NULL이면 기존 컬럼 값을 쓴다"는 뜻이다.
/// 덕분에 필드 조합마다 SQL을 따로 만들지 않아도 된다.
pub async fn update(
    db: &DbPool,
    id: i64,
    input: &UpdatePost,
) -> Result<Option<PostRow>, sqlx::Error> {
    let tags = input
        .tags
        .as_ref()
        .map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".to_string()));

    sqlx::query_as::<_, PostRow>(
        r#"
        UPDATE posts
        SET title      = COALESCE(?, title),
            content    = COALESCE(?, content),
            tags       = COALESCE(?, tags),
            updated_at = ?
        WHERE id = ?
        RETURNING id, title, content, author, tags, created_at, updated_at
        "#,
    )
    .bind(input.title.as_ref())
    .bind(input.content.as_ref())
    .bind(tags)
    .bind(Utc::now())
    .bind(id)
    .fetch_optional(db)
    .await
}

/// 삭제. 실제로 지워진 행이 있으면 `true`.
pub async fn delete(db: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM posts WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;

    Ok(result.rows_affected() > 0)
}
