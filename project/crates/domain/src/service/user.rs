//! 회원 서비스.
//!
//! demo/의 `repo` + `handler` 일부가 여기로 합쳐졌다.
//! 차이: **HTTP를 전혀 모른다.** 상태 코드도, `Json`도, extractor도 없다.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};

use crate::entity::user;
use crate::error::{DomainError, DomainResult};
use crate::password;

/// 회원가입.
pub async fn register(
    db: &DatabaseConnection,
    username: &str,
    email: &str,
    raw_password: String,
) -> DomainResult<user::Model> {
    let password_hash = password::hash(raw_password).await?;

    let now = Utc::now();
    let model = user::ActiveModel {
        username: Set(username.to_string()),
        email: Set(email.to_string()),
        password_hash: Set(password_hash),
        created_at: Set(now),
        ..Default::default() // id는 DB가 채운다
    };

    // UNIQUE 위반을 409로 번역할 수 있게 도메인 오류로 바꾼다.
    // (먼저 SELECT로 확인하는 방식은 동시 요청에서 경합이 생긴다)
    model.insert(db).await.map_err(map_unique_violation)
}

/// 로그인. 성공하면 사용자 모델을 돌려준다.
pub async fn authenticate(
    db: &DatabaseConnection,
    username: &str,
    raw_password: String,
) -> DomainResult<user::Model> {
    let found = user::Entity::find()
        .filter(user::Column::Username.eq(username))
        .one(db)
        .await?;

    // ⚠️ "없는 아이디"와 "틀린 비밀번호"를 구분해서 알려주지 않는다. (user enumeration 방지)
    let Some(user) = found else {
        return Err(DomainError::InvalidCredentials);
    };

    if password::verify(raw_password, user.password_hash.clone()).await? {
        Ok(user)
    } else {
        Err(DomainError::InvalidCredentials)
    }
}

pub async fn find(db: &DatabaseConnection, id: i32) -> DomainResult<user::Model> {
    user::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(DomainError::NotFound {
            resource: "회원",
            id,
        })
}

fn map_unique_violation(err: sea_orm::DbErr) -> DomainError {
    // `sql_err()`는 DB 종류(SQLite/PostgreSQL/MySQL)에 상관없이 UNIQUE 위반을 알려 준다.
    // 예전에는 메시지에 "UNIQUE constraint failed"가 있는지 문자열로 판단했는데,
    // PostgreSQL의 메시지는 "duplicate key value violates unique constraint"라서
    // DB를 바꾸는 순간 409가 500으로 바뀌었다. (study11)
    if matches!(
        err.sql_err(),
        Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
    ) {
        DomainError::Conflict("이미 사용 중인 아이디 또는 이메일입니다".to_string())
    } else {
        DomainError::Database(err)
    }
}
