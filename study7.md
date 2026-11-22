# study7 — 인증 · 보안 기초

> 🎯 **목표**: 회원가입·로그인(비밀번호 해시 + JWT)을 만들고, 인자에 `AuthUser`를 적는 것만으로 라우트가 보호되게 하며, **인증(누구인가)과 인가(할 수 있는가)** 를 구분한다. CORS·XSS·CSRF의 기초와, 로그인 상태를 프론트와 백엔드가 공유하는 두 방식(Bearer 헤더 / HttpOnly 쿠키)을 이해한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [demo/src/auth/](demo/src/auth/) (`password.rs`, `jwt.rs`, `extractor.rs`, `handler.rs`), [demo/src/posts/handler.rs](demo/src/posts/handler.rs) (`ensure_owner`), 쿠키 방식: [project/crates/web/src/auth.rs](project/crates/web/src/auth.rs) |
| 검증 | `demo/tests/api.rs`의 인증·인가 테스트 (토큰 없이 쓰기 401, 남의 글 수정 403, 로그인 실패 메시지 동일) |
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

JWT = `헤더.페이로드.서명`. 페이로드(`sub`, `username`, `iat`, `exp`)는 **암호화가 아니라 base64**라서 누구나 읽을 수 있다 → **비밀을 넣지 않는다.** 서명이 맞아야만 유효하다.

### CORS · XSS · CSRF — 서로 다른 세 가지

| | 무엇을 | 막는 곳 | 요점 |
|---|---|---|---|
| **CORS** | 다른 **출처**의 JS가 내 API 응답을 **읽는 것** | 브라우저 (서버는 허용 목록을 알려 줌) | 서버 보안 장치가 아니다. `curl`은 CORS와 무관하다 |
| **XSS** | 공격자의 스크립트가 **내 페이지 안에서 실행**되는 것 | 출력 이스케이프, `textContent`, CSP, HttpOnly 쿠키 | 한 번 뚫리면 그 페이지의 모든 권한을 가진다 |
| **CSRF** | 로그인된 사용자의 브라우저가 **공격자 사이트에서** 내 서버로 요청을 보내게 하는 것 | `SameSite` 쿠키, CSRF 토큰, Bearer 헤더 방식 | 브라우저가 **쿠키를 자동으로** 실어 보내기 때문에 생기는 문제 |

---

## ③ 🛠️ 실기

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
    pub username: String,  // 커스텀 클레임
    pub iat: i64,          // issued at
    pub exp: i64,          // expiration — 라이브러리가 만료 검사를 자동으로 해 준다
}

pub fn encode_token(secret: &str, user_id: i64, username: &str) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims { sub: user_id, username: username.to_string(), iat: now, exp: now + TOKEN_TTL_SECONDS };
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
pub struct AuthUser { pub id: i64, pub username: String }

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

        Ok(AuthUser { id: claims.sub, username: claims.username })
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
pub async fn login(State(state): State<AppState>, ValidatedJson(body): ValidatedJson<LoginRequest>)
    -> AppResult<Json<TokenResponse>>
{
    let row = repo::find_by_username(&state.db, &body.username).await?;

    // ⚠️ "없는 아이디"와 "틀린 비밀번호"를 구분해서 알려 주면 안 된다 (user enumeration)
    let Some(user) = row else { return Err(AppError::InvalidCredentials); };

    if !password::verify(body.password, user.password_hash.clone()).await? {
        return Err(AppError::InvalidCredentials);
    }

    let token = jwt::encode_token(&state.jwt_secret, user.id, &user.username)?;
    Ok(Json(TokenResponse { access_token: token, token_type: "Bearer", expires_in: jwt::TOKEN_TTL_SECONDS, user: UserPublic::from(user) }))
}

