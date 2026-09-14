# 회원 & JWT 인증 extractor (Study 6 · 고급 1)

여기서부터 고급 파트입니다. 지금까지는 **누구나 아무 글이나 쓰고 지울 수 있었습니다.** 이제 회원 개념을 넣고, JWT로 신원을 확인하고, 남의 글은 못 건드리게 만듭니다.

**이 문서에서 만드는 것**: `users` 테이블 + Argon2 해시 + 로그인/JWT 발급 + `AuthUser` extractor + 소유권 검사(403)

```powershell
cargo add jsonwebtoken --features rust_crypto
cargo add argon2
cargo add axum-extra --features typed-header
```

---

## 1. 이번 study의 설계

```
POST /auth/register  →  회원가입 (Argon2로 비밀번호 해시 저장)
POST /auth/login     →  아이디/비번 확인 후 JWT 발급
GET  /auth/me        →  🔒 토큰으로 내 정보 조회

GET    /posts        →  공개
GET    /posts/{id}   →  공개
POST   /posts        →  🔒 인증 필요 (작성자는 토큰에서)
PATCH  /posts/{id}   →  🔒 인증 + 본인 글만
DELETE /posts/{id}   →  🔒 인증 + 본인 글만
```

**인증(Authentication)과 인가(Authorization)를 구분**하는 것이 이번 문서의 큰 주제입니다.

| | 질문 | 이번 구현 |
|---|---|---|
| **인증** | "너 누구야?" | `AuthUser` extractor (토큰 검증) → 실패 시 **401** |
| **인가** | "그걸 할 권한이 있어?" | `ensure_owner()` (작성자 비교) → 실패 시 **403** |

---

## 2. 마이그레이션 추가 — 기존 파일은 건드리지 않는다

`0001_create_posts.sql`은 이미 적용되었으므로 **수정하면 안 됩니다**(체크섬 불일치). 새 파일을 만듭니다.

### `migrations/0002_create_users.sql`

```sql
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL UNIQUE,
    -- 평문 비밀번호는 절대 저장하지 않는다. Argon2 PHC 문자열만 저장.
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

-- 게시글에 작성자(FK)를 추가한다.
-- 기존 행에는 값이 없으므로 NULL 허용.
ALTER TABLE posts ADD COLUMN author_id INTEGER REFERENCES users (id);

CREATE INDEX IF NOT EXISTS idx_posts_author_id ON posts (author_id);
```

**`author_id`를 NOT NULL로 못 만든 이유**: 이미 데이터가 들어 있는 테이블에 NOT NULL 컬럼을 추가하려면 기본값이 필요한데, "기존 글의 작성자"는 정할 수 없습니다. 그래서 `Option<i64>`로 두고, 코드에서 "주인 없는 글"로 취급합니다. — **실무에서 나중에 인증을 붙일 때 실제로 겪는 상황**입니다.

---

## 3. 비밀번호는 절대 평문으로 저장하지 않는다

### 왜 Argon2인가

| 방식 | 문제 |
|---|---|
| 평문 저장 | 말할 것도 없이 최악 |
| MD5 / SHA-256 | **너무 빠르다.** GPU로 초당 수십억 번 시도 가능 |
| bcrypt | 괜찮지만 오래됨. 최대 72바이트 제한 |
| **Argon2id** | **2015 Password Hashing Competition 우승.** 메모리까지 많이 쓰게 만들어 GPU/ASIC 공격에 강함 |

핵심은 **"일부러 느리게"** 입니다. 로그인 한 번에 수십 ms가 걸리는 건 사용자에게 아무 문제가 없지만, 유출된 해시를 무차별 대입하는 공격자에게는 치명적입니다.

