use chrono::Utc;

use super::model::UserRow;
use crate::db::DbPool;

pub async fn insert(
    db: &DbPool,
    username: &str,
    email: &str,
    password_hash: &str,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        INSERT INTO users (username, email, password_hash, created_at)
        VALUES (?, ?, ?, ?)
        RETURNING id, username, email, password_hash, created_at
        "#,
    )
    .bind(username)
    .bind(email)
    .bind(password_hash)
    .bind(Utc::now())
    .fetch_one(db)
    .await
}

pub async fn find_by_username(
    db: &DbPool,
    username: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, username, email, password_hash, created_at FROM users WHERE username = ?",
    )
    .bind(username)
    .fetch_optional(db)
    .await
}

pub async fn find_by_id(db: &DbPool, id: i64) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, username, email, password_hash, created_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}
