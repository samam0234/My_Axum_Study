//! web 크레이트 통합 테스트 — **공통 API 스펙 v1**을 그대로 시험한다.
//!
//! demo/의 `tests/api.rs`와 같은 방식이다. 서버를 띄우지 않고 `tower::ServiceExt::oneshot`으로
//! 라우터에 요청을 직접 밀어 넣는다. 도메인 규칙(소유권, 검색, 충돌)은 `domain/tests/services.rs`가
//! 이미 시험하므로, 여기서는 **HTTP 표현**(경로·상태 코드·JSON 모양·헤더)에 집중한다.

use std::sync::atomic::{AtomicU32, Ordering};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt; // oneshot
use web::{app, state::AppState};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 시험마다 이름이 다른 in-memory SQLite를 쓴다. (풀의 모든 연결이 같은 DB를 보고, 시험끼리는 격리된다)
async fn test_app() -> Router {
    test_app_with(|state| state).await
}

async fn test_app_with(tweak: impl FnOnce(AppState) -> AppState) -> Router {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let db = domain::connect(&format!("sqlite:file:web_test_{n}?mode=memory&cache=shared"))
        .await
        .expect("테스트 DB 연결 실패");

    let state = tweak(AppState::new(
        db,
        "test-secret-test-secret-test-secret".to_string(),
        false,
    ));
    // cargo test의 작업 디렉터리는 이 크레이트 폴더다.
    app(state, "../../static")
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
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

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, json)
}

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let (status, _, json) = send(app, method, uri, body, token).await;
    (status, json)
}

async fn signup_and_login(app: &Router, nickname: &str) -> String {
    let (status, _) = request(
        app,
        "POST",
        "/api/auth/signup",
        Some(json!({
            "email": format!("{nickname}@example.com"),
            "password": "supersecret123",
            "nickname": nickname,
        })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "회원가입 실패");

    let (status, body) = request(
        app,
        "POST",
        "/api/auth/login",
        Some(json!({ "email": format!("{nickname}@example.com"), "password": "supersecret123" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "로그인 실패");
    body["data"]["accessToken"].as_str().unwrap().to_string()
}

async fn create_post(app: &Router, token: &str, title: &str) -> i64 {
    let (status, created) = request(
        app,
        "POST",
        "/api/posts",
        Some(json!({ "title": title, "body": "본문" })),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    created["data"]["id"].as_i64().unwrap()
}

// ── 공통: 헬스·오류 모양·SPA ─────────────────────────────────────────────

#[tokio::test]
async fn 헬스_체크는_접두어_없는_health다() {
    let app = test_app().await;
    let (status, _, body) = send(&app, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    // 예전의 /api/health는 없다
    let (status, body) = request(&app, "GET", "/api/health", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
}

#[tokio::test]
async fn api_아래_없는_경로는_html이_아니라_json_404다() {
    let app = test_app().await;
    let (status, headers, body) = send(&app, "GET", "/api/없는경로", None, None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    assert!(headers[header::CONTENT_TYPE].to_str().unwrap().contains("json"));
}

#[tokio::test]
async fn api_밖의_경로는_spa_진입점으로_200이다() {
    let app = test_app().await;
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/아무-화면").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        response.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
    .unwrap();
    assert!(html.contains("<html"));
}

#[tokio::test]
async fn 오류_본문의_request_id는_응답_헤더와_같다() {
    let app = test_app().await;
    let request = Request::builder()
        .uri("/api/nope")
        .header("x-request-id", "test-req-123")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();

    assert_eq!(response.headers()["x-request-id"], "test-req-123");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["requestId"], "test-req-123");
}

#[tokio::test]
async fn 메서드가_다르면_405_json이다() {
    let app = test_app().await;
    let (status, body) = request(&app, "DELETE", "/api/auth/me", None, None).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "METHOD_NOT_ALLOWED");
    assert!(body["error"]["requestId"].is_string());
}

#[tokio::test]
async fn 숫자가_아닌_id는_400_bad_request다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/api/posts/abc", None, None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "BAD_REQUEST");
    assert!(body["error"]["requestId"].is_string());
}

#[tokio::test]
async fn 깨진_json은_400_bad_request다() {
    let app = test_app().await;
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["code"], "BAD_REQUEST");
}

// ── 인증 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn 가입_응답은_data로_감싸고_비밀번호가_없다() {
    let app = test_app().await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/auth/signup",
        Some(json!({ "email": "Mina@Example.com", "password": "password123", "nickname": "mina" })),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["data"]["nickname"], "mina");
    assert_eq!(body["data"]["email"], "mina@example.com"); // 소문자로 맞춰 저장
    assert!(body["data"]["createdAt"].is_string());
    assert!(!body.to_string().contains("password"), "{body}");
}

#[tokio::test]
async fn 로그인은_본문에_access_token을_주고_쿠키도_굽는다() {
    let app = test_app().await;
    let _ = signup_and_login(&app, "alice").await;

    let (status, headers, body) = send(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({ "email": "ALICE@example.com", "password": "supersecret123" })),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["user"]["nickname"], "alice");
    let token = body["data"]["accessToken"].as_str().unwrap();
    assert!(token.starts_with("eyJ"));

    // 같은 도메인 풀스택이라 HttpOnly 쿠키도 굽는다. 본문 형식은 쿠키가 없어도 같다.
    let cookie = headers[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("session="), "{cookie}");
    assert!(cookie.contains("HttpOnly"));

    // 쿠키만으로도 인증된다
    let session = cookie.split(';').next().unwrap().to_string();
    let request = Request::builder()
        .uri("/api/auth/me")
        .header(header::COOKIE, session)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn me와_로그아웃은_인증이_필요하다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, body) = request(&app, "GET", "/api/auth/me", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["nickname"], "alice");

    let (status, headers, _) = send(&app, "POST", "/api/auth/logout", None, Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));

    for (method, uri) in [("GET", "/api/auth/me"), ("POST", "/api/auth/logout")] {
        let (status, body) = request(&app, method, uri, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
    }
}