### ⭐ 그래서 `spawn_blocking`이 필요하다

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
```

**이게 study0에서 예고했던 "async 안에서 블로킹하면 안 된다"의 실제 사례입니다.**

- Argon2 해싱은 **수십 ms 동안 CPU를 100% 쓰는 동기 연산**입니다.
- tokio 워커 스레드는 CPU 코어 수만큼(보통 8~16개)뿐입니다. 그 위에서 그냥 돌리면, 동시 로그인 16건이 **서버 전체를 멈춥니다.** 헬스체크도 응답 못 합니다.
- `spawn_blocking`은 **별도의 블로킹 전용 스레드 풀**(기본 최대 512개)에 작업을 던집니다. 워커 스레드는 즉시 양보하고 다른 요청을 처리합니다.

> 판단 기준: **"이 코드가 `.await` 없이 수 ms 이상 CPU를 쓰는가?"** 그렇다면 `spawn_blocking`. 해싱, 이미지 리사이즈, 압축, 큰 JSON 파싱 등이 해당합니다.

- `.await` 뒤의 `?`가 두 번 나오는 것에 주의하세요. `spawn_blocking(...).await`는 `Result<T, JoinError>`(태스크가 패닉했는지)를 반환하고, 그 안의 `T`가 또 `Result`입니다. 그래서 `.context(...)?`로 바깥을 벗기고, 안쪽은 함수 반환으로 넘깁니다.

### PHC 문자열

저장되는 값은 이렇게 생겼습니다.

```
$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$RdescudvJCsgt3ub+b+dWRWJTmaaJObG
 └ 알고리즘 └ 버전 └ 파라미터(메모리/반복/병렬) └ 솔트 └ 해시
```

- **솔트가 해시 안에 들어 있습니다.** 별도 컬럼이 필요 없습니다.
- 파라미터도 들어 있어서, 나중에 보안 강도를 올려도 **기존 해시를 그대로 검증**할 수 있습니다.
- 솔트는 사용자마다 다르므로, **같은 비밀번호라도 저장된 해시가 다릅니다.** 레인보우 테이블이 통하지 않습니다.

### 비밀번호를 응답에 안 흘리는 방법

```rust
/// DB 행 — password_hash 있음. Serialize 없음.
#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

/// API 응답 — password_hash 필드가 아예 없음.
#[derive(Debug, Clone, Serialize)]
pub struct UserPublic {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}
```

**`UserRow`에는 `Serialize`가 없습니다.** 즉 `Json(user_row)`를 쓰면 **컴파일이 안 됩니다.** 비밀번호 해시를 실수로 내보내는 게 타입 시스템 차원에서 막혀 있습니다.

| 프레임워크 | 방식 | 안전성 |
|---|---|---|
| TypeORM | `@Column({ select: false })` | `save()` 반환값에는 남아 있는 함정 |
| Prisma | 전역 `omit` | 꽤 확실함 |
| Spring | `@JsonIgnore` | 애노테이션을 빠뜨리면 샘 |
| **Axum + serde** | **`Serialize`를 구현하지 않은 별도 타입** | **컴파일 타임 보장** |

---

## 4. JWT

### 토큰 구조

```
eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9 . eyJzdWIiOjEsInVzZXJuYW1lIjoid2hpbGUi...  . m_ImsPxhlG2L2FMCg2rr_j2Lc...
└─ 헤더(알고리즘)                        └─ 페이로드(클레임)                       └─ 서명
```

**⚠️ 앞의 두 부분은 암호화가 아니라 그냥 Base64URL 인코딩입니다.** 누구나 디코드해서 읽을 수 있습니다. **토큰에 비밀을 담지 마세요.** 서명이 보장하는 건 "내용이 변조되지 않았다"뿐입니다.

### 클레임 정의

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,          // subject — 사용자 id (표준)
    pub username: String,  // 커스텀 클레임
    pub iat: i64,          // issued at (표준)
    pub exp: i64,          // expiration (표준)
}
```

`sub`/`iat`/`exp`는 **RFC 7519 표준 이름**입니다. 표준 이름을 쓰면 라이브러리가 만료 검사를 자동으로 해줍니다.

