# study7 — 인증 · 보안 기초

> 🎯 **목표**: 회원가입·로그인(비밀번호 해시 + JWT)을 만들고, 인자에 `AuthUser`를 적는 것만으로 라우트가 보호되게 하며, **인증(누구인가)과 인가(할 수 있는가)** 를 구분한다. CORS·XSS·CSRF의 기초와, 로그인 상태를 프론트와 백엔드가 공유하는 두 방식(Bearer 헤더 / HttpOnly 쿠키)을 이해한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | 단계 예제 [lab/examples/s07_auth.rs](lab/examples/s07_auth.rs)(포트 3007), 실제 프로젝트: [demo/src/auth/](demo/src/auth/) (`password.rs`, `jwt.rs`, `extractor.rs`, `handler.rs`), [demo/src/posts/handler.rs](demo/src/posts/handler.rs) (`ensure_owner`), 쿠키 방식: [project/crates/web/src/auth.rs](project/crates/web/src/auth.rs) |
| 검증 | `demo/tests/api.rs`의 인증·인가 테스트 (토큰 없이 쓰기 401, 남의 글 수정 403, 로그인 실패 메시지 동일, 로그아웃 204, 이메일 대소문자 무시) |
| 버전 주의 | **jsonwebtoken 11** (`rust_crypto` feature 필수), **argon2 0.6** (API 변경) |

---

## ① 목표

1. 비밀번호를 **평문이 아니라 Argon2 해시**로 저장하고, 해싱을 `spawn_blocking`으로 격리한다.
2. JWT를 발급·검증하고, 한계(즉시 무효화 불가)를 안다.
3. `AuthUser` extractor로 인증을, `ensure_owner`로 인가(소유권 403)를 구현한다.
4. CORS, XSS, CSRF가 **각각 무엇을 막는지** 구분한다.
5. 로그인 상태를 프론트와 공유하는 방법 두 가지의 장단점을 안다.

---

## ② 📖 이론

### 인증(Authentication) vs 인가(Authorization)

| | 질문 | 실패 시 | 어디서 |
|---|---|---|---|
| **인증** | "너는 **누구**냐?" | **401** Unauthorized | `AuthUser` extractor (토큰 검증) |
| **인가** | "너는 **이걸 해도 되냐?**" | **403** Forbidden | 핸들러/서비스 (DB를 읽어 소유자 비교) |

extractor는 "누구인지"까지만 답한다. "이 글의 주인인가"는 **DB를 읽어야 알 수 있어서** 미들웨어로 뺄 수 없다. 그래서 인가는 핸들러(또는 서비스 함수)에서 한다. (`project/`는 서비스 함수가 `actor_id`를 **필수 인자**로 받게 해서 검사를 빠뜨릴 수 없게 만들었다 — studyFinal)

### 비밀번호는 왜 해시인가

DB가 유출돼도 비밀번호가 새지 않게 하려면 **되돌릴 수 없는 변환**(해시)만 저장한다. 그리고 해시는 **일부러 느린** 알고리즘이어야 한다.

| 방식 | 평가 |
|---|---|
| 평문 | ❌ 유출 = 즉시 사고 |
| `sha256(pw)` | ❌ 너무 빠르다. GPU로 초당 수십억 번 대입 |
| **Argon2id** (+ 사용자별 솔트) | ✅ 일부러 느리고 메모리를 많이 쓴다. 같은 비밀번호도 솔트 때문에 해시가 다르다 |

저장 형태(PHC 문자열): `$argon2id$v=19$m=19456,t=2,p=1$<솔트>$<해시>` — **알고리즘·파라미터·솔트가 한 문자열에 들어 있어서** 검증 때 따로 보관할 것이 없다.

### 세션 vs 토큰(JWT)

| | 서버 세션 | **JWT** (이 저장소) |
|---|---|---|
| 상태 저장 | 서버가 세션 목록을 기억 | **서버는 기억하지 않는다.** 서명만 검증 |
| 즉시 로그아웃/차단 | 쉽다 (세션 삭제) | **어렵다** — 만료 시각까지 유효 |
| 서버 여러 대 | 세션 공유 저장소 필요 | 키만 같으면 된다 |

JWT = `헤더.페이로드.서명`. 페이로드(`sub`, `nickname`, `iat`, `exp`)는 **암호화가 아니라 base64**라서 누구나 읽을 수 있다 → **비밀을 넣지 않는다.** 서명이 맞아야만 유효하다.

