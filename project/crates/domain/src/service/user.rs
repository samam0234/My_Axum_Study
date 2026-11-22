//! 회원 서비스.
//!
//! demo/의 `repo` + `handler` 일부가 여기로 합쳐졌다.
//! 차이: **HTTP를 전혀 모른다.** 상태 코드도, `Json`도, extractor도 없다.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set,
};

use crate::entity::user;
use crate::error::{DomainError, DomainResult};
use crate::password;

/// 이메일은 대소문자를 구분하지 않는 것이 사용자 기대에 맞다. 저장도 조회도 이 함수를 거친다.
///
/// HTTP 계층이 아니라 **도메인이** 정규화한다. 그래야 어떤 진입점(웹, CLI, 배치)으로 와도 같은 규칙이다.
fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// 회원가입.
pub async fn register(
    db: &DatabaseConnection,
    nickname: &str,
    email: &str,
    raw_password: String,
) -> DomainResult<user::Model> {
    let email = normalize_email(email);
    let nickname = nickname.trim();

    // 어느 쪽이 겹쳤는지 알려 주려고 먼저 확인한다. (오류 메시지의 문자열을 해석하는 것보다
    // DB 종류에 상관없이 확실하다.) 확인과 INSERT 사이에 끼어든 동시 가입은 아래의
    // UNIQUE 제약이 막는다.
    if user::Entity::find()
        .filter(user::Column::Email.eq(email.as_str()))
        .count(db)
        .await?
        > 0
    {
        return Err(DomainError::EmailTaken);
    }
    if user::Entity::find()
        .filter(user::Column::Nickname.eq(nickname))
        .count(db)
        .await?
        > 0
    {
        return Err(DomainError::Conflict("이미 사용 중인 닉네임입니다".to_string()));
    }

    let password_hash = password::hash(raw_password).await?;

    let model = user::ActiveModel {
        nickname: Set(nickname.to_string()),
        email: Set(email),
        password_hash: Set(password_hash),
        created_at: Set(Utc::now()),
        ..Default::default() // id는 DB가 채운다
    };

    model.insert(db).await.map_err(map_unique_violation)
}

/// 로그인. 성공하면 사용자 모델을 돌려준다.
pub async fn authenticate(
    db: &DatabaseConnection,
    email: &str,
    raw_password: String,
) -> DomainResult<user::Model> {
    let found = user::Entity::find()
        .filter(user::Column::Email.eq(normalize_email(email)))
        .one(db)
        .await?;

    // ⚠️ "없는 이메일"과 "틀린 비밀번호"를 구분해서 알려주지 않는다. (user enumeration 방지)
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
    //
    // 여기까지 왔다는 것은 위의 사전 확인을 통과한 뒤 **동시에 같은 값으로 가입한** 경우다.
    // 어느 쪽인지는 알 수 없으므로 일반적인 충돌로 알린다.
    if matches!(
        err.sql_err(),
        Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
    ) {
        DomainError::Conflict("이미 사용 중인 이메일 또는 닉네임입니다".to_string())
    } else {
        DomainError::Database(err)
    }
}