### 발급과 검증

```rust
pub fn encode_token(secret: &str, user_id: i64, username: &str) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now,
        exp: now + TOKEN_TTL_SECONDS,
    };

    Ok(encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

pub fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let validation = Validation::new(Algorithm::HS256);  // exp 검사 자동 포함
    decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation)
        .map(|data| data.claims)
}
```

- **HS256**은 대칭 키(서명·검증에 같은 비밀). 서비스가 하나면 충분합니다. 여러 서비스가 검증만 해야 한다면 **RS256**(공개키/개인키)을 씁니다.
- `Validation::new(...)`는 기본적으로 `exp`를 검사합니다. 만료된 토큰은 `ExpiredSignature` 에러가 됩니다.

### JWT의 근본적 한계 — 즉시 무효화가 안 된다

서명만 맞으면 통과하므로, **로그아웃해도 만료 전까지는 그 토큰이 계속 유효합니다.** 실무에서는:

- 만료 시간을 짧게(15분~1시간) + **리프레시 토큰**으로 갱신
- 강제 로그아웃이 필요하면 **블랙리스트**(Redis 등)를 둠 — 이러면 "상태 없는 인증"이라는 JWT의 장점이 반쯤 사라집니다
- 정말 세션 무효화가 중요하면 그냥 **서버 세션**을 쓰는 게 나을 수도 있습니다

---

## 5. ⭐ `AuthUser` extractor — 이게 Guard다

`src/auth/extractor.rs`:

```rust
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1) Authorization: Bearer <token> 헤더 파싱
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AppError::Unauthorized("Authorization 헤더에 Bearer 토큰이 필요합니다".into()))?;

        // 2) 앱 상태에서 서명 키를 꺼낸다
        let app_state = AppState::from_ref(state);

        // 3) 서명 + 만료 검증
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

### 사용법이 놀랍도록 단순하다

```rust
// 🔒 인증 필요 — 인자에 AuthUser를 적기만 하면 된다
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,                                 // ← 실패하면 핸들러가 아예 호출되지 않고 401
    ValidatedJson(body): ValidatedJson<CreatePost>,
) -> AppResult<impl IntoResponse> { ... }