#[tokio::test]
async fn 중복_이메일과_중복_닉네임은_다른_409다() {
    let app = test_app().await;
    let _ = signup_and_login(&app, "alice").await;

    let (status, body) = request(
        &app,
        "POST",
        "/api/auth/signup",
        Some(json!({ "email": "alice@example.com", "password": "supersecret123", "nickname": "other" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "EMAIL_TAKEN");

    let (status, body) = request(
        &app,
        "POST",
        "/api/auth/signup",
        Some(json!({ "email": "new@example.com", "password": "supersecret123", "nickname": "alice" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "CONFLICT");
}

#[tokio::test]
async fn 검증_실패는_400_validation_error와_fields다() {
    let app = test_app().await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/auth/signup",
        Some(json!({ "email": "not-an-email", "password": "short", "nickname": "x" })),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
    for field in ["email", "password", "nickname"] {
        assert!(body["error"]["fields"][field].is_array(), "{field}");
    }
    assert!(body["error"]["requestId"].is_string());
}

#[tokio::test]
async fn 로그인_실패는_이메일_존재_여부를_알려주지_않는다() {
    let app = test_app().await;
    let _ = signup_and_login(&app, "alice").await;

    let (s1, b1) = request(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({ "email": "alice@example.com", "password": "wrongpassword" })),
        None,
    )
    .await;
    let (s2, b2) = request(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({ "email": "nobody@example.com", "password": "wrongpassword" })),
        None,
    )
    .await;

    assert_eq!((s1, s2), (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED));
    assert_eq!(b1["error"]["code"], "INVALID_CREDENTIALS");
    assert_eq!(b1["error"]["message"], b2["error"]["message"]);
}

// ── 게시글 ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn 게시글_crud_응답은_스펙_모양이다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, headers, created) = send(
        &app,
        "POST",
        "/api/posts",
        Some(json!({ "title": "첫 글", "body": "안녕하세요", "tags": ["intro"] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let post = &created["data"];
    assert_eq!(post["body"], "안녕하세요");
    assert_eq!(post["tags"], json!(["intro"]));
    assert_eq!(post["author"]["nickname"], "alice");
    assert!(post["author"]["id"].is_number());
    assert_eq!(post["commentCount"], 0);
    assert!(post["createdAt"].is_string());
    for snake in ["created_at", "comment_count", "author_name", "author_id"] {
        assert!(post.get(snake).is_none(), "{snake}");
    }

    let id = post["id"].as_i64().unwrap();
    assert_eq!(headers[header::LOCATION], format!("/api/posts/{id}").as_str());

    let (_, fetched) = request(&app, "GET", &format!("/api/posts/{id}"), None, None).await;
    assert_eq!(fetched["data"]["title"], "첫 글");

    let (status, updated) = request(
        &app,
        "PATCH",
        &format!("/api/posts/{id}"),
        Some(json!({ "title": "수정" })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["data"]["title"], "수정");
    assert_eq!(updated["data"]["body"], "안녕하세요");

    let (status, _, body) =
        send(&app, "DELETE", &format!("/api/posts/{id}"), None, Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
}

#[tokio::test]
async fn 남의_글은_403이고_로그인_없이는_401이다() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;
    let id = create_post(&app, &alice, "앨리스의 글").await;

    let (status, body) = request(
        &app,
        "PATCH",
        &format!("/api/posts/{id}"),
        Some(json!({ "title": "탈취" })),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "FORBIDDEN");

    let (status, body) = request(
        &app,
        "POST",
        "/api/posts",
        Some(json!({ "title": "무단", "body": "본문" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn 목록은_data와_meta를_주고_limit은_50으로_줄인다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    for i in 1..=7 {
        create_post(&app, &token, &format!("글 {i}")).await;
    }

    let (_, page1) = request(&app, "GET", "/api/posts?page=1&limit=3", None, None).await;
    assert_eq!(page1["data"].as_array().unwrap().len(), 3);
    assert_eq!(page1["data"][0]["title"], "글 7"); // 기본 정렬 -createdAt
    assert_eq!(page1["meta"]["total"], 7);
    assert_eq!(page1["meta"]["totalPages"], 3);
    assert_eq!(page1["meta"]["limit"], 3);
    assert!(page1["meta"]["q"].is_null());

    let (status, big) = request(&app, "GET", "/api/posts?limit=1000", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(big["meta"]["limit"], 50);

    let (_, asc) = request(&app, "GET", "/api/posts?sort=createdAt", None, None).await;
    assert_eq!(asc["data"][0]["title"], "글 1");
}

#[tokio::test]
async fn 검색어는_meta_q로_돌아오고_잘못된_값은_400이다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    create_post(&app, &token, "Axum 입문").await;
    create_post(&app, &token, "다른 글").await;

    let (_, body) = request(&app, "GET", "/api/posts?q=AXUM", None, None).await;
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["meta"]["q"], "AXUM");

    for (query, field) in [("page=0", "page"), ("limit=0", "limit"), ("sort=title", "sort")] {
        let (status, body) = request(&app, "GET", &format!("/api/posts?{query}"), None, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert!(body["error"]["fields"][field].is_array(), "{query}");
    }
}

// ── 댓글 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn 댓글_작성_목록_삭제와_comment_count() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;
    let post_id = create_post(&app, &alice, "글").await;
    let uri = format!("/api/posts/{post_id}/comments");

    let (status, _) = request(&app, "POST", &uri, Some(json!({ "body": "익명" })), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, headers, created) =
        send(&app, "POST", &uri, Some(json!({ "body": "첫 댓글" })), Some(&bob)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers[header::LOCATION], uri.as_str());
    assert_eq!(created["data"]["author"]["nickname"], "bob");
    assert_eq!(created["data"]["postId"], post_id);
    let comment_id = created["data"]["id"].as_i64().unwrap();

    let (status, list) = request(&app, "GET", &uri, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().unwrap().len(), 1);

    let (_, post) = request(&app, "GET", &format!("/api/posts/{post_id}"), None, None).await;
    assert_eq!(post["data"]["commentCount"], 1);

    // 글 주인이어도 남의 댓글은 못 지운다
    let (status, body) = request(
        &app,
        "DELETE",
        &format!("/api/comments/{comment_id}"),
        None,
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "FORBIDDEN");

    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/api/comments/{comment_id}"),
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn 없는_글의_댓글은_404고_popular_정렬이_된다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, body) = request(&app, "GET", "/api/posts/999/comments", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");

    let quiet = create_post(&app, &token, "조용한 글").await;
    let busy = create_post(&app, &token, "시끄러운 글").await;
    for text in ["a", "b"] {
        request(
            &app,
            "POST",
            &format!("/api/posts/{busy}/comments"),
            Some(json!({ "body": text })),
            Some(&token),
        )
        .await;
    }
    let _ = quiet;

    let (_, body) = request(&app, "GET", "/api/posts?sort=-commentCount", None, None).await;
    assert_eq!(body["data"][0]["title"], "시끄러운 글");
    assert_eq!(body["data"][0]["commentCount"], 2);
}

// ── 호출 제한 ───────────────────────────────────────────────────────────

#[tokio::test]
async fn 호출_제한을_넘으면_429_rate_limited와_retry_after를_준다() {
    let app = test_app_with(|s| s.with_rate_limit_per_minute(3)).await;

    for _ in 0..3 {
        let (status, _) = request(&app, "GET", "/api/posts", None, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, headers, body) = send(&app, "GET", "/api/posts", None, None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "RATE_LIMITED");
    assert!(body["error"]["requestId"].is_string());
    let retry: u64 = headers[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry));

    // 헬스 체크는 제한 대상이 아니다
    let (status, _) = request(&app, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
}