### CORS · XSS · CSRF — 서로 다른 세 가지

| | 무엇을 | 막는 곳 | 요점 |
|---|---|---|---|
| **CORS** | 다른 **출처**의 JS가 내 API 응답을 **읽는 것** | 브라우저 (서버는 허용 목록을 알려 줌) | 서버 보안 장치가 아니다. `curl`은 CORS와 무관하다 |
| **XSS** | 공격자의 스크립트가 **내 페이지 안에서 실행**되는 것 | 출력 이스케이프, `textContent`, CSP, HttpOnly 쿠키 | 한 번 뚫리면 그 페이지의 모든 권한을 가진다 |
| **CSRF** | 로그인된 사용자의 브라우저가 **공격자 사이트에서** 내 서버로 요청을 보내게 하는 것 | `SameSite` 쿠키, CSRF 토큰, Bearer 헤더 방식 | 브라우저가 **쿠키를 자동으로** 실어 보내기 때문에 생기는 문제 |

---

## ③ 🛠️ 실기

### 3-0. 단계 예제 — [lab/examples/s07_auth.rs](lab/examples/s07_auth.rs) (포트 3007)

인증·인가의 **핵심만** 한 파일에 담았다. DB 대신 메모리(`Vec`)에 저장해서 Argon2 · JWT · `AuthUser` · 401/403만 보이게 했다. DB와 합쳐진 실제 모양은 아래 3-1부터의 `demo/src/auth/`다.

```powershell
cd lab
cargo run --example s07_auth
$env:JWT_SECRET = "길고-무작위인-비밀키-32자-이상"; cargo run --example s07_auth    # 짧으면(16자 미만) 시작을 거부한다
cargo test --example s07_auth                                                       # 시험 4개
```

```rust
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
```

시험 코드는 같은 파일 아래쪽의 `#[cfg(test)] mod tests`에 있다: [s07_auth.rs](lab/examples/s07_auth.rs).

| 코드 | 설명 |
|---|---|
| `hash_password` / `verify_password` + `spawn_blocking` | Argon2는 **일부러 느린**(수십 ms CPU) 알고리즘. async 워커에서 그냥 돌리면 동시 로그인 몇 건에 서버 전체가 멈춘다. argon2 0.6은 솔트를 알아서 만들고(1인자) 반환 타입이 제네릭이라 `let hash: PasswordHash` 주석이 필요 |
| PHC 문자열 `$argon2id$v=19$m=…$<솔트>$<해시>` | 알고리즘·파라미터·솔트가 한 문자열에 들어 있어 검증 때 따로 보관할 것이 없다 |
| `verify_password`가 불일치를 `false`로 | 불일치는 "오류"가 아니다. `Err`는 해시 형식이 깨진 진짜 오류용 |
| `User.password_hash`에 `Serialize` 없음 / `UserPublic` | 응답에 해시가 **구조적으로** 섞여 나갈 수 없다 |
| `Claims { sub, nickname, iat, exp }` | `sub`·`iat`·`exp`는 JWT 표준 이름이라 라이브러리가 만료를 자동 검사한다. 페이로드는 **암호화가 아니라 base64** → 비밀을 넣지 않는다 |
| `Validation::new(Algorithm::HS256)` | 허용 알고리즘을 **명시**. 토큰 헤더가 말하는 알고리즘을 믿으면 `alg: none` 공격이 가능 |
| `FromRequestParts for AuthUser` | 본문을 읽지 않는 extractor라서 인자 **위치가 자유**롭다. **핸들러 인자에 적는 것이 곧 가드**이고, 실패하면 핸들러는 아예 호출되지 않고 401이 나간다 |
| `AppState: FromRef<S>` | "상태 `S`에서 `AppState`를 꺼낼 수 있으면 된다" — 상태 타입이 하나면 기본 구현이 맞아떨어진다 |
| `TypedHeader<Authorization<Bearer>>` | `Bearer <토큰>` 형식 검사를 라이브러리에 맡긴다 (직접 문자열을 자르지 않는다) |
| `ErrorKind::ExpiredSignature` 분기 | 만료와 위조를 **다른 문구**로 알려 프론트가 "다시 로그인"을 안내할 수 있게 한다. 코드는 둘 다 `UNAUTHORIZED` |
| `login`이 두 경우 모두 `InvalidCredentials` | 없는 이메일과 틀린 비밀번호를 구분해서 알려 주면 가입된 이메일 목록이 새 나간다(user enumeration) |
| `normalize_email` | 같은 사람이 대소문자만 바꿔 두 계정을 만들지 못하게. 저장도 조회도 소문자로 |
| `ensure_owner` → `Forbidden` | **인증(누구인가, 401)과 인가(이걸 해도 되는가, 403)는 다른 문제.** extractor는 "누구인지"까지만 답한다. 주인인지는 데이터를 읽어야 알 수 있어서 핸들러에서 |
| `create_post`의 `author_id: user.id` | 작성자는 요청 본문이 아니라 **토큰에서**. 클라이언트가 보낸 값을 믿지 않는다 |
| `main`의 `JWT_SECRET.len() < 16` | 짧은 비밀키는 서버가 뜨기 전에 거부 (안전한 쪽으로 실패) |