// 🔓 공개 — 그냥 안 적으면 된다
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Post>>> { ... }
```

| | 표현 방법 | 어디에 적히나 |
|---|---|---|
| Spring Security | `.requestMatchers("/posts").authenticated()` | **설정 파일** (핸들러와 떨어져 있음) |
| NestJS | `@UseGuards(AuthGuard('jwt'))` | 핸들러 위 데코레이터 |
| FastAPI | `user = Depends(get_current_user)` | **함수 인자** ← 가장 비슷 |
| **Axum** | **`auth: AuthUser` 인자** | **함수 인자 + 타입** |

Axum 방식의 장점: **인증된 사용자 정보를 쓰려면 그 인자가 반드시 있어야 하므로, "가드는 붙였는데 사용자 정보를 못 꺼내는" 불일치가 생길 수 없습니다.** 반대로 단점: "어떤 라우트가 보호되는지"를 한눈에 보려면 핸들러 시그니처를 다 봐야 합니다. (라우터에서 `route_layer`로 묶는 방법도 있습니다 — study7)

### `FromRequestParts` vs `FromRequest`

| trait | 본문 접근 | 핸들러 인자 위치 |
|---|---|---|
| `FromRequestParts` | ❌ (헤더/경로/쿼리만) | **아무 데나, 여러 개 가능** |
| `FromRequest` | ✅ | **마지막 하나만** |

`AuthUser`는 헤더만 보므로 `FromRequestParts`입니다. 그래서 `ValidatedJson`과 **함께** 쓸 수 있습니다. (`FromRequest`로 만들었다면 둘 중 하나만 가능했을 겁니다)

### `FromRef`가 뭔가

```rust
where AppState: FromRef<S>
```

"제네릭 상태 `S`에서 `AppState`를 꺼낼 수 있다면"이라는 뜻입니다.

- 우리 앱은 `S == AppState`입니다. axum에 `impl<T: Clone> FromRef<T> for T` 블랭킷 구현이 있어서 **자동으로 만족**됩니다.
- 그냥 `impl FromRequestParts<AppState> for AuthUser`라고 써도 지금은 동작합니다. **하지만** 나중에 상태를 `AppState` 대신 더 큰 구조체로 감싸거나, 이 extractor를 다른 앱에서 재사용하려면 `FromRef` 버전이 유연합니다. axum 생태계의 관용 패턴이니 익혀두는 게 좋습니다.

### `axum-extra`의 `TypedHeader`

```rust
parts.extract::<TypedHeader<Authorization<Bearer>>>().await
```

직접 하면 이런 코드가 됩니다.

```rust
let header = parts.headers.get("authorization").and_then(|v| v.to_str().ok());
let token = header.and_then(|h| h.strip_prefix("Bearer "));
// 대소문자? 공백 여러 개? 빈 토큰? ...
```

`TypedHeader`는 **헤더 형식을 타입으로 검증**합니다. `Bearer` 접두사 처리, 공백, 인코딩을 전부 맡깁니다. `axum-extra`의 `typed-header` feature로 켭니다.

---

## 6. 로그인 핸들러 — 보안 디테일

```rust
pub async fn login(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> AppResult<Json<TokenResponse>> {
    let row = repo::find_by_username(&state.db, &body.username).await?;

    // ⚠️ "없는 아이디"와 "틀린 비밀번호"를 구분해서 알려주면 안 된다.
    let Some(user) = row else {
        return Err(AppError::Unauthorized("아이디 또는 비밀번호가 올바르지 않습니다".into()));
    };

    if !password::verify(body.password, user.password_hash.clone()).await? {
        return Err(AppError::Unauthorized("아이디 또는 비밀번호가 올바르지 않습니다".into()));
    }

    let token = jwt::encode_token(&state.jwt_secret, user.id, &user.username)?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "Bearer",
        expires_in: jwt::TOKEN_TTL_SECONDS,
        user: UserPublic::from(user),
    }))
}
```

### 왜 오류 메시지를 똑같이 하나 — user enumeration

"그런 아이디 없습니다" vs "비밀번호가 틀렸습니다"를 구분해 주면, 공격자가 **가입된 아이디 목록을 수집**할 수 있습니다. 그 목록으로 크리덴셜 스터핑, 피싱을 합니다. **둘 다 같은 메시지, 같은 상태 코드**로 응답해야 합니다.

> 엄밀히 하려면 타이밍도 맞춰야 합니다. 지금 코드는 "없는 아이디"일 때 해싱을 건너뛰므로 **응답이 눈에 띄게 빠릅니다.** 타이밍 공격을 막으려면 사용자가 없어도 더미 해시로 검증을 한 번 돌리는 기법을 씁니다. (학습용이라 여기까지는 안 했습니다)

### `let ... else` 문법

```rust
let Some(user) = row else {
    return Err(...);
};
// 여기부터 user는 UserRow (Option이 아님)
```

Rust 1.65에 들어온 문법입니다. `match`나 `if let`으로 감싸서 들여쓰기가 깊어지는 걸 막아줍니다. **`else` 블록은 반드시 발산(return/break/panic)해야** 합니다.

### UNIQUE 제약 위반을 409로 번역

```rust
fn map_unique_violation(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(ref db_err) = err {
        if db_err.is_unique_violation() {
            return AppError::Conflict("이미 사용 중인 아이디 또는 이메일입니다".into());
        }
    }
    AppError::Database(err)
}
```

그냥 `?`로 던지면 **500**이 됩니다. 하지만 "이미 있는 아이디"는 **서버 잘못이 아니라 클라이언트 상황**이므로 `409 Conflict`가 맞습니다.

`is_unique_violation()`은 sqlx가 **DB별 에러 코드 차이를 흡수**해 주는 메서드입니다(SQLite `2067`, Postgres `23505`). DB를 바꿔도 이 코드는 그대로 동작합니다.

> "먼저 SELECT로 중복을 확인하고 없으면 INSERT"는 **경합 조건(race condition)** 이 있습니다. 두 요청이 동시에 오면 둘 다 통과합니다. **DB 제약에 맡기고 위반을 번역하는 쪽이 정답**입니다.

---

## 7. 인가 — 인증만으로는 부족하다

`AuthUser`는 "누구인지"까지만 알려줍니다. 로그인만 하면 **남의 글도 지울 수 있습니다.**

```rust
async fn ensure_owner(state: &AppState, post_id: i64, auth: &AuthUser) -> AppResult<()> {
    let row = repo::find(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::not_found("post", post_id))?;

    match row.author_id {
        Some(owner_id) if owner_id == auth.id => Ok(()),
        // 인증 도입 전에 만들어진 글(author_id IS NULL)은 주인이 없으므로 수정 불가로 본다.
        _ => Err(AppError::Forbidden("본인이 작성한 글만 수정/삭제할 수 있습니다".into())),
    }
}
```

| 상태 코드 | 의미 | 이 앱에서 |
|---|---|---|
| **401** Unauthorized | "너 누군지 모르겠다" (사실 이름이 잘못됨 — Unauthenticated가 맞음) | 토큰 없음/만료/위조 |
| **403** Forbidden | "누군지는 알겠는데, 그건 못 한다" | 남의 글 수정 시도 |

`match`의 **가드 패턴** `Some(owner_id) if owner_id == auth.id`에 주목하세요. "값이 `Some`이면서 **동시에** 조건을 만족할 때"를 한 줄로 표현합니다.

### 작성자는 클라이언트가 정하지 않는다

```rust
// study5의 CreatePost
pub struct CreatePost {
    pub title: String,
    pub content: String,
    pub author: String,   // ← 클라이언트가 아무나 사칭 가능!
    pub tags: Vec<String>,
}

