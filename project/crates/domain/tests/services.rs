//! 도메인 서비스 통합 테스트.
//!
//! **HTTP가 없다.** 서비스 함수를 직접 부르므로 axum도 라우터도 필요 없다.
//! 도메인 크레이트가 HTTP를 모른다는 것의 보상이 바로 이것이다 — 가장 중요한 규칙
//! (소유권, 중복 가입, 검색)을 서버를 띄우지 않고 시험한다.
//!
//! DB는 이름 있는 in-memory SQLite(`cache=shared`)다. 풀의 모든 연결이 **같은** 메모리 DB를 보고,
//! 이름이 시험마다 달라서 서로 격리된다.

use std::sync::atomic::{AtomicU32, Ordering};

use domain::{
    DomainError, connect,
    service::{
        comment, post,
        post::Sort,
        user,
    },
};
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

async fn make_post(db: &DatabaseConnection, author: i32, title: &str) -> i32 {
    post::create(db, author, title, "본문", &[]).await.unwrap().id
}

#[tokio::test]
async fn 같은_이메일이나_닉네임으로_다시_가입하면_충돌이다() {
    let db = test_db().await;
    make_user(&db, "alice").await;

    // 닉네임 중복
    let by_name = user::register(&db, "alice", "other@example.com", "supersecret123".into()).await;
    assert!(matches!(by_name, Err(DomainError::Conflict(_))), "{by_name:?}");

    // 이메일 중복은 닉네임 중복과 구분된다 (스펙: EMAIL_TAKEN)
    let by_email = user::register(&db, "alice2", "alice@example.com", "supersecret123".into()).await;
    assert!(matches!(by_email, Err(DomainError::EmailTaken)), "{by_email:?}");

    // 대소문자만 다른 이메일도 같은 이메일이다
    let by_case = user::register(&db, "alice3", "ALICE@Example.com", "supersecret123".into()).await;
    assert!(matches!(by_case, Err(DomainError::EmailTaken)), "{by_case:?}");
}

#[tokio::test]
async fn 없는_이메일과_틀린_비밀번호는_같은_오류다() {
    let db = test_db().await;
    make_user(&db, "alice").await;

    let wrong_pw = user::authenticate(&db, "alice@example.com", "wrongpassword".into()).await;
    let no_user = user::authenticate(&db, "nobody@example.com", "wrongpassword".into()).await;

    assert!(matches!(wrong_pw, Err(DomainError::InvalidCredentials)));
    assert!(matches!(no_user, Err(DomainError::InvalidCredentials)));
    // 올바른 비밀번호는 통과 (이메일 대소문자는 상관없다)
    assert!(user::authenticate(&db, "Alice@EXAMPLE.com", "supersecret123".into()).await.is_ok());
}

#[tokio::test]
async fn 남의_글은_수정도_삭제도_못_한다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    let bob = make_user(&db, "bob").await;

    let created = post::create(&db, alice, "앨리스의 글", "본문", &[]).await.unwrap();

    let edit = post::update(&db, created.id, bob, Some("탈취"), None, None).await;
    assert!(matches!(edit, Err(DomainError::Forbidden(_))), "{edit:?}");

    let del = post::delete(&db, created.id, bob).await;
    assert!(matches!(del, Err(DomainError::Forbidden(_))), "{del:?}");

    // 글은 그대로
    assert_eq!(post::find(&db, created.id).await.unwrap().title, "앨리스의 글");

    // 주인은 된다. 안 보낸 필드(본문)는 유지된다
    let edited = post::update(&db, created.id, alice, Some("수정됨"), None, None).await.unwrap();
    assert_eq!(edited.title, "수정됨");
    assert_eq!(edited.body, "본문");
    post::delete(&db, created.id, alice).await.unwrap();
    assert!(matches!(post::find(&db, created.id).await, Err(DomainError::NotFound { .. })));
}

#[tokio::test]
async fn 글에는_태그와_작성자_닉네임이_붙는다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;

    let tags = vec!["rust".to_string(), "axum".to_string()];
    let created = post::create(&db, alice, "태그 글", "본문", &tags).await.unwrap();
    assert_eq!(created.tags, tags);
    assert_eq!(created.author_nickname, "alice");
    assert_eq!(created.comment_count, 0);

    // tags를 안 보내면 유지, 보내면 교체
    let kept = post::update(&db, created.id, alice, Some("제목만"), None, None).await.unwrap();
    assert_eq!(kept.tags, tags);
    let replaced =
        post::update(&db, created.id, alice, None, None, Some(&["new".to_string()])).await.unwrap();
    assert_eq!(replaced.tags, vec!["new".to_string()]);
}