**실행 결과** (직접 실행한 결과):

```
s07_auth  http://127.0.0.1:3007/api/auth/me

$ curl -X POST .../api/auth/signup -d '{"email":"Alice@Example.com","password":"supersecret123","nickname":"alice"}'
{"data":{"id":1,"email":"alice@example.com","nickname":"alice"}}            ← 이메일은 소문자로, 비밀번호 필드는 없다
$ ... {"email":"ALICE@example.com", ...}                                     ← 대소문자만 다른 같은 이메일
{"error":{"code":"EMAIL_TAKEN","message":"이미 사용 중인 이메일입니다"}}

$ 토큰 가운데 부분(페이로드)을 base64 디코드
{'sub': 1, 'nickname': 'alice', 'iat': 1795373835, 'exp': 1795377435}        ← 암호화가 아니라서 그대로 읽힌다 (exp - iat = 3600)
$ 토큰 첫 부분(헤더)                                          {'typ': 'JWT', 'alg': 'HS256'}

$ curl .../api/auth/me                                          → 401 {"error":{"code":"UNAUTHORIZED","message":"Authorization 헤더에 Bearer 토큰이 필요합니다"}}
$ curl .../api/auth/me -H "authorization: Bearer <토큰>x"        → 401 {"error":{"code":"UNAUTHORIZED","message":"토큰이 유효하지 않습니다"}}   ← 위조
$ curl .../api/auth/me -H "authorization: Bearer <토큰>"         → 200 {"data":{"id":1,"nickname":"alice"}}

$ bob의 토큰으로 alice의 글 PATCH   → 403 {"error":{"code":"FORBIDDEN","message":"본인이 작성한 글만 수정/삭제할 수 있습니다"}}
$ alice의 토큰으로 PATCH            → 200 {"data":{"id":1,"title":"edited","authorId":1}}

$ 로그인 실패: 틀린 비밀번호 / 없는 이메일
{"error":{"code":"INVALID_CREDENTIALS","message":"이메일 또는 비밀번호가 올바르지 않습니다"}}     ← 두 응답이 완전히 같다
{"error":{"code":"INVALID_CREDENTIALS","message":"이메일 또는 비밀번호가 올바르지 않습니다"}}

$ JWT_SECRET=short cargo run --example s07_auth
Error: JWT_SECRET이 너무 짧습니다 (최소 16자)
```

시험 (`cargo test --example s07_auth`, Argon2가 일부러 느려서 1.3초 걸린다):

```
test tests::토큰이_없거나_위조면_401이고_있으면_통과한다 ... ok
test tests::남의_글은_403이고_로그인_실패는_두_경우가_같다 ... ok
test tests::이메일은_소문자로_맞추고_응답에_비밀번호가_없다 ... ok
test tests::토큰_페이로드는_암호화가_아니라_base64라서_읽힌다 ... ok      ← 비밀키 없이 페이로드가 읽히고, 다른 키로는 검증이 실패한다
test result: ok. 4 passed
```

아래 3-1부터는 이 개념이 실제 프로젝트(`demo/`)에서 어떤 모양이 되는지 이어서 본다.

### 3-1. 비밀번호 해시 — [demo/src/auth/password.rs](demo/src/auth/password.rs)

```rust
pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let hash: PasswordHash = Argon2::default()
            .hash_password(password.as_bytes())
            .map_err(|e| anyhow::anyhow!("비밀번호 해싱 실패: {e}"))?;
        Ok(hash.to_string())
    })
    .await
    .context("해싱 태스크가 패닉했습니다")?
}

pub async fn verify(password: String, password_hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|e| anyhow::anyhow!("저장된 해시 형식이 잘못되었습니다: {e}"))?;
        Ok(Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
    })
    .await
    .context("검증 태스크가 패닉했습니다")?
}
```