// study6
pub struct CreatePost {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,   // author 필드 삭제
}
```

```rust
let row = repo::insert(&state.db, &body, &auth.username, auth.id).await?;
//                                        ↑ 토큰에서 나온 값
```

**신원 정보는 절대 요청 본문에서 받지 않습니다.** 이건 프레임워크와 무관한 원칙인데, DTO에 필드가 있으면 무심코 쓰게 되므로 **아예 필드를 지우는 게** 확실합니다. `#[serde(deny_unknown_fields)]` 덕분에 `author`를 보내면 422로 거부되기까지 합니다.

---

## 8. 실행 및 검증

| # | 요청 | 응답 |
|---|---|---|
| 1 | `POST /auth/register` `{"username":"while","email":"while@example.com","password":"supersecret123"}` | `201` `{"id":1,"username":"while","email":"while@example.com","created_at":"..."}` |
| 2 | 같은 방식으로 `bob` 가입 | `201` `{"id":2,...}` |
| 3 | `while` 아이디로 재가입 | `409` `{"error":"conflict","message":"이미 사용 중인 아이디 또는 이메일입니다"}` |
| 4 | `{"email":"not-an-email",...}` | `422` `{"error":"validation_failed",...,"details":{"email":["이메일 형식이 아닙니다"]}}` |
| 5 | `POST /auth/login` 비번 틀림 | `401` `{"error":"unauthorized","message":"아이디 또는 비밀번호가 올바르지 않습니다"}` |
| 6 | `POST /auth/login` 없는 아이디 | `401` **같은 메시지** ← user enumeration 방지 |
| 7 | `POST /auth/login` 정상 | `200` 아래 참고 |
| 8 | `GET /auth/me` 토큰 없이 | `401` `{"error":"unauthorized","message":"Authorization 헤더에 Bearer 토큰이 필요합니다"}` |
| 9 | `GET /auth/me` 가짜 토큰 | `401` `{"error":"unauthorized","message":"토큰이 유효하지 않습니다"}` |
| 10 | `GET /auth/me` 정상 토큰 | `200` `{"id":1,"username":"while",...}` |
| 11 | `POST /posts` 토큰 없이 | `401` |
| 12 | `POST /posts` `while` 토큰 | `201` `{"id":1,"title":"Authored","author":"while","author_id":1,...}` |
| 13 | `PATCH /posts/1` **본인** 토큰 | `200` `{"title":"Authored (edited)",...}` |
| 14 | `PATCH /posts/1` **bob** 토큰 | `403` `{"error":"forbidden","message":"본인이 작성한 글만 수정/삭제할 수 있습니다"}` |
| 15 | `DELETE /posts/1` **bob** 토큰 | `403` 동일 |
| 16 | `GET /posts` 토큰 없이 | `200` (공개 라우트는 그대로) |