#[tokio::test]
async fn 검색은_대소문자를_구분하지_않는다() {
    // PostgreSQL에서 contains()가 대소문자를 구분해 검색이 비던 문제(study11)의 회귀 시험.
    // SQLite에서도 같은 결과가 나와야 DB를 바꿔도 동작이 같다.
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    post::create(&db, alice, "SeaORM 가이드", "Entity와 Migration", &[]).await.unwrap();
    post::create(&db, alice, "다른 글", "관계 없는 본문", &[]).await.unwrap();

    for q in ["seaorm", "SEAORM", "SeaORM", "migration"] {
        let page = post::list(&db, 1, 10, Some(q), Sort::CreatedAtDesc).await.unwrap();
        assert_eq!(page.total, 1, "q={q}");
    }
    let none = post::list(&db, 1, 10, Some("없는단어"), Sort::CreatedAtDesc).await.unwrap();
    assert_eq!(none.total, 0);
    let all = post::list(&db, 1, 10, None, Sort::CreatedAtDesc).await.unwrap();
    assert_eq!(all.total, 2);
}

#[tokio::test]
async fn 페이지네이션은_1부터_세고_전체_수를_준다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    for i in 1..=7 {
        make_post(&db, alice, &format!("글 {i}")).await;
    }

    let p1 = post::list(&db, 1, 3, None, Sort::CreatedAtDesc).await.unwrap();
    assert_eq!((p1.total, p1.total_pages, p1.items.len()), (7, 3, 3));
    assert_eq!(p1.items[0].title, "글 7"); // 최신순

    let p3 = post::list(&db, 3, 3, None, Sort::CreatedAtDesc).await.unwrap();
    assert_eq!(p3.items.len(), 1);

    let oldest = post::list(&db, 1, 3, None, Sort::CreatedAtAsc).await.unwrap();
    assert_eq!(oldest.items[0].title, "글 1");
}

#[tokio::test]
async fn 댓글_수와_댓글_많은_순_정렬() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    let first = make_post(&db, alice, "첫째").await;
    let _second = make_post(&db, alice, "둘째").await;
    let third = make_post(&db, alice, "셋째").await;

    for (post_id, text) in [(first, "a"), (first, "b"), (third, "c")] {
        comment::create(&db, post_id, alice, text).await.unwrap();
    }

    let popular = post::list(&db, 1, 10, None, Sort::CommentCountDesc).await.unwrap();
    let titles: Vec<_> = popular.items.iter().map(|p| (p.title.as_str(), p.comment_count)).collect();
    assert_eq!(titles, [("첫째", 2), ("셋째", 1), ("둘째", 0)]);

    // 단건 조회의 댓글 수도 같다
    assert_eq!(post::find(&db, first).await.unwrap().comment_count, 2);
}

#[tokio::test]
async fn 댓글은_작성자만_지우고_글을_지우면_함께_사라진다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    let bob = make_user(&db, "bob").await;
    let post_id = make_post(&db, alice, "글").await;

    let made = comment::create(&db, post_id, bob, "밥의 댓글").await.unwrap();
    assert_eq!(made.author_nickname, "bob");

    // 글 주인이어도 남의 댓글은 못 지운다
    let forbidden = comment::delete(&db, made.id, alice).await;
    assert!(matches!(forbidden, Err(DomainError::Forbidden(_))), "{forbidden:?}");
    assert_eq!(comment::list(&db, post_id).await.unwrap().len(), 1);

    // 없는 글에는 댓글을 달 수도, 목록을 볼 수도 없다 (빈 목록과 구분)
    assert!(matches!(comment::create(&db, 999, bob, "x").await, Err(DomainError::NotFound { .. })));
    assert!(matches!(comment::list(&db, 999).await, Err(DomainError::NotFound { .. })));

    // 글을 지우면 FK cascade로 댓글도 사라진다
    post::delete(&db, post_id, alice).await.unwrap();
    assert!(matches!(comment::find(&db, made.id).await, Err(DomainError::NotFound { .. })));
}