| 코드 | 설명 |
|---|---|
| `tokio::task::spawn_blocking` | Argon2는 **수십 ms 동안 CPU를 쥐는 동기 연산**. async 워커 스레드에서 그냥 돌리면 동시 로그인 몇 건에 **서버 전체가 멈춘다.** 별도 스레드 풀로 보낸다 (study15에서 직접 측정) |
| `move ||` | 클로저가 `password`의 **소유권을 가져간다.** 다른 스레드로 넘기려면 `'static`이어야 해서 복사/이동이 필요 |
| `let hash: PasswordHash = ...` | argon2 0.6의 `hash_password`는 **솔트를 알아서 만든다**(1인자). 반환 타입이 제네릭이라 **타입 주석이 필요** |
| `.is_ok()` | 비밀번호 불일치는 "오류"가 아니라 `false`로 다룬다. `Err`는 해시 형식이 깨진 **진짜 오류**용 |
| `.await.context(...)?` 두 겹 | 바깥은 "태스크가 패닉했나", 안쪽 `?`는 "해싱 자체가 실패했나" |

### 3-2. JWT — [demo/src/auth/jwt.rs](demo/src/auth/jwt.rs)

```rust
pub const TOKEN_TTL_SECONDS: i64 = 60 * 60; // 1시간 (공통 API 스펙 v1 §6)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,          // subject — 사용자 id (JWT 표준 이름)
    pub nickname: String,  // 커스텀 클레임
    pub iat: i64,          // issued at
    pub exp: i64,          // expiration — 라이브러리가 만료 검사를 자동으로 해 준다
}

pub fn encode_token(secret: &str, user_id: i64, nickname: &str) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims { sub: user_id, nickname: nickname.to_string(), iat: now, exp: now + TOKEN_TTL_SECONDS };
    Ok(encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret.as_bytes()))?)
}

pub fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::new(Algorithm::HS256))
        .map(|data| data.claims)
}
```

- `Validation::new(Algorithm::HS256)` — **허용 알고리즘을 명시**한다. 토큰 헤더가 말하는 알고리즘을 그대로 믿으면 `alg: none` 같은 공격이 가능해진다.
- `HS256`은 **같은 키로 서명하고 검증**한다(대칭). 키가 곧 모든 권한이므로 `JWT_SECRET`은 길고 무작위여야 하고 이미지에 굽지 않는다.

### 3-3. ⭐ `AuthUser` extractor — [demo/src/auth/extractor.rs](demo/src/auth/extractor.rs)

```rust
#[derive(Debug, Clone)]
pub struct AuthUser { pub id: i64, pub nickname: String }

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>().await
            .map_err(|_| AppError::Unauthorized("Authorization 헤더에 Bearer 토큰이 필요합니다".into()))?;

        let app_state = AppState::from_ref(state);

        let claims = jwt::decode_token(&app_state.jwt_secret, bearer.token()).map_err(|e| {
            match e.kind() {
                ErrorKind::ExpiredSignature => AppError::Unauthorized("토큰이 만료되었습니다".into()),
                _ => AppError::Unauthorized("토큰이 유효하지 않습니다".into()),
            }
        })?;

        Ok(AuthUser { id: claims.sub, nickname: claims.nickname })
    }
}
```

| 코드 | 설명 |
|---|---|
| `FromRequestParts` | **본문을 읽지 않는** extractor 트레잇. 헤더·경로·쿼리용이라 인자 **위치가 자유**롭다 (본문용 `FromRequest`는 마지막 하나만) |
| `AppState: FromRef<S>` | "상태 `S`에서 `AppState`를 꺼낼 수 있으면 된다"는 조건. 상태 타입이 `AppState` 하나면 기본 구현이 알아서 맞아 떨어진다 |
| `TypedHeader<Authorization<Bearer>>` | `Bearer <토큰>` 형식 검사를 라이브러리에 맡긴다 (직접 문자열을 자르지 않는다). `axum-extra`의 `typed-header` feature |
| `ExpiredSignature` 분기 | 만료와 위조를 **다른 문구**로 알려 프론트가 "다시 로그인"을 안내할 수 있게 한다. 코드는 둘 다 `UNAUTHORIZED` |
| 실패 시 | **핸들러가 아예 호출되지 않고** 401이 나간다 |