7번 로그인 응답 전문:

```json
{
  "access_token": "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOjEsInVzZXJuYW1lIjoid2hpbGUiLCJpYXQiOjE3ODkzNjc1NzcsImV4cCI6MTc4OTM3MTE3N30.m_ImsPxhlG2L2FMCg2rr_j2LcXpT9ECIukeADBC_3o0",
  "token_type": "Bearer",
  "expires_in": 3600,
  "user": { "id": 1, "username": "while", "email": "while@example.com", "created_at": "..." }
}
```

페이로드 부분(`eyJzdWIiOjEs...`)을 Base64 디코드하면 그대로 읽힙니다.

```json
{"sub":1,"username":"while","iat":1789367577,"exp":1789371177}
```

**다시 강조: JWT 페이로드는 암호화가 아닙니다.**

**12~15번이 이번 study의 핵심**입니다. 같은 엔드포인트인데 **토큰의 주인에 따라 200과 403이 갈립니다.**

---

## 9. 처음 만난 오류

### 9-1. ⭐ jsonwebtoken 11의 CryptoProvider 패닉

로그인 요청을 보냈더니 HTTP 오류가 아니라 **연결 자체가 끊어졌습니다.**

```
Invoke-WebRequest : The underlying connection was closed: The connection was closed unexpectedly.
```

서버 stderr를 확인하니:

```
thread 'tokio-rt-worker' panicked at jsonwebtoken-11.0.0\src\crypto\mod.rs:124:40:
Could not automatically determine the process-level CryptoProvider from jsonwebtoken crate features.
Call CryptoProvider::install_default() before this point to select a provider manually,
or make sure exactly one of the 'rust_crypto' and 'aws_lc_rs' features is enabled.
```

- jsonwebtoken 11부터 암호화 백엔드를 **직접 골라야** 합니다. `cargo add jsonwebtoken`만 하면 **어느 쪽도 켜지지 않고, 런타임에 패닉**합니다.
- 해결:

```powershell
cargo add jsonwebtoken --features rust_crypto
```

| feature | 특징 |
|---|---|
| `rust_crypto` | **순수 Rust.** C 컴파일러 불필요, 크로스 컴파일·Alpine Docker에 유리 |
| `aws_lc_rs` | C 기반. 더 빠르고 FIPS 인증 경로가 있음 |

study10에서 Docker 이미지를 만들 때 `rust_crypto`가 편하므로 이쪽을 골랐습니다.

**이 사건에서 배울 것 두 가지:**

1. **핸들러에서 패닉하면 HTTP 500이 아니라 연결이 끊어집니다.** 클라이언트는 "서버가 응답을 안 준다"만 봅니다. → study7에서 `CatchPanicLayer`로 500 JSON으로 바꿉니다.
2. **서버 로그를 보지 않으면 원인을 알 수 없습니다.** → study9에서 `tracing`으로 로깅을 제대로 갖춥니다.

