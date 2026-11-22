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
//! 이 파일은 **공통 API 스펙 v1**(경로 `/api`, `{ data, meta }`, 오류 `{ error: { code, … } }`)을
//! 그대로 시험한다. 스펙에서 어긋나면 이 테스트가 먼저 깨진다.

// 테스트는 in-memory SQLite를 쓰므로 sqlite feature(기본값)에서만 컴파일한다.
#![cfg(feature = "sqlite")]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use demo::{app, config::Config, db, state::AppState};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt; // oneshot

/// 매번 새 in-memory DB로 앱을 만든다.
async fn test_app() -> Router {
    test_app_with(|state| state).await
}

/// 상태를 조금 바꿔서 앱을 만든다. (예: 호출 제한을 켠다)
async fn test_app_with(tweak: impl FnOnce(AppState) -> AppState) -> Router {
    let config = Config::for_test();
    let pool = db::init(&config.database_url)
        .await
        .expect("테스트 DB 초기화 실패");

    app(tweak(AppState::new(
        pool,
        config.jwt_secret,
        config.upload_dir,
    )))
}

/// 요청 하나를 보내고 (상태 코드, 헤더, JSON 본문)을 돌려준다.
async fn request_full(
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

    // Router는 Clone이 저렴하다. oneshot은 Service를 소비하므로 복제해서 넘긴다.
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

/// 헤더가 필요 없을 때의 줄임 버전.
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let (status, _, json) = request_full(app, method, uri, body, token).await;
    (status, json)
}

/// 회원가입 + 로그인을 거쳐 토큰을 얻는다. 이메일은 `<nickname>@example.com`.
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

/// 글을 하나 쓰고 id를 돌려준다.
async fn create_post(app: &Router, token: &str, title: &str, body: &str) -> i64 {
    let (status, created) = request(
        app,
        "POST",
        "/api/posts",
        Some(json!({ "title": title, "body": body })),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    created["data"]["id"].as_i64().unwrap()
}

// ---------------------------------------------------------------------------
// 공통: 헬스·오류 모양
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_체크는_접두어_없이_200을_돌려준다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/health", None, None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn 없는_경로는_통일된_404_json을_돌려준다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/api/nope", None, None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn 접두어_없는_옛_경로는_더_이상_없다() {
    let app = test_app().await;
    for uri in ["/posts", "/auth/me"] {
        let (status, body) = request(&app, "GET", uri, None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(body["error"]["code"], "NOT_FOUND");
    }
}

#[tokio::test]
async fn 메서드가_다르면_405다() {
    let app = test_app().await;
    let (status, body) = request(&app, "DELETE", "/health", None, None).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "METHOD_NOT_ALLOWED");
}

#[tokio::test]
async fn 패닉해도_500_json이_나간다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/panic", None, None).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "INTERNAL");
}

#[tokio::test]
async fn 오류_본문의_request_id는_응답_헤더와_같다() {
    let app = test_app().await;

    let request = Request::builder()
        .uri("/nope")
        .header("x-request-id", "test-req-123")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();

    assert_eq!(response.headers()["x-request-id"], "test-req-123");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"]["requestId"], "test-req-123");
}

// ---------------------------------------------------------------------------
// 인증 (§1 auth, §2 본문, §3 로그인 응답, §5 오류 코드)
// ---------------------------------------------------------------------------

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
    // 이메일은 소문자로 맞춰 저장한다
    assert_eq!(body["data"]["email"], "mina@example.com");
    assert!(body["data"]["id"].is_number());
    assert!(body["data"]["createdAt"].is_string());
    let text = body.to_string();
    assert!(!text.contains("password"), "응답에 비밀번호 관련 필드가 있다: {text}");
}

#[tokio::test]
async fn 로그인_응답은_data_user_access_token이다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    assert!(token.starts_with("eyJ"), "JWT 모양이 아니다");

    let (_, body) = request(
        &app,
        "POST",
        "/api/auth/login",
        // 대소문자가 달라도 같은 계정이다
        Some(json!({ "email": "ALICE@example.com", "password": "supersecret123" })),
        None,
    )
    .await;

    assert_eq!(body["data"]["user"]["email"], "alice@example.com");
    assert_eq!(body["data"]["user"]["nickname"], "alice");
    assert!(body["data"]["accessToken"].is_string());
    // 스펙에 없는 필드는 내보내지 않는다
    assert!(body["data"].get("tokenType").is_none());
}

