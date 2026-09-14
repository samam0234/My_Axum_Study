//! 통합 테스트.
//!
//! `tests/` 폴더의 각 파일은 **별도의 크레이트**로 컴파일되며, 라이브러리(`demo`)를
//! 외부 사용자처럼 가져다 쓴다. 즉 `pub`이 아닌 항목은 여기서 보이지 않는다.
//!
//! 핵심은 **서버를 띄우지 않는다**는 점이다.
//! `tower::ServiceExt::oneshot`으로 `Router`에 요청을 직접 밀어 넣는다.
//! - 포트 충돌이 없어서 테스트를 병렬로 돌릴 수 있다
//! - 네트워크 왕복이 없어 빠르다
//! - 매 테스트가 in-memory DB를 새로 만들어 완전히 격리된다
//!
//! Spring의 MockMvc, Nest의 Supertest에 해당한다.

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use demo::{app, config::Config, db, state::AppState};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt; // oneshot

/// 매번 새 in-memory DB로 앱을 만든다.
async fn test_app() -> Router {
    let config = Config::for_test();
    let pool = db::init(&config.database_url)
        .await
        .expect("테스트 DB 초기화 실패");

    app(AppState::new(
        pool,
        config.jwt_secret,
        config.upload_dir,
    ))
}

/// 요청 하나를 보내고 (상태 코드, JSON 본문)을 돌려준다.
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);

    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }

    let request = match body {
        Some(json) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };

    // Router는 Clone이 저렴하다. oneshot은 Service를 소비하므로 복제해서 넘긴다.
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };

    (status, json)
}

/// 회원가입 + 로그인을 거쳐 토큰을 얻는다.
async fn signup_and_login(app: &Router, username: &str) -> String {
    let (status, _) = request(
        app,
        "POST",
        "/auth/register",
        Some(json!({
            "username": username,
            "email": format!("{username}@example.com"),
            "password": "supersecret123",
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "회원가입 실패");

    let (status, body) = request(
        app,
        "POST",
        "/auth/login",
        Some(json!({ "username": username, "password": "supersecret123" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "로그인 실패");

    body["access_token"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_체크는_200을_돌려준다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/health", None, None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn 없는_경로는_통일된_404_json을_돌려준다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/nope", None, None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
    assert!(body["message"].is_string());
}

#[tokio::test]
async fn 메서드가_다르면_405다() {
    let app = test_app().await;
    let (status, body) = request(&app, "DELETE", "/health", None, None).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"], "method_not_allowed");
}

#[tokio::test]
async fn 패닉해도_500_json이_나간다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/panic", None, None).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "internal_error");
}

#[tokio::test]
async fn 토큰_없이는_글을_쓸_수_없다() {
    let app = test_app().await;
    let (status, body) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "title": "무단 작성", "content": "본문" })),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn 게시글_crud_전체_흐름() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    // 생성
    let (status, created) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "title": "첫 글", "content": "본문입니다", "tags": ["rust"] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["title"], "첫 글");
    // 작성자는 요청 본문이 아니라 토큰에서 나온다
    assert_eq!(created["author"], "alice");

    let id = created["id"].as_i64().unwrap();

    // 조회
    let (status, fetched) = request(&app, "GET", &format!("/posts/{id}"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["content"], "본문입니다");

    // 수정 (부분)
    let (status, updated) = request(
        &app,
        "PATCH",
        &format!("/posts/{id}"),
        Some(json!({ "title": "수정된 글" })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["title"], "수정된 글");
    assert_eq!(updated["content"], "본문입니다"); // 안 보낸 필드는 그대로

    // 삭제
    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/posts/{id}"),
        None,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 삭제 후 조회하면 404
    let (status, _) = request(&app, "GET", &format!("/posts/{id}"), None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 남의_글은_수정할_수_없다() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;

    let (_, created) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "title": "앨리스의 글", "content": "본문" })),
        Some(&alice),
    )
    .await;
    let id = created["id"].as_i64().unwrap();

    let (status, body) = request(
        &app,
        "PATCH",
        &format!("/posts/{id}"),
        Some(json!({ "title": "탈취 시도" })),
        Some(&bob),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "forbidden");
}

#[tokio::test]
async fn 검증_실패는_422와_필드별_상세를_준다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, body) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "title": "", "content": "" })),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "validation_failed");
    assert!(body["details"]["title"].is_array());
    assert!(body["details"]["content"].is_array());
}

#[tokio::test]
async fn 중복_가입은_409다() {
    let app = test_app().await;
    let _ = signup_and_login(&app, "alice").await;

    let (status, body) = request(
        &app,
        "POST",
        "/auth/register",
        Some(json!({
            "username": "alice",
            "email": "another@example.com",
            "password": "supersecret123",
        })),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "conflict");
}

#[tokio::test]
async fn 로그인_실패_메시지는_아이디_존재_여부를_알려주지_않는다() {
    let app = test_app().await;
    let _ = signup_and_login(&app, "alice").await;

    let (s1, b1) = request(
        &app,
        "POST",
        "/auth/login",
        Some(json!({ "username": "alice", "password": "wrongpassword" })),
        None,
    )
    .await;

    let (s2, b2) = request(
        &app,
        "POST",
        "/auth/login",
        Some(json!({ "username": "nobody", "password": "wrongpassword" })),
        None,
    )
    .await;

    assert_eq!(s1, StatusCode::UNAUTHORIZED);
    assert_eq!(s2, StatusCode::UNAUTHORIZED);
    // 두 경우의 응답이 완전히 같아야 한다 (user enumeration 방지)
    assert_eq!(b1, b2);
}

#[tokio::test]
async fn 페이지네이션이_동작한다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    for i in 1..=7 {
        let (status, _) = request(
            &app,
            "POST",
            "/posts",
            Some(json!({ "title": format!("글 {i}"), "content": format!("본문 {i}") })),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, page1) = request(&app, "GET", "/posts?page=1&per_page=3", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page1["total"], 7);
    assert_eq!(page1["total_pages"], 3);
    assert_eq!(page1["has_next"], true);
    assert_eq!(page1["items"].as_array().unwrap().len(), 3);

    let (_, page3) = request(&app, "GET", "/posts?page=3&per_page=3", None, None).await;
    assert_eq!(page3["items"].as_array().unwrap().len(), 1);
    assert_eq!(page3["has_next"], false);
}

#[tokio::test]
async fn sql_인젝션_시도는_그냥_검색어로_취급된다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (_, _) = request(
        &app,
        "POST",
        "/posts",
        Some(json!({ "title": "정상 글", "content": "본문" })),
        Some(&token),
    )
    .await;

    let (status, body) = request(&app, "GET", "/posts?q=%27%20OR%201%3D1%20--", None, None).await;

    assert_eq!(status, StatusCode::OK);
    // 전체가 쏟아지지 않고 0건이어야 한다
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn 페이지_파라미터_범위를_벗어나면_422다() {
    let app = test_app().await;

    let (status, body) = request(&app, "GET", "/posts?per_page=1000", None, None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["details"]["per_page"].is_array());
}
