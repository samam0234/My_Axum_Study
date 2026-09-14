//! 게시글 DB 접근 계층.
//!
//! Spring의 `JpaRepository`, Nest의 `Repository<Post>`에 해당한다.
//! 다만 sqlx는 ORM이 아니라서 **SQL을 직접 쓴다.** 대신 무슨 쿼리가 나가는지 100% 명확하다.
//!
//! ⚠️ sqlx 0.9부터 `query`/`query_as`는 `&'static str`(리터럴)만 받는다(`SqlSafeStr`).
//!    `format!`으로 SQL을 조립하면 컴파일 에러가 난다 — SQL 인젝션을 타입으로 막는 설계다.
//!    조건이 동적으로 바뀌는 쿼리는 `QueryBuilder`를 쓴다. (아래 `list_paged` 참고)

use chrono::{DateTime, Utc};
use sqlx::QueryBuilder;

use super::model::{CreatePost, PostRow, UpdatePost};
use crate::db::DbPool;
use crate::pagination::PageQuery;

/// 검색/필터 조건을 공통으로 붙이는 헬퍼.
///
/// `QueryBuilder`는 SQL 조각과 **바인딩 값**을 나눠서 쌓기 때문에,
/// 조건이 동적으로 변해도 값이 SQL 문자열에 섞여 들어가지 않는다.
fn push_filters(builder: &mut QueryBuilder<sqlx::Sqlite>, query: &PageQuery) {
    if let Some(q) = query.q.as_deref().filter(|s| !s.trim().is_empty()) {
        let pattern = format!("%{}%", q.trim());
        builder.push(" AND (title LIKE ");
        builder.push_bind(pattern.clone());
        builder.push(" OR content LIKE ");
        builder.push_bind(pattern);
        builder.push(")");
    }

    if let Some(tag) = query.tag.as_deref().filter(|s| !s.trim().is_empty()) {
        // tags는 '["rust","axum"]' 형태의 JSON 문자열이라 부분 문자열로 찾는다.
        // (정규화된 태그 테이블이 있다면 JOIN으로 바꾸는 게 정석)
        let needle = format!("%\"{}\"%", tag.trim());
        builder.push(" AND tags LIKE ");
        builder.push_bind(needle);
    }
}

/// 페이지 단위 목록 + 전체 건수
pub async fn list_paged(
    db: &DbPool,
    query: &PageQuery,
) -> Result<(Vec<PostRow>, i64), sqlx::Error> {
    // 1) 전체 건수
    let mut count_builder = QueryBuilder::<sqlx::Sqlite>::new(
        // `WHERE 1 = 1`은 뒤에 " AND ..."을 무조건 이어붙이기 위한 관용구다.
        "SELECT COUNT(*) FROM posts WHERE 1 = 1",
    );
    push_filters(&mut count_builder, query);
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(db)
        .await?;

    // 2) 해당 페이지의 행
    let mut builder = QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, title, content, author, author_id, tags, thumbnail, created_at, updated_at \
         FROM posts WHERE 1 = 1",
    );
    push_filters(&mut builder, query);
    builder.push(" ORDER BY id DESC LIMIT ");
    builder.push_bind(query.limit());
    builder.push(" OFFSET ");
    builder.push_bind(query.offset());

    let items = builder.build_query_as::<PostRow>().fetch_all(db).await?;

    Ok((items, total))
}

/// 단건 조회. 없으면 `Ok(None)`.
pub async fn find(db: &DbPool, id: i64) -> Result<Option<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(
        r#"
        SELECT id, title, content, author, author_id, tags, thumbnail, created_at, updated_at
        FROM posts
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// 생성. `RETURNING`으로 INSERT와 조회를 한 번에 끝낸다.
pub async fn insert(
    db: &DbPool,
    input: &CreatePost,
    author: &str,
    author_id: i64,
) -> Result<PostRow, sqlx::Error> {
    let now: DateTime<Utc> = Utc::now();
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());

    sqlx::query_as::<_, PostRow>(
        r#"
        INSERT INTO posts (title, content, author, author_id, tags, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        RETURNING id, title, content, author, author_id, tags, thumbnail, created_at, updated_at
        "#,
    )
    .bind(&input.title)
    .bind(&input.content)
    .bind(author)
    .bind(author_id)
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
        RETURNING id, title, content, author, author_id, tags, thumbnail, created_at, updated_at
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

/// 썸네일 파일명 갱신
pub async fn set_thumbnail(
    db: &DbPool,
    id: i64,
    filename: &str,
) -> Result<Option<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(
        r#"
        UPDATE posts
        SET thumbnail = ?, updated_at = ?
        WHERE id = ?
        RETURNING id, title, content, author, author_id, tags, thumbnail, created_at, updated_at
        "#,
    )
    .bind(filename)
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
