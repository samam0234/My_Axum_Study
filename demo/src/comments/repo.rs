//! 댓글 DB 접근 계층.

use chrono::Utc;

use super::model::CommentRow;
use crate::db::DbPool;

/// 댓글 한 건을 읽는 SELECT. 작성자 닉네임은 `users`에서 가져온다. (댓글은 회원만 쓸 수 있어서 INNER JOIN)
macro_rules! select_comment {
    () => {
        "SELECT c.id, c.post_id, c.body, c.author_id, u.nickname AS author_nickname, c.created_at \
         FROM comments c JOIN users u ON u.id = c.author_id"
    };
}

/// 글의 댓글을 오래된 순으로. 댓글은 한 글에 붙는 양이 작다고 보고 페이지를 나누지 않는다.
pub async fn list_by_post(db: &DbPool, post_id: i64) -> Result<Vec<CommentRow>, sqlx::Error> {
    sqlx::query_as::<_, CommentRow>(concat!(
        select_comment!(),
        " WHERE c.post_id = $1 ORDER BY c.created_at ASC, c.id ASC"
    ))
    .bind(post_id)
    .fetch_all(db)
    .await
}

pub async fn find(db: &DbPool, id: i64) -> Result<Option<CommentRow>, sqlx::Error> {
    sqlx::query_as::<_, CommentRow>(concat!(select_comment!(), " WHERE c.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 생성. INSERT로 id를 받고, 닉네임이 붙은 모양으로 다시 읽어 돌려준다.
pub async fn insert(
    db: &DbPool,
    post_id: i64,
    author_id: i64,
    body: &str,
) -> Result<CommentRow, sqlx::Error> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO comments (post_id, author_id, body, created_at) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(post_id)
    .bind(author_id)
    .bind(body)
    .bind(Utc::now())
    .fetch_one(db)
    .await?;

    find(db, id).await?.ok_or(sqlx::Error::RowNotFound)
}

pub async fn delete(db: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM comments WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(result.rows_affected() > 0)
}