fn map_unique_violation(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.is_unique_violation() { return AppError::EmailTaken; }     // 409
    }
    AppError::Database(err)
}
```

- **로그인 실패 응답이 두 경우에서 완전히 같다** (`INVALID_CREDENTIALS`, 같은 문구). 다르면 공격자가 "가입된 아이디 목록"을 수집한다. 테스트가 이것을 지킨다: `로그인_실패_메시지는_아이디_존재_여부를_알려주지_않는다`.
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

`update`/`delete`/`upload_thumbnail`이 모두 첫 줄에서 `ensure_owner`를 부른다. 그리고 **작성자는 요청 본문에서 받지 않고** `auth.username`/`auth.id`(토큰)에서만 가져온다 — 클라이언트가 `author: "남의이름"`을 보내도 `deny_unknown_fields`로 거절된다.

### 3-6. CORS 설정 — [demo/src/lib.rs](demo/src/lib.rs)

```rust
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin("http://localhost:5173".parse::<HeaderValue>().unwrap())   // 출처를 명시
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .max_age(Duration::from_secs(3600))                                      // preflight 결과를 1시간 캐시
}
```

`CorsLayer::permissive()`는 편하지만 **운영에서 쓰지 않는다.** 허용 출처는 환경변수(`CORS_ORIGIN`)로 빼는 것이 공통 API 스펙 v1의 요구인데, 현재 `demo/`는 코드에 고정되어 있다 (study6 ④ 표의 ❌ 항목).

### 3-7. 실행 확인

```
$ curl -X POST .../posts -d '{"title":"무단","content":"본문"}'                    # 토큰 없음
{"error":{"code":"UNAUTHORIZED","message":"Authorization 헤더에 Bearer 토큰이 필요합니다", ...}}   → 401

alice가 쓴 글을 bob의 토큰으로 PATCH
{"error":{"code":"FORBIDDEN","message":"본인이 작성한 글만 수정/삭제할 수 있습니다", ...}}        → 403

없는 아이디 / 틀린 비밀번호로 로그인  →  둘 다 401 {"code":"INVALID_CREDENTIALS", 같은 message}
```

---

## ④ 🔗 연동 포인트 — 로그인 상태를 프론트와 백엔드가 공유하는 방법

### 방식 A. Bearer 토큰 (이 `demo/`의 방식)

```
로그인:  POST /auth/login → { "accessToken": "eyJ..." }
이후:    fetch("/posts", { headers: { Authorization: `Bearer ${token}` } })
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
| 401 `INVALID_CREDENTIALS` | 로그인 폼에 "아이디 또는 비밀번호를 확인하세요" |
| 403 `FORBIDDEN` | "권한이 없습니다" (로그인은 되어 있음 → 로그인 화면으로 보내지 않는다) |
| 409 `EMAIL_TAKEN` | 가입 폼의 이메일 칸에 오류 표시 |

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

1. **토큰 직접 열어 보기**: 로그인으로 받은 `accessToken`을 `.`로 나눠 가운데 부분을 base64 디코드하면 `sub`, `username`, `exp`가 그대로 보인다. 왜 비밀을 넣으면 안 되는지 확인한다.
2. **만료 실험**: `TOKEN_TTL_SECONDS`를 5초로 줄이고 로그인 → 6초 뒤 글쓰기를 호출해 `토큰이 만료되었습니다` 문구를 본다. 원래 값(3600)으로 되돌린다.
3. **CSRF 직접 보기**: [study12](study12.md)의 `s12_front`로 쿠키를 발급받은 뒤, 다른 출처의 HTML에서 `<form method="POST" action="http://127.0.0.1:3012/api/logout">`을 제출하면 `SameSite=Lax` 때문에 쿠키가 실리는지 개발자도구 Network 탭에서 확인한다. `SameSite=None`으로 바꿨을 때와 비교한다.

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

---

## ⑧ 다음 단계 연결

보호 장치가 생겼지만 코드가 `main.rs`/`lib.rs`에 점점 쌓이고 있다. [study8](study8.md)에서는 폴더 구조를 정리하고, 공통 모듈(오류·extractor·미들웨어)과 설정을 분리하고, 미들웨어 **순서**를 다루며, 개발/운영 설정을 나누는 방법을 정리한다.
