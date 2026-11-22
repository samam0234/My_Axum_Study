//! study7 — 인증 · 보안 기초. 포트 3007.
//!
//! 한 단계에서 배우는 것만 담았다: Argon2 해시(+ spawn_blocking), JWT 발급·검증, `AuthUser` extractor
//! (인자에 적는 것이 곧 가드), 인증 401 vs 인가 403, 사용자 열거 방지, 이메일 정규화.
//! DB 대신 메모리(`Vec`)에 저장해서 인증 로직만 보이게 했다. (DB를 붙인 실제 모양은 demo/src/auth/)
//!
//! 실행:  cargo run --example s07_auth
//!        JWT_SECRET="길고-무작위인-비밀키-32자-이상" cargo run --example s07_auth

use std::sync::{Arc, RwLock};

use anyhow::Context;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    Json, Router,
    extract::{FromRef, FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    RequestPartsExt,
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, errors::ErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::json;

const TOKEN_TTL_SECONDS: i64 = 60 * 60; // 1시간 (공통 API 스펙 v1 §6)

// ── 상태: 사용자와 글을 메모리에 ─────────────────────────────────────────────
#[derive(Clone)]
struct AppState {
    users: Arc<RwLock<Vec<User>>>,
    posts: Arc<RwLock<Vec<Post>>>,
    jwt_secret: Arc<String>,
}

struct User {
    id: u64,
    email: String,
    nickname: String,
    password_hash: String, // Serialize를 구현하지 않는다 → 응답에 실수로 섞여 나갈 일이 구조적으로 없다
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")] // 응답 JSON은 camelCase (author_id → authorId)
struct Post {
    id: u64,
    title: String,
    author_id: u64,
}

#[derive(Serialize)]
struct UserPublic {
    id: u64,
    email: String,
    nickname: String, // 비밀번호 해시 필드가 아예 없다
}
impl From<&User> for UserPublic {
    fn from(u: &User) -> Self {
        Self { id: u.id, email: u.email.clone(), nickname: u.nickname.clone() }
    }
}

// ── 비밀번호: 평문이 아니라 Argon2 해시 ──────────────────────────────────────
async fn hash_password(password: String) -> anyhow::Result<String> {
    // Argon2는 일부러 느리다(수십 ms CPU). async 워커에서 그냥 돌리면 동시 로그인 몇 건에 서버 전체가 멈춘다
    tokio::task::spawn_blocking(move || {
        let hash: PasswordHash = Argon2::default() // argon2 0.6: 솔트를 알아서 만든다(1인자). 반환 타입이 제네릭이라 타입 주석 필요
            .hash_password(password.as_bytes())
            .map_err(|e| anyhow::anyhow!("해싱 실패: {e}"))?;
        Ok(hash.to_string()) // $argon2id$v=19$m=19456,t=2,p=1$<솔트>$<해시> — 알고리즘·파라미터·솔트가 한 문자열에
    })
    .await
    .context("해싱 태스크가 패닉했습니다")?
}

async fn verify_password(password: String, hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&hash).map_err(|e| anyhow::anyhow!("저장된 해시 형식 오류: {e}"))?;
        // 불일치는 "오류"가 아니라 false. Err는 해시가 깨진 진짜 오류용
        Ok(Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
    .context("검증 태스크가 패닉했습니다")?
}

// ── JWT ────────────────────────────────────────────────────────────────────
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: u64,        // subject — 사용자 id (JWT 표준 이름)
    nickname: String, // 커스텀 클레임. 페이로드는 암호화가 아니라 base64라서 누구나 읽는다 → 비밀을 넣지 않는다
    iat: i64,
    exp: i64, // 만료. Validation이 자동으로 검사해 준다
}

fn encode_token(secret: &str, user: &User) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims { sub: user.id, nickname: user.nickname.clone(), iat: now, exp: now + TOKEN_TTL_SECONDS };
    Ok(encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret.as_bytes()))?)
}

fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    // 허용 알고리즘을 명시한다. 토큰 헤더가 말하는 알고리즘을 그대로 믿으면 `alg: none` 공격이 가능해진다
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::new(Algorithm::HS256)).map(|d| d.claims)
}

// ── 오류 ───────────────────────────────────────────────────────────────────
#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("{0}")]
    Unauthorized(String), // 토큰 없음·만료·위조 → 401
    #[error("이메일 또는 비밀번호가 올바르지 않습니다")]
    InvalidCredentials, // 로그인 실패 → 401. 이메일/비밀번호 중 어느 쪽인지 숨긴다
    #[error("본인이 작성한 글만 수정/삭제할 수 있습니다")]
    Forbidden, // 인가 실패 → 403
    #[error("이미 사용 중인 이메일입니다")]
    EmailTaken, // → 409
    #[error("글 {0}을(를) 찾을 수 없습니다")]
    NotFound(u64),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::EmailTaken => (StatusCode::CONFLICT, "EMAIL_TAKEN"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL"),
        };
        let message = if let Self::Internal(e) = &self {
            eprintln!("internal error: {e:?}"); // 원인은 로그에만
            "서버 내부 오류가 발생했습니다.".to_string()
        } else {
            self.to_string()
        };
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}