사용법이 놀랍도록 단순하다 — **인자에 적는 것이 곧 보호**다.

```rust
async fn list(State(state): State<AppState>, ...) -> ...        // 인증 불필요: 안 적으면 공개
async fn create(State(state): State<AppState>, auth: AuthUser,   // 인증 필요: 적으면 보호
                ValidatedJson(body): ValidatedJson<CreatePost>) -> ...
```

보호 여부가 **시그니처에 보이고**, 데코레이터를 붙이다 빠뜨리는 실수가 없다.

### 3-4. 회원가입·로그인 — [demo/src/auth/handler.rs](demo/src/auth/handler.rs)

```rust
/// POST /api/auth/login — 이메일로 로그인한다. (스펙 v1: { email, password })
pub async fn login(State(state): State<AppState>, ValidatedJson(body): ValidatedJson<LoginRequest>)
    -> AppResult<Json<Data<LoginData>>>
{
    let row = repo::find_by_email(&state.db, &normalize_email(&body.email)).await?;   // 이메일은 소문자로 맞춰 저장·조회

    // ⚠️ "없는 이메일"과 "틀린 비밀번호"를 구분해서 알려 주면 안 된다 (user enumeration)
    let Some(user) = row else { return Err(AppError::InvalidCredentials); };

    if !password::verify(body.password, user.password_hash.clone()).await? {
        return Err(AppError::InvalidCredentials);
    }

    let token = jwt::encode_token(&state.jwt_secret, user.id, &user.nickname)?;
    Ok(Json(Data::new(LoginData { user: UserPublic::from(user), access_token: token })))   // { data: { user, accessToken } }
}

/// POST /api/auth/logout — 인증이 필요하고 204. Bearer 토큰은 서버가 기억하지 않으니 지울 것이 없다.
pub async fn logout(_auth: AuthUser) -> StatusCode { StatusCode::NO_CONTENT }

fn map_unique_violation(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.is_unique_violation() {
            let message = db_err.message();
            // 이메일·닉네임 둘 다 UNIQUE라서 어느 쪽이 걸렸는지 오류 메시지에서 가린다
            if message.contains("nickname") || message.contains("username") {
                return AppError::Conflict("이미 사용 중인 닉네임입니다".into());    // 409 CONFLICT
            }
            return AppError::EmailTaken;                                          // 409 EMAIL_TAKEN
        }
    }
    AppError::Database(err)
}
```

- **로그인 실패 응답이 두 경우에서 완전히 같다** (`INVALID_CREDENTIALS`, 같은 문구). 다르면 공격자가 "가입된 이메일 목록"을 수집한다. 테스트가 이것을 지킨다: `로그인_실패_메시지는_이메일_존재_여부를_알려주지_않는다`.
- 이메일은 **소문자로 맞춰 저장하고 조회**한다(`Mina@Example.com`으로 가입해도 `mina@example.com`으로 로그인된다). 이메일 대소문자 때문에 같은 사람이 두 계정을 만드는 일을 막는다.
- 회원가입의 **중복 확인은 `SELECT` 후 `INSERT`가 아니라 `UNIQUE` 제약 + 위반 변환**으로 한다. 먼저 조회하는 방식은 동시 요청 사이에 끼어들 틈이 있다(경합).
- `UserPublic`에는 **`password_hash` 필드가 없다.** 응답에 실수로 섞여 나갈 일이 구조적으로 없다.
- 가입은 평문 비밀번호가 **한 번도 로그에 찍히지 않게** 주의한다(`#[derive(Debug)]`한 요청 타입을 `tracing::debug!(?body)`로 찍으면 비밀번호가 로그에 남는다).

### 3-5. 인가 — 소유권 검사

```rust
async fn ensure_owner(state: &AppState, post_id: i64, auth: &AuthUser) -> AppResult<PostRow> {
    let row = repo::find(&state.db, post_id).await?
        .ok_or_else(|| AppError::not_found("post", post_id))?;       // 없으면 404

    match row.author_id {
        Some(owner_id) if owner_id == auth.id => Ok(row),            // 주인이면 통과
        _ => Err(AppError::Forbidden("본인이 작성한 글만 수정/삭제할 수 있습니다".into())),   // 403
    }
}
```

