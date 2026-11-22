use chrono::Utc;

use super::model::UserRow;
use crate::db::DbPool;

pub async fn insert(
    db: &DbPool,
    nickname: &str,
    email: &str,
    password_hash: &str,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        INSERT INTO users (nickname, email, password_hash, created_at)
        VALUES ($1, $2, $3, $4)
        RETURNING id, nickname, email, password_hash, created_at
        "#,
    )
    .bind(nickname)
    .bind(email)
    .bind(password_hash)
    .bind(Utc::now())
    .fetch_one(db)
    .await
}

/// 로그인은 이메일로 한다. (이메일은 가입 때 소문자로 맞춰 저장하므로 호출하는 쪽도 소문자로 넘긴다)
pub async fn find_by_email(db: &DbPool, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, nickname, email, password_hash, created_at FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(db)
    .await
}

pub async fn find_by_id(db: &DbPool, id: i64) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, nickname, email, password_hash, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}