// ── ⭐ AuthUser: 이게 Guard 자리다. 인자에 적는 것이 곧 보호 ─────────────────────
#[derive(Debug, Clone)]
struct AuthUser {
    id: u64,
    nickname: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>, // "상태 S에서 AppState를 꺼낼 수 있으면 된다"
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1) Authorization: Bearer <토큰>. 형식 검사를 TypedHeader에 맡긴다 (문자열을 직접 자르지 않는다)
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AppError::Unauthorized("Authorization 헤더에 Bearer 토큰이 필요합니다".into()))?;

        let app_state = AppState::from_ref(state);

        // 2) 서명 + 만료 검증. 만료와 위조를 다른 문구로 알려 프론트가 "다시 로그인"을 안내할 수 있다
        let claims = decode_token(&app_state.jwt_secret, bearer.token()).map_err(|e| match e.kind() {
            ErrorKind::ExpiredSignature => AppError::Unauthorized("토큰이 만료되었습니다".into()),
            _ => AppError::Unauthorized("토큰이 유효하지 않습니다".into()),
        })?;

        Ok(AuthUser { id: claims.sub, nickname: claims.nickname })
    }
}

// ── 핸들러 ─────────────────────────────────────────────────────────────────
#[derive(Deserialize)]
struct Signup {
    email: String,
    password: String,
    nickname: String,
}
#[derive(Deserialize)]
struct Login {
    email: String,
    password: String,
}
#[derive(Deserialize)]
struct NewPost {
    title: String,
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase() // 같은 사람이 대소문자만 바꿔 두 계정을 만들지 못하게
}

async fn signup(State(state): State<AppState>, Json(body): Json<Signup>) -> Result<impl IntoResponse, AppError> {
    let email = normalize_email(&body.email);
    if state.users.read().unwrap().iter().any(|u| u.email == email) {
        return Err(AppError::EmailTaken);
    }
    let password_hash = hash_password(body.password).await?;
    let mut users = state.users.write().unwrap();
    // (DB라면 SELECT 후 INSERT가 아니라 UNIQUE 제약 + 위반 변환으로 한다 — 동시 요청 사이의 틈 때문)
    let user = User { id: users.len() as u64 + 1, email, nickname: body.nickname, password_hash };
    let public = UserPublic::from(&user);
    users.push(user);
    Ok((StatusCode::CREATED, Json(json!({ "data": public }))))
}

async fn login(State(state): State<AppState>, Json(body): Json<Login>) -> Result<Json<serde_json::Value>, AppError> {
    let email = normalize_email(&body.email);
    let found = state.users.read().unwrap().iter().find(|u| u.email == email).map(|u| (u.id, u.nickname.clone(), u.password_hash.clone()));

    // ⚠️ "없는 이메일"과 "틀린 비밀번호"를 구분해서 알려 주면 안 된다. 공격자가 가입된 이메일 목록을 수집한다(user enumeration)
    let Some((id, nickname, hash)) = found else {
        return Err(AppError::InvalidCredentials);
    };
    if !verify_password(body.password, hash).await? {
        return Err(AppError::InvalidCredentials);
    }
    let user = User { id, email, nickname, password_hash: String::new() };
    let token = encode_token(&state.jwt_secret, &user)?;
    Ok(Json(json!({ "data": { "user": UserPublic::from(&user), "accessToken": token } })))
}

/// 인자에 `AuthUser`가 있다 → 이 라우트는 보호된다. 실패하면 핸들러는 아예 호출되지 않는다.
async fn me(user: AuthUser) -> Json<serde_json::Value> {
    Json(json!({ "data": { "id": user.id, "nickname": user.nickname } }))
}

async fn create_post(State(state): State<AppState>, user: AuthUser, Json(body): Json<NewPost>) -> impl IntoResponse {
    let mut posts = state.posts.write().unwrap();
    // 작성자는 요청 본문이 아니라 토큰에서. 클라이언트가 보낸 author를 믿지 않는다
    let post = Post { id: posts.len() as u64 + 1, title: body.title, author_id: user.id };
    posts.push(post.clone());
    (StatusCode::CREATED, Json(json!({ "data": post })))
}

/// 인증(누구인가)만으로는 부족하다. "이 글의 주인인가"는 데이터를 읽어야 알 수 있어서 핸들러에서 판단한다 → 403
fn ensure_owner(posts: &[Post], id: u64, user: &AuthUser) -> Result<(), AppError> {
    let post = posts.iter().find(|p| p.id == id).ok_or(AppError::NotFound(id))?; // 없으면 404
    if post.author_id != user.id {
        return Err(AppError::Forbidden); // 남의 글이면 403
    }
    Ok(())
}

async fn rename_post(State(state): State<AppState>, user: AuthUser, Path(id): Path<u64>, Json(body): Json<NewPost>) -> Result<Json<serde_json::Value>, AppError> {
    let mut posts = state.posts.write().unwrap();
    ensure_owner(&posts, id, &user)?;
    let post = posts.iter_mut().find(|p| p.id == id).unwrap();
    post.title = body.title;
    Ok(Json(json!({ "data": post })))
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(me))
        .route("/api/posts", post(create_post))
        .route("/api/posts/{id}", patch(rename_post))
        .with_state(state)
}