`update`/`delete`/`upload_thumbnail`이 모두 첫 줄에서 `ensure_owner`를 부른다. 그리고 **작성자는 요청 본문에서 받지 않고** `auth.nickname`/`auth.id`(토큰)에서만 가져온다 — 클라이언트가 `author: "남의이름"`을 보내도 `deny_unknown_fields`로 거절된다.

### 3-6. CORS 설정 — [demo/src/lib.rs](demo/src/lib.rs)

```rust
fn cors_layer(origins: &str) -> CorsLayer {
    let origins: Vec<HeaderValue> = origins.split(',').map(str::trim).filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok()).collect();                               // CORS_ORIGIN: 쉼표로 여러 개

    CorsLayer::new()
        .allow_origin(origins)                                                   // 출처를 명시
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .max_age(Duration::from_secs(3600))                                      // preflight 결과를 1시간 캐시
}
```

`CorsLayer::permissive()`는 편하지만 **운영에서 쓰지 않는다.** 허용 출처는 환경변수 `CORS_ORIGIN`(쉼표로 여러 개, 기본 `http://localhost:5173`)에서 읽는다 — 공통 API 스펙 v1 §7의 요구이고 `Config::from_env`가 시작할 때 헤더로 쓸 수 없는 값을 거절한다. `CORS_ORIGIN=https://front.example.com`으로 띄워 그 출처만 `access-control-allow-origin`을 받고 `http://localhost:5173`은 받지 않는 것을 확인했다 (study6 3-10).

### 3-7. 실행 확인

```
$ curl -X POST .../api/posts -d '{"title":"x","body":"y"}'                     # 토큰 없음
{"error":{"code":"UNAUTHORIZED","message":"Authorization 헤더에 Bearer 토큰이 필요합니다","requestId":"5699cde3-..."}}   → 401

$ curl -X PATCH .../api/posts/1 -H "authorization: Bearer $BOB" -d '{"title":"hijack"}'   # alice가 쓴 글
{"error":{"code":"FORBIDDEN","message":"본인이 작성한 글만 수정/삭제할 수 있습니다","requestId":"ec0899eb-..."}}        → 403

$ curl -X POST .../api/auth/login -d '{"email":"alice@example.com","password":"wrongwrong"}'   # 틀린 비밀번호
$ curl -X POST .../api/auth/login -d '{"email":"nobody@example.com","password":"wrongwrong"}'  # 없는 이메일
{"error":{"code":"INVALID_CREDENTIALS","message":"이메일 또는 비밀번호가 올바르지 않습니다","requestId":"..."}}   → 둘 다 401, message 동일

$ 토큰 가운데 부분을 base64 디코드
{'sub': 1, 'nickname': 'alice', 'iat': 1795371872, 'exp': 1795375472}        ← 암호화가 아니라서 그대로 읽힌다 (exp - iat = 3600)

$ curl .../api/auth/me -H "authorization: Bearer $TOKEN"
{"data":{"id":1,"email":"alice@example.com","nickname":"alice","createdAt":"2026-11-22T18:24:31.885385500Z"}}
$ curl -X POST .../api/auth/logout -H "authorization: Bearer $TOKEN"                                  → 204
```

---

## ④ 🔗 연동 포인트 — 로그인 상태를 프론트와 백엔드가 공유하는 방법

### 방식 A. Bearer 토큰 (이 `demo/`의 방식)

```
로그인:  POST /api/auth/login → { "data": { "user": {...}, "accessToken": "eyJ..." } }
이후:    fetch("/api/posts", { headers: { Authorization: `Bearer ${token}` } })
```

- 토큰을 **프론트가 직접 들고 있다가 헤더로 실어 보낸다.**
- 브라우저가 자동으로 보내는 것이 없어서 **CSRF에 안전**하다.
- 대신 토큰을 **어디에 두느냐**가 문제다: `localStorage`는 XSS 한 번에 털린다. 메모리(변수)에 두면 안전하지만 새로고침하면 사라진다(→ 다시 로그인 또는 리프레시 토큰 설계).
- 모바일 앱·다른 서버가 호출하는 API에 어울린다.

### 방식 B. HttpOnly 쿠키 ([project/crates/web/src/auth.rs](project/crates/web/src/auth.rs))

```rust
pub fn session_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={TOKEN_TTL_SECONDS}");
    if secure { cookie.push_str("; Secure"); }
    cookie
}
```