### 9-2. sqlx 0.9의 `SqlSafeStr` — 동적 SQL 금지

컬럼 목록을 상수로 빼서 `format!`으로 SQL을 조립했더니:

```
error[E0277]: dynamic SQL strings should be audited for possible injections
   = help: the trait `SqlSafeStr` is not implemented for `&String`
   = note: prefer literal SQL strings with bind parameters or `QueryBuilder`
           To bypass this error, manually audit ... and wrap with `AssertSqlSafe()`.
   = note: this trait is only implemented for `&'static str`, not all `&str`
```

- **sqlx 0.9부터 `query`/`query_as`는 `&'static str`(리터럴)만 받습니다.** 런타임에 만들어진 문자열은 타입 수준에서 거부됩니다.
- 진짜 동적 SQL이 필요하면 `sqlx::QueryBuilder`를 쓰거나, 직접 감사한 뒤 `AssertSqlSafe()`로 감쌉니다.
- **에러 메시지 자체가 안내문**이라는 게 인상적입니다. "이 트레이트는 `&str` 전부가 아니라 `&'static str`에만 구현되어 있다"는 각주까지 달아줍니다.

> 컴파일러가 SQL 인젝션 벡터를 원천 차단하는 설계입니다. 처음엔 "왜 안 되지?" 싶지만, 문자열 연결로 SQL을 만드는 습관 자체를 막아줍니다.

### 9-3. argon2 0.6의 API 변경

블로그 예제 대부분은 argon2 0.5 기준입니다.

```rust
// 0.5 (옛날)
let salt = SaltString::generate(&mut OsRng);
let hash = Argon2::default().hash_password(password.as_bytes(), &salt)?;

// 0.6 (현재) — 솔트를 알아서 만들어 준다
let hash: PasswordHash = Argon2::default().hash_password(password.as_bytes())?;
```

- `SaltString`, `rand_core`가 최상위에서 사라졌고, `hash_password`가 **1인자**가 되었습니다.
- 반환 타입이 제네릭(`PasswordHasher<H>`)이라 **`let hash: PasswordHash = ...` 타입 주석이 필요**합니다. 없으면 "type annotations needed".
- 확인 방법: `cargo doc --open`, 또는 `~/.cargo/registry/src/.../argon2-0.6.0/src/lib.rs`를 직접 열어보기. **의존성 소스가 로컬에 다 있다는 게 Rust의 큰 장점**입니다.

---

## 10. 이번 study 정리

- **인증(401)과 인가(403)는 다른 문제다.** extractor는 "누구인지"까지, 소유권 판단은 핸들러/서비스에서.
- 비밀번호는 **Argon2**로 해시하고, 느린 연산이므로 **반드시 `spawn_blocking`** 으로 워커 스레드를 양보한다.
- DB 타입(`UserRow`)에 `Serialize`를 **구현하지 않으면** 비밀번호 해시 유출이 컴파일 타임에 막힌다.
- JWT 페이로드는 **암호화가 아니다.** 비밀을 담지 말고, 즉시 무효화가 필요하면 별도 장치를 둔다.
- **`FromRequestParts` 구현체가 Axum의 Guard다.** 핸들러 인자에 `AuthUser`를 적는 것만으로 보호된다.
- 로그인 실패 메시지는 **"없는 아이디"와 "틀린 비밀번호"를 구분하지 않는다.**
- 중복 확인은 SELECT가 아니라 **DB UNIQUE 제약**에 맡기고 `is_unique_violation()`으로 409로 번역한다.
- 신원 정보(작성자)는 **요청 본문이 아니라 토큰**에서 가져온다.

다음: [study7.md](study7.md) — tower 미들웨어 계층으로 로깅·CORS·타임아웃·패닉 처리 붙이기. (고급 2)
