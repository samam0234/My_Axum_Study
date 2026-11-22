//! 도메인 서비스 통합 테스트.
//!
//! **HTTP가 없다.** 서비스 함수를 직접 부르므로 axum도 라우터도 필요 없다.
//! 도메인 크레이트가 HTTP를 모른다는 것의 보상이 바로 이것이다 — 가장 중요한 규칙
//! (소유권, 중복 가입, 검색)을 서버를 띄우지 않고 시험한다.
//!
//! DB는 이름 있는 in-memory SQLite(`cache=shared`)다. 풀의 모든 연결이 **같은** 메모리 DB를 보고,
//! 이름이 시험마다 달라서 서로 격리된다.

use std::sync::atomic::{AtomicU32, Ordering};

use domain::{DomainError, connect, service::{post, user}};
use sea_orm::DatabaseConnection;

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn test_db() -> DatabaseConnection {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    connect(&format!("sqlite:file:svc_test_{n}?mode=memory&cache=shared"))
        .await
        .expect("테스트 DB 연결 실패")
}

async fn make_user(db: &DatabaseConnection, name: &str) -> i32 {
    user::register(db, name, &format!("{name}@example.com"), "supersecret123".into())
        .await
        .expect("가입 실패")
        .id
}

#[tokio::test]
async fn 같은_아이디나_이메일로_다시_가입하면_충돌이다() {
    let db = test_db().await;
    make_user(&db, "alice").await;

    // 아이디 중복
    let by_name = user::register(&db, "alice", "other@example.com", "supersecret123".into()).await;
    assert!(matches!(by_name, Err(DomainError::Conflict(_))), "{by_name:?}");

    // 이메일 중복
    let by_email = user::register(&db, "alice2", "alice@example.com", "supersecret123".into()).await;
    assert!(matches!(by_email, Err(DomainError::Conflict(_))), "{by_email:?}");
}

#[tokio::test]
async fn 없는_아이디와_틀린_비밀번호는_같은_오류다() {
    let db = test_db().await;
    make_user(&db, "alice").await;

    let wrong_pw = user::authenticate(&db, "alice", "wrongpassword".into()).await;
    let no_user = user::authenticate(&db, "nobody", "wrongpassword".into()).await;

    assert!(matches!(wrong_pw, Err(DomainError::InvalidCredentials)));
    assert!(matches!(no_user, Err(DomainError::InvalidCredentials)));
    // 올바른 비밀번호는 통과
    assert!(user::authenticate(&db, "alice", "supersecret123".into()).await.is_ok());
}

#[tokio::test]
async fn 남의_글은_수정도_삭제도_못_한다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    let bob = make_user(&db, "bob").await;

    let created = post::create(&db, alice, "앨리스의 글", "본문").await.unwrap();

    let edit = post::update(&db, created.id, bob, Some("탈취"), None).await;
    assert!(matches!(edit, Err(DomainError::Forbidden(_))), "{edit:?}");

    let del = post::delete(&db, created.id, bob).await;
    assert!(matches!(del, Err(DomainError::Forbidden(_))), "{del:?}");

    // 글은 그대로
    assert_eq!(post::find(&db, created.id).await.unwrap().title, "앨리스의 글");

    // 주인은 된다. 안 보낸 필드(본문)는 유지된다
    let edited = post::update(&db, created.id, alice, Some("수정됨"), None).await.unwrap();
    assert_eq!(edited.title, "수정됨");
    assert_eq!(edited.body, "본문");
    post::delete(&db, created.id, alice).await.unwrap();
    assert!(matches!(post::find(&db, created.id).await, Err(DomainError::NotFound { .. })));
}

#[tokio::test]
async fn 검색은_대소문자를_구분하지_않는다() {
    // PostgreSQL에서 contains()가 대소문자를 구분해 검색이 비던 문제(study11)의 회귀 시험.
    // SQLite에서도 같은 결과가 나와야 DB를 바꿔도 동작이 같다.
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    post::create(&db, alice, "SeaORM 가이드", "Entity와 Migration").await.unwrap();
    post::create(&db, alice, "다른 글", "관계 없는 본문").await.unwrap();

    for q in ["seaorm", "SEAORM", "SeaORM", "migration"] {
        let page = post::list(&db, 1, 10, Some(q)).await.unwrap();
        assert_eq!(page.total, 1, "q={q}");
    }
    assert_eq!(post::list(&db, 1, 10, Some("없는단어")).await.unwrap().total, 0);
    assert_eq!(post::list(&db, 1, 10, None).await.unwrap().total, 2);
}

#[tokio::test]
async fn 페이지네이션은_1부터_세고_전체_수를_준다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    for i in 1..=7 {
        post::create(&db, alice, &format!("글 {i}"), "본문").await.unwrap();
    }

    let p1 = post::list(&db, 1, 3, None).await.unwrap();
    assert_eq!((p1.total, p1.total_pages, p1.items.len()), (7, 3, 3));
    assert_eq!(p1.items[0].title, "글 7"); // 최신순

    let p3 = post::list(&db, 3, 3, None).await.unwrap();
    assert_eq!(p3.items.len(), 1);
}