#[tokio::test]
async fn me는_data로_내_정보를_준다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, body) = request(&app, "GET", "/api/auth/me", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["nickname"], "alice");

    let (status, body) = request(&app, "GET", "/api/auth/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn 로그아웃은_인증이_필요하고_204다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, _) = request(&app, "POST", "/api/auth/logout", None, Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = request(&app, "POST", "/api/auth/logout", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn 중복_이메일은_409_email_taken이다() {
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
}

#[tokio::test]
async fn 중복_닉네임은_이메일_중복과_구분되는_409다() {
    let app = test_app().await;
    let _ = signup_and_login(&app, "alice").await;

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
async fn 가입_검증_실패는_fields에_필드별_메시지를_준다() {
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
}

#[tokio::test]
async fn 로그인_실패_메시지는_이메일_존재_여부를_알려주지_않는다() {
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

    assert_eq!(s1, StatusCode::UNAUTHORIZED);
    assert_eq!(s2, StatusCode::UNAUTHORIZED);
    assert_eq!(b1["error"]["code"], "INVALID_CREDENTIALS");
    // 두 경우의 응답이 완전히 같아야 한다 (user enumeration 방지).
    // requestId는 요청마다 달라지므로 비교에서 뺀다.
    assert_eq!(b1["error"]["message"], b2["error"]["message"]);
    assert_eq!(b1["error"]["code"], b2["error"]["code"]);
}

// ---------------------------------------------------------------------------
// 게시글 (§1 posts, §2 본문, §3 응답, §4 쿼리)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn 토큰_없이는_글을_쓸_수_없다() {
    let app = test_app().await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/posts",
        Some(json!({ "title": "무단 작성", "body": "본문" })),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn 게시글_crud_전체_흐름() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    // 생성: 201 + Location 헤더 + { data }
    let (status, headers, created) = request_full(
        &app,
        "POST",
        "/api/posts",
        Some(json!({ "title": "첫 글", "body": "본문입니다", "tags": ["rust"] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let post = &created["data"];
    assert_eq!(post["title"], "첫 글");
    assert_eq!(post["body"], "본문입니다");
    assert_eq!(post["tags"], json!(["rust"]));
    // 작성자는 요청 본문이 아니라 토큰에서 나오고, 객체 { id, nickname }이다
    assert_eq!(post["author"]["nickname"], "alice");
    assert!(post["author"]["id"].is_number());
    assert_eq!(post["commentCount"], 0);

    let id = post["id"].as_i64().unwrap();
    assert_eq!(headers[header::LOCATION], format!("/api/posts/{id}").as_str());

    // 조회
    let (status, fetched) = request(&app, "GET", &format!("/api/posts/{id}"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["data"]["body"], "본문입니다");

    // 수정 (부분)
    let (status, updated) = request(
        &app,
        "PATCH",
        &format!("/api/posts/{id}"),
        Some(json!({ "title": "수정된 글" })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["data"]["title"], "수정된 글");
    assert_eq!(updated["data"]["body"], "본문입니다"); // 안 보낸 필드는 그대로

    // 삭제
    let (status, _, body) = request_full(
        &app,
        "DELETE",
        &format!("/api/posts/{id}"),
        None,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null(), "204는 본문이 없어야 한다");

    // 삭제 후 조회하면 404
    let (status, _) = request(&app, "GET", &format!("/api/posts/{id}"), None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 남의_글은_수정할_수_없다() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;
    let id = create_post(&app, &alice, "앨리스의 글", "본문").await;

    let (status, body) = request(
        &app,
        "PATCH",
        &format!("/api/posts/{id}"),
        Some(json!({ "title": "탈취 시도" })),
        Some(&bob),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "FORBIDDEN");
}

#[tokio::test]
async fn 글_검증_실패는_400과_필드별_fields를_준다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, body) = request(
        &app,
        "POST",
        "/api/posts",
        Some(json!({ "title": "", "body": "" })),
        Some(&token),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
    assert!(body["error"]["fields"]["title"].is_array());
    assert!(body["error"]["fields"]["body"].is_array());
}

#[tokio::test]
async fn 숫자가_아닌_id는_400_bad_request다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/api/posts/abc", None, None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn 목록은_data와_meta를_준다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    for i in 1..=7 {
        create_post(&app, &token, &format!("글 {i}"), &format!("본문 {i}")).await;
    }

    let (status, page1) = request(&app, "GET", "/api/posts?page=1&limit=3", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page1["data"].as_array().unwrap().len(), 3);
    assert_eq!(page1["meta"]["page"], 1);
    assert_eq!(page1["meta"]["limit"], 3);
    assert_eq!(page1["meta"]["total"], 7);
    assert_eq!(page1["meta"]["totalPages"], 3);
    assert!(page1["meta"]["q"].is_null());
    // 기본 정렬은 -createdAt: 가장 늦게 쓴 글이 먼저
    assert_eq!(page1["data"][0]["title"], "글 7");
    // 목록 항목에도 author 객체와 commentCount가 있다
    assert_eq!(page1["data"][0]["author"]["nickname"], "alice");
    assert_eq!(page1["data"][0]["commentCount"], 0);

    let (_, page3) = request(&app, "GET", "/api/posts?page=3&limit=3", None, None).await;
    assert_eq!(page3["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn 검색어는_meta_q로_돌아오고_대소문자를_무시한다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    create_post(&app, &token, "Axum 입문", "라우터").await;
    create_post(&app, &token, "다른 글", "AXUM 이야기").await; // 본문에서도 찾는다
    create_post(&app, &token, "무관한 글", "내용").await;

    let (_, body) = request(&app, "GET", "/api/posts?q=axum", None, None).await;
    assert_eq!(body["meta"]["total"], 2);
    assert_eq!(body["meta"]["q"], "axum");
}

#[tokio::test]
async fn limit이_50을_넘으면_오류가_아니라_50으로_줄인다() {
    let app = test_app().await;
    let (status, body) = request(&app, "GET", "/api/posts?limit=1000", None, None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["meta"]["limit"], 50);
}

#[tokio::test]
async fn 잘못된_page_limit_sort는_400이다() {
    let app = test_app().await;
    for (query, field) in [("page=0", "page"), ("limit=0", "limit"), ("sort=title", "sort")] {
        let (status, body) = request(&app, "GET", &format!("/api/posts?{query}"), None, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR", "{query}");
        assert!(body["error"]["fields"][field].is_array(), "{query}");
    }
}

#[tokio::test]
async fn sort로_오래된_순과_댓글_많은_순을_고를_수_있다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    let first = create_post(&app, &token, "첫째", "본문").await;
    let _second = create_post(&app, &token, "둘째", "본문").await;
    let third = create_post(&app, &token, "셋째", "본문").await;

    // createdAt: 오름차순
    let (_, asc) = request(&app, "GET", "/api/posts?sort=createdAt", None, None).await;
    assert_eq!(asc["data"][0]["title"], "첫째");

    // -commentCount: 댓글이 많은 글이 먼저. 첫째에 2개, 셋째에 1개
    for (post, text) in [(first, "a"), (first, "b"), (third, "c")] {
        let (status, _) = request(
            &app,
            "POST",
            &format!("/api/posts/{post}/comments"),
            Some(json!({ "body": text })),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (_, popular) = request(&app, "GET", "/api/posts?sort=-commentCount", None, None).await;
    let titles: Vec<&str> = popular["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["첫째", "셋째", "둘째"]);
    assert_eq!(popular["data"][0]["commentCount"], 2);
}

#[tokio::test]
async fn sql_인젝션_시도는_그냥_검색어로_취급된다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    create_post(&app, &token, "정상 글", "본문").await;

    let (status, body) =
        request(&app, "GET", "/api/posts?q=%27%20OR%201%3D1%20--", None, None).await;

    assert_eq!(status, StatusCode::OK);
    // 전체가 쏟아지지 않고 0건이어야 한다
    assert_eq!(body["meta"]["total"], 0);
}

#[tokio::test]
async fn 응답_json은_camel_case다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    let id = create_post(&app, &token, "camel", "본문").await;

    let (_, body) = request(&app, "GET", &format!("/api/posts/{id}"), None, None).await;
    let post = &body["data"];

    assert!(post["createdAt"].is_string());
    assert!(post["updatedAt"].is_string());
    assert!(post["commentCount"].is_number());
    // snake_case 키가 남아 있으면 안 된다
    for key in ["created_at", "updated_at", "comment_count", "author_id"] {
        assert!(post.get(key).is_none(), "{key}");
    }
}

// ---------------------------------------------------------------------------
// 댓글 (§1 comments)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn 댓글_작성_목록_삭제_흐름() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;
    let post_id = create_post(&app, &alice, "글", "본문").await;

    // 비로그인은 쓸 수 없다
    let (status, _) = request(
        &app,
        "POST",
        &format!("/api/posts/{post_id}/comments"),
        Some(json!({ "body": "익명" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 작성: 201 + { data }
    let (status, headers, created) = request_full(
        &app,
        "POST",
        &format!("/api/posts/{post_id}/comments"),
        Some(json!({ "body": "첫 댓글" })),
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        headers[header::LOCATION],
        format!("/api/posts/{post_id}/comments").as_str()
    );
    assert_eq!(created["data"]["body"], "첫 댓글");
    assert_eq!(created["data"]["author"]["nickname"], "bob");
    assert_eq!(created["data"]["postId"], post_id);
    let comment_id = created["data"]["id"].as_i64().unwrap();

    // 목록: 로그인 없이 볼 수 있고 { data: [...] }
    let (status, list) = request(
        &app,
        "GET",
        &format!("/api/posts/{post_id}/comments"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"].as_array().unwrap().len(), 1);

    // 글의 commentCount가 따라 올라간다
    let (_, post) = request(&app, "GET", &format!("/api/posts/{post_id}"), None, None).await;
    assert_eq!(post["data"]["commentCount"], 1);

    // 남의 댓글은 못 지운다 (글 주인이어도)
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

    // 작성자는 지울 수 있다
    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/api/comments/{comment_id}"),
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/api/comments/{comment_id}"),
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn 없는_글의_댓글은_404고_빈_본문은_400이다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, body) = request(&app, "GET", "/api/posts/999/comments", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "NOT_FOUND");

    let post_id = create_post(&app, &token, "글", "본문").await;
    let (status, body) = request(
        &app,
        "POST",
        &format!("/api/posts/{post_id}/comments"),
        Some(json!({ "body": "" })),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"]["fields"]["body"].is_array());
}

#[tokio::test]
async fn 글을_지우면_댓글도_함께_사라진다() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;
    let post_id = create_post(&app, &token, "글", "본문").await;

    let (_, created) = request(
        &app,
        "POST",
        &format!("/api/posts/{post_id}/comments"),
        Some(json!({ "body": "댓글" })),
        Some(&token),
    )
    .await;
    let comment_id = created["data"]["id"].as_i64().unwrap();

    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/api/posts/{post_id}"),
        None,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // ON DELETE CASCADE: 댓글만 남아 있으면 안 된다
    let (status, _) = request(
        &app,
        "DELETE",
        &format!("/api/comments/{comment_id}"),
        None,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 호출 제한 (§5 429), CORS (§7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn 호출_제한을_넘으면_429_rate_limited와_retry_after를_준다() {
    let app = test_app_with(|s| s.with_rate_limit_per_minute(3)).await;

    for _ in 0..3 {
        let (status, _) = request(&app, "GET", "/api/posts", None, None).await;
        assert_eq!(status, StatusCode::OK);
    }

    let (status, headers, body) = request_full(&app, "GET", "/api/posts", None, None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "RATE_LIMITED");
    assert!(body["error"]["requestId"].is_string());
    let retry_after: u64 = headers[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
    assert!((1..=60).contains(&retry_after));

    // 헬스 체크는 제한 대상이 아니다. (배포 점검이 제한에 걸리면 안 된다)
    let (status, _) = request(&app, "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn cors_출처는_설정값을_따른다() {
    let app = test_app_with(|s| s.with_cors_origin("https://front.example.com, http://localhost:5173".into())).await;

    for origin in ["https://front.example.com", "http://localhost:5173"] {
        let request = Request::builder()
            .method("OPTIONS")
            .uri("/api/posts")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            origin,
            "{origin}"
        );
    }

    // 목록에 없는 출처에는 허용 헤더를 주지 않는다
    let request = Request::builder()
        .method("OPTIONS")
        .uri("/api/posts")
        .header(header::ORIGIN, "https://evil.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_ne!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).map(|v| v.to_str().unwrap()),
        Some("https://evil.example.com")
    );
}