| 속성 | 효과 |
|---|---|
| `HttpOnly` | JS(`document.cookie`)가 **못 읽는다** → XSS가 나도 토큰을 훔치기 어렵다 |
| `SameSite=Lax` | 다른 사이트에서 시작된 `POST` 등에는 쿠키가 **안 실린다** → CSRF 완화 |
| `Secure` | HTTPS에서만 전송. **운영 필수**, 로컬 `http`에서는 꺼야 동작 (`SECURE_COOKIE` 환경변수) |
| `Max-Age` | 수명. 로그아웃은 같은 이름을 `Max-Age=0`으로 덮어쓴다 |

- 프론트는 **토큰을 만지지 않는다.** 같은 도메인이면 `fetch`가 알아서 쿠키를 보낸다. 다른 출처(포트가 다른 개발 서버)면 `fetch(..., { credentials: "include" })` + 서버 CORS에 `allow_credentials(true)` + **정확한 출처**(`*` 불가)가 필요하다 (study12에서 실행).
- `project/`의 `AuthUser`는 `Authorization: Bearer`와 `session` 쿠키를 **둘 다** 받는다(헤더 우선).

### 선택 기준

| 상황 | 추천 |
|---|---|
| 같은 도메인의 웹사이트(SPA를 Axum이 함께 서빙) | **쿠키** (공통 API 스펙 v1 §6도 허용) |
| 모바일 앱, 외부 서버가 호출, 도메인이 다른 프론트 | **Bearer** |
| 둘 다 | `AuthUser`가 두 가지를 모두 받게 한다 (`project/` 방식) |

### 프론트 화면과의 약속

| 응답 | 프론트 동작 |
|---|---|
| 401 `UNAUTHORIZED` | 로그인 화면으로 보내고, 원래 가려던 주소를 기억 |
| 401 `INVALID_CREDENTIALS` | 로그인 폼에 "이메일 또는 비밀번호를 확인하세요" |
| 403 `FORBIDDEN` | "권한이 없습니다" (로그인은 되어 있음 → 로그인 화면으로 보내지 않는다) |
| 409 `EMAIL_TAKEN` | 가입 폼의 이메일 칸에 오류 표시 (닉네임 중복은 `CONFLICT` — 닉네임 칸에) |
| 429 `RATE_LIMITED` | `Retry-After` 초만큼 기다렸다가 다시 시도하라고 안내 (study6 3-8) |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 로그인 요청 시 **HTTP 오류가 아니라 `connection was closed unexpectedly`** | **jsonwebtoken 11**: `cargo add jsonwebtoken`만 하면 암호화 백엔드가 안 켜져서 **런타임 패닉**. 패닉한 요청은 500이 아니라 연결이 끊긴다 | `cargo add jsonwebtoken --features rust_crypto` (순수 Rust라 Docker/크로스 컴파일에 유리). 패닉을 500 JSON으로 바꾸는 `CatchPanicLayer`는 study8 |
| 서버 로그: `Could not automatically determine the process-level CryptoProvider` | 위와 같은 원인 | 위와 같음. **서버 로그를 먼저 본다** |
| `hash_password` 인자 개수 오류 / `SaltString` 없음 | 인터넷 예제는 **argon2 0.5** 기준. 0.6은 솔트를 알아서 만든다 | `Argon2::default().hash_password(pw.as_bytes())` (1인자) |
| `type annotations needed` (`hash_password`) | 반환 타입이 제네릭 | `let hash: PasswordHash = ...` |
| 동시 로그인 몇 건에 **서버 전체가 멈춤** | Argon2를 async 안에서 직접 실행 (CPU 독점) | `spawn_blocking` |
| `ExpiredSignature`인데 계속 401 | 토큰 만료 (1시간) | 프론트가 401 `UNAUTHORIZED`를 받으면 재로그인. 학습용이라면 TTL을 잠깐 늘릴 수 있다 |
| 로그아웃했는데 토큰이 계속 유효 | **JWT는 서버가 기억하지 않아서 만료 전 무효화가 안 된다** | TTL을 짧게 + 리프레시 토큰, 또는 차단 목록(서버 상태)을 둔다. 완전한 즉시 로그아웃이 필요하면 서버 세션 방식 |
| 쿠키가 안 저장됨 (개발 중) | `Secure` 속성이 붙은 채 `http://localhost`로 접속 / `SameSite` 때문에 다른 사이트의 요청에서 제외 | 개발에서는 `SECURE_COOKIE=false`. 다른 출처 요청은 `credentials: "include"` + CORS 설정 |
| 브라우저 콘솔: `Credential is not supported if the CORS header 'Access-Control-Allow-Origin' is '*'` | 쿠키를 쓰는 CORS인데 허용 출처가 `*` | 출처를 정확히 적고 `allow_credentials(true)` |
| 남의 글을 고칠 수 있음 | **인증만 하고 인가를 안 함** (`AuthUser`만 인자로 두고 소유자 비교 누락) | 수정·삭제 첫 줄에 `ensure_owner`. 서비스 계층이라면 `actor_id`를 필수 인자로 |
| 로그에 비밀번호가 찍힘 | 요청 DTO를 `Debug`로 출력 | 비밀번호 필드는 로그에 남기지 않는다. 커스텀 `Debug`나 `secrecy` 같은 타입 |
| 사용자 입력을 그대로 `innerHTML`로 그림 | **XSS** | `textContent`, 템플릿 엔진의 자동 이스케이프, (추가) `Content-Security-Policy` 헤더 |