fn new_state(secret: &str) -> AppState {
    AppState { users: Default::default(), posts: Default::default(), jwt_secret: Arc::new(secret.to_string()) }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-only-insecure-secret-change-me".into());
    if secret.len() < 16 {
        anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)"); // 서버가 뜬 뒤가 아니라 시작할 때 거절
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3007").await?;
    println!("s07_auth  http://127.0.0.1:3007/api/auth/me");
    axum::serve(listener, app(new_state(&secret))).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn call(app: &Router, method: &str, uri: &str, body: Option<serde_json::Value>, token: Option<&str>) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let body = match body {
            Some(v) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    async fn signup_login(app: &Router, name: &str) -> String {
        call(app, "POST", "/api/auth/signup", Some(json!({"email":format!("{name}@example.com"),"password":"supersecret123","nickname":name})), None).await;
        let (_, b) = call(app, "POST", "/api/auth/login", Some(json!({"email":format!("{name}@example.com"),"password":"supersecret123"})), None).await;
        b["data"]["accessToken"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn 토큰이_없거나_위조면_401이고_있으면_통과한다() {
        let app = app(new_state("test-secret-test-secret"));
        let (s, b) = call(&app, "GET", "/api/auth/me", None, None).await;
        assert_eq!((s, &b["error"]["code"]), (StatusCode::UNAUTHORIZED, &json!("UNAUTHORIZED")));
        let (s, _) = call(&app, "GET", "/api/auth/me", None, Some("eyJhbGciOiJIUzI1NiJ9.e30.위조")).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);

        let token = signup_login(&app, "alice").await;
        let (s, b) = call(&app, "GET", "/api/auth/me", None, Some(&token)).await;
        assert_eq!((s, &b["data"]["nickname"]), (StatusCode::OK, &json!("alice")));
    }

    #[tokio::test]
    async fn 남의_글은_403이고_로그인_실패는_두_경우가_같다() {
        let app = app(new_state("test-secret-test-secret"));
        let alice = signup_login(&app, "alice").await;
        let bob = signup_login(&app, "bob").await;
        let (_, made) = call(&app, "POST", "/api/posts", Some(json!({"title":"앨리스의 글"})), Some(&alice)).await;
        let id = made["data"]["id"].as_u64().unwrap();

        let (s, b) = call(&app, "PATCH", &format!("/api/posts/{id}"), Some(json!({"title":"탈취"})), Some(&bob)).await;
        assert_eq!((s, &b["error"]["code"]), (StatusCode::FORBIDDEN, &json!("FORBIDDEN")));
        assert_eq!(call(&app, "PATCH", &format!("/api/posts/{id}"), Some(json!({"title":"수정"})), Some(&alice)).await.0, StatusCode::OK);

        let (s1, b1) = call(&app, "POST", "/api/auth/login", Some(json!({"email":"alice@example.com","password":"wrongwrong"})), None).await;
        let (s2, b2) = call(&app, "POST", "/api/auth/login", Some(json!({"email":"nobody@example.com","password":"wrongwrong"})), None).await;
        assert_eq!((s1, s2), (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED));
        assert_eq!(b1["error"], b2["error"], "두 응답이 다르면 가입된 이메일을 알아낼 수 있다");
    }

    #[tokio::test]
    async fn 이메일은_소문자로_맞추고_응답에_비밀번호가_없다() {
        let app = app(new_state("test-secret-test-secret"));
        let (s, b) = call(&app, "POST", "/api/auth/signup", Some(json!({"email":"Mina@Example.com","password":"supersecret123","nickname":"mina"})), None).await;
        assert_eq!((s, &b["data"]["email"]), (StatusCode::CREATED, &json!("mina@example.com")));
        assert!(!b.to_string().contains("password"));
        let (s, b) = call(&app, "POST", "/api/auth/signup", Some(json!({"email":"MINA@example.com","password":"supersecret123","nickname":"x"})), None).await;
        assert_eq!((s, &b["error"]["code"]), (StatusCode::CONFLICT, &json!("EMAIL_TAKEN")));
    }

    #[test]
    fn 토큰_페이로드는_암호화가_아니라_base64라서_읽힌다() {
        use jsonwebtoken::dangerous::insecure_decode;
        let user = User { id: 7, email: "a@b.c".into(), nickname: "neo".into(), password_hash: String::new() };
        let token = encode_token("test-secret-test-secret", &user).unwrap();
        // 비밀키 없이도 페이로드가 읽힌다 → 비밀을 넣지 않는다
        let claims = insecure_decode::<Claims>(&token).unwrap().claims;
        assert_eq!((claims.sub, claims.nickname.as_str(), claims.exp - claims.iat), (7, "neo", TOKEN_TTL_SECONDS));
        // 다른 키로는 검증이 실패한다 (서명이 맞아야 유효)
        assert!(decode_token("another-secret-another", &token).is_err());
    }
}
