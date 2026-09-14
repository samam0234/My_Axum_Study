//! 도메인 오류.
//!
//! ⭐ **여기에 `StatusCode`가 없다는 점이 이 프로젝트 구조의 핵심이다.**
//!
//! demo/의 `AppError`는 `IntoResponse`를 구현해서 HTTP에 묶여 있었다.
//! 여기서는 도메인이 "무엇이 잘못됐는가"만 말하고,
//! **그것을 몇 번 상태 코드로 바꿀지는 web 크레이트가 결정**한다.
//!
//! 덕분에 같은 서비스 코드를 CLI 도구, 배치 작업, gRPC 서버에서도 그대로 쓸 수 있다.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("{resource}을(를) 찾을 수 없습니다 (id: {id})")]
    NotFound { resource: &'static str, id: i32 },

    #[error("{0}")]
    Conflict(String),

    #[error("아이디 또는 비밀번호가 올바르지 않습니다")]
    InvalidCredentials,

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    Invalid(String),

    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub type DomainResult<T> = Result<T, DomainError>;