---

## ⑥ 직접 해보기

1. **토큰 직접 열어 보기**: 로그인으로 받은 `data.accessToken`을 `.`로 나눠 가운데 부분을 base64 디코드하면 `sub`, `nickname`, `exp`가 그대로 보인다. 왜 비밀을 넣으면 안 되는지 확인한다.
2. **만료 실험**: `TOKEN_TTL_SECONDS`를 5초로 줄이고 로그인 → 6초 뒤 글쓰기를 호출해 `토큰이 만료되었습니다` 문구를 본다. 원래 값(3600)으로 되돌린다.
3. **CSRF 직접 보기**: [study12](study12.md)의 `s12_front`로 쿠키를 발급받은 뒤, 다른 출처의 HTML에서 `<form method="POST" action="http://127.0.0.1:3012/api/logout">`을 제출하면 `SameSite=Lax` 때문에 쿠키가 실리는지 개발자도구 Network 탭에서 확인한다. `SameSite=None`으로 바꿨을 때와 비교한다.
4. **단계 예제로 실험**: `s07_auth`의 `rename_post`에서 `ensure_owner` 호출 한 줄을 지우고 `cargo test --example s07_auth`를 돌려 어떤 시험이 깨지는지 본다. **인증(`AuthUser`)만 있고 인가가 빠진 코드가 얼마나 조용히 구멍이 되는지** 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ 인증(401)과 인가(403)의 차이를 예와 함께 설명할 수 있다
- ☐ 비밀번호를 평문·단순 해시로 저장하면 안 되는 이유와 Argon2의 PHC 문자열 구성을 설명할 수 있다
- ☐ 해싱을 `spawn_blocking`으로 보내야 하는 이유를 설명할 수 있다
- ☐ JWT가 암호화가 아니라 서명이라는 것, 즉시 무효화가 안 된다는 것을 안다
- ☐ `AuthUser`를 인자에 적는 것만으로 라우트가 보호되는 원리(`FromRequestParts`)를 설명할 수 있다
- ☐ 로그인 실패 메시지를 같게 하는 이유(user enumeration)를 설명할 수 있다
- ☐ 남의 글 수정이 403으로 막히는 것을 테스트로 확인했다
- ☐ CORS / XSS / CSRF가 각각 무엇을 막는지 한 문장씩 말할 수 있다
- ☐ Bearer 방식과 HttpOnly 쿠키 방식의 장단점, `HttpOnly`/`SameSite`/`Secure`의 의미를 안다
- ☐ `jsonwebtoken`에 `rust_crypto` feature가 필요한 이유와 증상을 기억한다
- ☐ Bearer 방식의 로그아웃이 서버에서 하는 일이 없는 이유(204만 준다)와 즉시 무효화가 필요할 때의 대안을 말할 수 있다
- ☐ `cargo run --example s07_auth`로 가입→로그인→토큰 디코드→401/403/로그인 실패 동일 응답을 직접 확인했다

---

## ⑧ 다음 단계 연결

보호 장치가 생겼지만 코드가 `main.rs`/`lib.rs`에 점점 쌓이고 있다. [study8](study8.md)에서는 폴더 구조를 정리하고, 공통 모듈(오류·extractor·미들웨어)과 설정을 분리하고, 미들웨어 **순서**를 다루며, 개발/운영 설정을 나누는 방법을 정리한다.
