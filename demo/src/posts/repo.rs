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
use crate::db::{Db, DbPool};
use crate::pagination::PageQuery;

/// 검색/필터 조건을 공통으로 붙이는 헬퍼.
///
/// `QueryBuilder`는 SQL 조각과 **바인딩 값**을 나눠서 쌓기 때문에,
/// 조건이 동적으로 변해도 값이 SQL 문자열에 섞여 들어가지 않는다.
///
/// `LOWER(col) LIKE LOWER(?)`로 쓰는 이유: SQLite의 `LIKE`는 대소문자를 무시하지만
/// PostgreSQL의 `LIKE`는 구분한다. 양쪽에서 같은 결과를 내려고 명시적으로 소문자로 맞춘다.
/// (`ILIKE`는 PostgreSQL 전용이라 SQLite에서 쓸 수 없다)
fn push_filters(builder: &mut QueryBuilder<Db>, query: &PageQuery) {
    if let Some(q) = query.q.as_deref().filter(|s| !s.trim().is_empty()) {
        let pattern = format!("%{}%", q.trim());
        builder.push(" AND (LOWER(title) LIKE LOWER(");
        builder.push_bind(pattern.clone());
        builder.push(") OR LOWER(content) LIKE LOWER(");
        builder.push_bind(pattern);
        builder.push("))");
    }

    if let Some(tag) = query.tag.as_deref().filter(|s| !s.trim().is_empty()) {
        // tags는 '["rust","axum"]' 형태의 JSON 문자열이라 부분 문자열로 찾는다.
        // (정규화된 태그 테이블이 있다면 JOIN으로 바꾸는 게 정석)
        let needle = format!("%\"{}\"%", tag.trim());
        builder.push(" AND LOWER(tags) LIKE LOWER(");
        builder.push_bind(needle);
        builder.push(")");
    }
}

/// 페이지 단위 목록 + 전체 건수
pub async fn list_paged(
    db: &DbPool,
    query: &PageQuery,
) -> Result<(Vec<PostRow>, i64), sqlx::Error> {
    // 1) 전체 건수
    let mut count_builder = QueryBuilder::<Db>::new(
        // `WHERE 1 = 1`은 뒤에 " AND ..."을 무조건 이어붙이기 위한 관용구다.
        "SELECT COUNT(*) FROM posts WHERE 1 = 1",
    );
    push_filters(&mut count_builder, query);
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(db)
        .await?;

    // 2) 해당 페이지의 행
    let mut builder = QueryBuilder::<Db>::new(
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
        WHERE id = $1
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
        VALUES ($1, $2, $3, $4, $5, $6, $7)
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
/// `COALESCE($1, column)`은 "바인딩 값이 NULL이면 기존 컬럼 값을 쓴다"는 뜻이다.
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
        SET title      = COALESCE($1, title),
            content    = COALESCE($2, content),
            tags       = COALESCE($3, tags),
            updated_at = $4
        WHERE id = $5
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
        SET thumbnail = $1, updated_at = $2
        WHERE id = $3
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
    let result = sqlx::query("DELETE FROM posts WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;

    Ok(result.rows_affected() > 0)
}
