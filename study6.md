# study6 — DB · API 연동 (CRUD)

> 🎯 **목표**: DB(sqlx + SQLite)를 연결해 게시글 **목록·상세·등록·수정·삭제** CRUD를 만들고, 오류 응답과 JSON 이름을 **공통 API 스펙 v1**에 맞추며, 연결 정보를 환경변수로 분리한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [demo/src/db.rs](demo/src/db.rs), [demo/src/posts/](demo/src/posts/), [demo/src/error.rs](demo/src/error.rs), [demo/src/middleware.rs](demo/src/middleware.rs), [demo/src/config.rs](demo/src/config.rs), [demo/migrations/](demo/migrations/) |
| 이 단계의 코드 변경 | `AppError`의 JSON 본문을 스펙 v1 `{ error: { code, message, fields, requestId } }`로, 응답 JSON을 camelCase로 변경 (테스트 15개 통과) |
| 버전 | axum 0.8.9, **sqlx 0.9** (`query`/`query_as`는 리터럴 SQL만 받음) |

---

## ① 목표

1. DB 연결 설정(`create_if_missing`, WAL, `busy_timeout`)과 마이그레이션을 이해한다.
2. DB 행(`PostRow`)과 API 응답(`Post`)을 **별개 타입**으로 두고 경계에서 변환한다.
3. `INSERT … RETURNING`, `COALESCE` 부분 수정, `rows_affected` 삭제를 쓴다.
4. 오류를 `Result<T, AppError>`로 다루고, 응답 본문을 스펙 v1 모양으로 만든다.
5. 연결 정보·비밀 값을 환경변수로 분리한다.

---

## ② 📖 이론

### sqlx는 ORM이 아니다

SQL을 **직접 쓰고**, 결과를 구조체로 받아 주는 얇은 도구다. 어떤 쿼리가 나가는지 100% 보이는 대신, 컬럼 매핑 실수는 **런타임**에 드러난다(`query!` 매크로는 컴파일 타임 검증이지만 DB 연결/`.sqlx` 캐시가 필요해 학습 단계에서는 런타임 함수 `query`/`query_as`를 쓴다).

### 3-타입 구조 — DB, 요청, 응답을 섞지 않는다

```
요청 JSON ──▶ CreatePost / UpdatePost   (검증 규칙이 붙은 입력 DTO)
                    │
              repo (SQL)  ◀──▶  PostRow   (DB 한 행 그대로: tags는 JSON 문자열)
                                   │ From
                                   ▼
                               Post  ──▶ 응답 JSON  (camelCase, tags는 배열)
```

비밀번호 해시 같은 필드는 **응답 타입에 아예 없게** 만들어 실수로 새어 나갈 일을 구조적으로 막는다(study7).

### 오류는 값이다

```rust
async fn get_one(...) -> AppResult<Json<Post>>        // Result<Json<Post>, AppError>
```

실패는 예외가 아니라 **반환 타입에 적힌 값**이다. `AppError`가 `IntoResponse`를 구현하므로 `?` 한 글자로 sqlx 오류가 500 JSON까지 흘러간다. 새 오류 갈래를 추가하면 응답 매핑 `match`가 **컴파일 에러로 누락을 알려 준다.**

### 공통 API 스펙 v1 — 프론트와의 계약

프론트가 백엔드를 바꿔 끼워도 코드를 고치지 않으려면 **경로·응답·오류 모양**이 같아야 한다. 이 단계에서 맞춘 것은 **오류 본문**과 **JSON 필드 이름**이다(경로·목록 응답의 차이는 아래 ④에 기록).

```json
{ "error": { "code": "VALIDATION_ERROR", "message": "입력값을 확인하세요.",
             "fields": { "title": ["제목은 1~100자여야 합니다"] },
             "requestId": "772252c7-f863-4fa0-aef6-0e28a18cd0a0" } }
```

---

## ③ 🛠️ 실기

### 3-1. 연결 설정 — [demo/src/db.rs](demo/src/db.rs) (SQLite 부분)

```rust
pub type Db = sqlx::Sqlite;
pub type DbPool = sqlx::Pool<Db>;

pub async fn init(database_url: &str) -> anyhow::Result<DbPool> {
    let in_memory = database_url.contains(":memory:") || database_url.contains("mode=memory");

    let mut options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5));

    if !in_memory {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(if in_memory { 1 } else { 5 })
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
```

| 코드 | 설명 |
|---|---|
| `create_if_missing(true)` | **기본값이 false**라서 켜지 않으면 첫 실행부터 `unable to open database file` |
| `busy_timeout(5s)` | 쓰기 락을 기다리는 시간. 기본 0이면 동시 쓰기에서 즉시 `database is locked` |
| `journal_mode(Wal)` | 읽기와 쓰기가 서로 막지 않는다. **SQLite를 서버에서 쓸 때 사실상 필수** |
| `in_memory`일 때 `max_connections(1)` | 메모리 DB는 **연결마다 별개의 DB**. 풀 연결이 둘 이상이면 테스트가 랜덤하게 `no such table` |
| `sqlx::migrate!("./migrations")` | **컴파일 시점에** `.sql` 파일을 바이너리에 박아 넣는다. 배포 이미지에 `migrations/` 폴더가 없어도 된다 |
| `pub type Db`, `DbPool` | **DB 종류가 나오는 유일한 자리.** 나머지 코드는 별칭만 쓴다 (study11에서 PostgreSQL로 바꿀 때 이득) |

### 3-2. 마이그레이션 — 기존 파일은 고치지 않고 **새 파일을 추가**

```sql
-- migrations/0001_create_posts.sql
CREATE TABLE IF NOT EXISTS posts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    author     TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',     -- SQLite에는 배열이 없다. JSON 문자열로 저장
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
```

`0002_create_users.sql`(회원 + `posts.author_id`), `0003_add_thumbnail.sql`(`thumbnail`)가 이어진다. **적용된 마이그레이션을 수정하면** sqlx가 체크섬 불일치(`VersionMismatch`)로 거부한다 — 운영이라면 새 파일을 추가한다.

### 3-3. 모델 — [demo/src/posts/model.rs](demo/src/posts/model.rs)

```rust
/// API 응답으로 나가는 게시글.  (스펙 v1: 응답 JSON 필드는 camelCase)
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    pub author_id: Option<i64>,
    pub tags: Vec<String>,
    pub thumbnail: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// DB의 posts 한 행. tags가 JSON 문자열이라 API 모양과 다르다.
#[derive(Debug, Clone, FromRow)]
pub struct PostRow { /* id, title, content, author, author_id, tags: String, thumbnail, created_at, updated_at */ }

impl From<PostRow> for Post {
    fn from(row: PostRow) -> Self {
        Self { /* ... */
            tags: serde_json::from_str(&row.tags).unwrap_or_default(),   // 깨진 JSON이어도 500 대신 빈 배열
        }
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,
    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub content: String,
    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}

/// 모든 필드가 Option → 부분 수정(PATCH)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePost { pub title: Option<String>, pub content: Option<String>, pub tags: Option<Vec<String>> /* + validate */ }
```

| 코드 | 설명 |
|---|---|
| `#[serde(rename_all = "camelCase")]` | `author_id → authorId`, `created_at → createdAt`. **직렬화 경계에서만** 이름이 바뀐다. Rust 코드는 snake_case 그대로 |
| `CreatePost`에 `author` 필드가 **없다** | 작성자는 클라이언트가 정하는 값이 아니라 **토큰에서 나온다** (study7) |
| `UpdatePost`의 `Option` | `None`이면 "안 보냈다" → 기존 값 유지 (PATCH 의미) |
| `ToSchema` | OpenAPI 문서 자동 생성용 (study14) |

### 3-4. 저장소(repo) — [demo/src/posts/repo.rs](demo/src/posts/repo.rs)

```rust
/// 단건 조회. 없으면 Ok(None)
pub async fn find(db: &DbPool, id: i64) -> Result<Option<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(
        "SELECT id, title, content, author, author_id, tags, thumbnail, created_at, updated_at
         FROM posts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// 생성. RETURNING으로 INSERT와 조회를 한 번에 끝낸다.
pub async fn insert(db: &DbPool, input: &CreatePost, author: &str, author_id: i64)
    -> Result<PostRow, sqlx::Error>
{
    let now: DateTime<Utc> = Utc::now();
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());

    sqlx::query_as::<_, PostRow>(
        "INSERT INTO posts (title, content, author, author_id, tags, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, title, content, author, author_id, tags, thumbnail, created_at, updated_at",
    )
    .bind(&input.title).bind(&input.content).bind(author).bind(author_id)
    .bind(tags).bind(now).bind(now)
    .fetch_one(db)
    .await
}

/// 부분 수정. COALESCE($1, title)는 "바인딩 값이 NULL이면 기존 값을 쓴다"
pub async fn update(db: &DbPool, id: i64, input: &UpdatePost) -> Result<Option<PostRow>, sqlx::Error> {
    let tags = input.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".to_string()));

    sqlx::query_as::<_, PostRow>(
        "UPDATE posts
         SET title      = COALESCE($1, title),
             content    = COALESCE($2, content),
             tags       = COALESCE($3, tags),
             updated_at = $4
         WHERE id = $5
         RETURNING id, title, content, author, author_id, tags, thumbnail, created_at, updated_at",
    )
    .bind(input.title.as_ref()).bind(input.content.as_ref()).bind(tags)
    .bind(Utc::now()).bind(id)
    .fetch_optional(db)
    .await
}

pub async fn delete(db: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM posts WHERE id = $1").bind(id).execute(db).await?;
    Ok(result.rows_affected() > 0)
}
```

| 코드 | 설명 |
|---|---|
| `$1, $2 …` | 바인딩 자리. **값을 SQL 문자열에 끼워 넣지 않는다 → SQL 인젝션 방어.** (SQLite도 `$1`을 받는다 — study11에서 PostgreSQL과 같은 SQL을 쓰는 이유) |
| `fetch_optional` | 0건이면 `Ok(None)` → 핸들러가 404로 번역 |
| `RETURNING …` | 저장된 행을 되돌려 받는다. INSERT 후 SELECT를 한 번 더 하지 않는다 |
| `COALESCE($1, title)` | 필드 조합마다 SQL을 따로 만들지 않아도 PATCH가 된다 |
| `rows_affected() > 0` | 실제로 지워졌는지 확인 → 없는 id면 404 |
| `$4 ← Utc::now()` | 수정 시각은 서버가 정한다 |

> ⚠️ **sqlx 0.9에서 `format!`으로 SQL을 만들면 컴파일 에러**(`SqlSafeStr`)다. 동적 조건(검색·필터)은 `QueryBuilder`로 쓴다. 이는 SQL 인젝션을 타입으로 막는 설계다. 검색 필터 코드는 study8과 [demo/src/posts/repo.rs](demo/src/posts/repo.rs)의 `push_filters`를 참고.

### 3-5. 핸들러 — [demo/src/posts/handler.rs](demo/src/posts/handler.rs)

```rust
pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Post>> {
    repo::find(&state.db, id).await?                       // sqlx 오류는 ? 로 AppError::Database → 500
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))    // 없으면 404
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,                                        // 토큰이 없거나 틀리면 여기서 401 (study7)
    ValidatedJson(body): ValidatedJson<CreatePost>,        // 본문 extractor는 마지막
) -> AppResult<impl IntoResponse> {
    let row = repo::insert(&state.db, &body, &auth.username, auth.id).await?;
    let location = format!("/posts/{}", row.id);
    Ok((StatusCode::CREATED, [(header::LOCATION, location)], Json(Post::from(row))))
}
```

`update`는 먼저 `ensure_owner`(없으면 404, 남의 글이면 403)를 거친 뒤 `repo::update`, `delete`는 성공 시 **204 + 본문 없음**.

### 3-6. ⭐ 오류 응답을 스펙 v1 모양으로 — [demo/src/error.rs](demo/src/error.rs)

```rust
#[derive(Debug, Error)]
pub enum AppError {
    #[error("{resource} {id} not found")]          NotFound { resource: &'static str, id: String },
    #[error("{0}")]                                BadRequest(String),
    #[error("{message}")]                          InvalidBody { status: StatusCode, message: String },
    #[error("validation failed")]                  Validation(validator::ValidationErrors),
    #[error("{0}")]                                Unauthorized(String),
    #[error("아이디 또는 비밀번호가 올바르지 않습니다")] InvalidCredentials,
    #[error("{0}")]                                Forbidden(String),
    #[error("이미 사용 중인 아이디 또는 이메일입니다")]   EmailTaken,
    #[error("{0}")]                                Conflict(String),
    #[error("이 경로에서 지원하지 않는 HTTP 메서드입니다")] MethodNotAllowed,
    #[error(transparent)]                          Database(#[from] sqlx::Error),
    #[error(transparent)]                          Internal(#[from] anyhow::Error),
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope { pub error: ErrorDetail }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDetail {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub fields: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub request_id: Option<String>,
}
```

변환표 (`status()` / `code()`):

| `AppError` | 상태 | `error.code` |
|---|---|---|
| `NotFound` | 404 | `NOT_FOUND` |
| `BadRequest` | 400 | `BAD_REQUEST` |
| `InvalidBody` (axum의 400/422) | **400** (422는 400으로 정규화) | `BAD_REQUEST` |
| `InvalidBody` (413 / 415) | 413 / 415 | `PAYLOAD_TOO_LARGE` / `UNSUPPORTED_MEDIA_TYPE` |
| `Validation` | **400** | `VALIDATION_ERROR` (+ `fields`) |
| `Unauthorized` | 401 | `UNAUTHORIZED` |
| `InvalidCredentials` | 401 | `INVALID_CREDENTIALS` |
| `Forbidden` | 403 | `FORBIDDEN` |
| `EmailTaken` | 409 | `EMAIL_TAKEN` |
| `Conflict` | 409 | `CONFLICT` |
| `MethodNotAllowed` | 405 | `METHOD_NOT_ALLOWED` |
| `Database` / `Internal` | 500 | `INTERNAL` (원인은 **로그에만**, 응답 메시지는 고정 문구) |

`requestId`는 핸들러가 요청 헤더를 모르기 때문에 **응답이 나가는 길목의 미들웨어**가 채운다 ([demo/src/middleware.rs](demo/src/middleware.rs)):

```rust
pub async fn error_envelope(request: Request, next: Next) -> Response {
    let request_id = request.headers().get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok()).map(str::to_owned);

    let response = next.run(request).await;
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;                                   // 성공 응답은 건드리지 않는다
    }

    let (mut parts, body) = response.into_parts();
    let bytes = to_bytes(body, ERROR_BODY_LIMIT).await.unwrap_or_default();

    // 이미 { "error": { ... } } 이면 requestId만 채우고, 아니면(빈 본문 408, ServeDir의 404 …) 새로 감싼다
    let mut value = serde_json::from_slice::<Value>(&bytes).ok()
        .filter(|v| v.get("error").is_some_and(|e| e.is_object()))
        .unwrap_or_else(|| { /* ErrorEnvelope::new(code_for_status(status), message) */ });

    if let (Some(id), Some(error)) = (request_id, value.get_mut("error")) {
        error["requestId"] = Value::String(id);
    }

    parts.headers.remove(header::CONTENT_LENGTH);          // 본문이 바뀌었으니 길이를 다시 계산하게
    parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Response::from_parts(parts, Body::from(value.to_string()))
}
```

이 미들웨어는 [demo/src/lib.rs](demo/src/lib.rs)에서 **`CompressionLayer`보다 안쪽, `CatchPanicLayer`·`TimeoutLayer`보다 바깥**에 둔다 (이유는 study8의 순서 규칙).

### 3-7. 환경변수로 분리 — [demo/src/config.rs](demo/src/config.rs)

```rust
pub struct Config { pub database_url: String, pub jwt_secret: String, pub bind_addr: String, pub upload_dir: String }

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            database_url: env_or("DATABASE_URL", DEFAULT_DATABASE_URL),
            jwt_secret:   env_or("JWT_SECRET", DEV_JWT_SECRET),
            bind_addr:    env_or("BIND_ADDR", "127.0.0.1:3000"),
            upload_dir:   env_or("UPLOAD_DIR", "uploads"),
        };
        config.validate()?;          // 너무 짧은 비밀키는 서버가 뜨기 전에 거절
        Ok(config)
    }
}
```

설정은 **한 곳에서, 시작할 때 한 번** 읽고 검증한다. "이 앱이 어떤 환경변수를 쓰는가"가 `Config` 하나로 보인다. 키 이름 목록은 [demo/.env.example](demo/.env.example).

### 3-8. 실행 결과 (실제 서버 + curl)

```
$ curl -X POST .../auth/register -d '{"username":"alice","email":"alice@example.com","password":"supersecret123"}'
{"id":1,"username":"alice","email":"alice@example.com","createdAt":"2026-11-22T15:12:17.395524Z"}       ← camelCase

$ curl -X POST .../auth/register -d '{"username":"alice","email":"a2@example.com", ...}'                  # 중복
{"error":{"code":"EMAIL_TAKEN","message":"이미 사용 중인 아이디 또는 이메일입니다","requestId":"cec4858c-..."}}

$ curl -i -X POST .../posts -H "authorization: Bearer $TOKEN" -d '{"title":"Hello PG","content":"Rust Axum Postgres","tags":["rust","axum"]}'
HTTP/1.1 201 Created
location: /posts/1
x-request-id: 084b1311-03a6-4713-b51d-ff9fa9e03e41
{"id":1,"title":"Hello PG","content":"Rust Axum Postgres","author":"alice","authorId":1,"tags":["rust","axum"],
 "thumbnail":null,"createdAt":"2026-11-22T15:12:18.036154Z","updatedAt":"2026-11-22T15:12:18.036154Z"}

$ curl ".../posts?q=hello&tag=RUST"
{"items":[{...}],"page":1,"perPage":10,"total":1,"totalPages":1,"hasNext":false}

$ curl -X POST .../posts -d '{"title":"","content":""}' -H "authorization: Bearer $TOKEN"                     # 검증 실패
{"error":{"code":"VALIDATION_ERROR","fields":{"content":["본문은 1~5000자여야 합니다"],"title":["제목은 1~100자여야 합니다"]},
          "message":"입력값을 확인하세요.","requestId":"772252c7-f863-4fa0-aef6-0e28a18cd0a0"}}               → 400

$ curl -X DELETE .../posts/1 -H "authorization: Bearer $TOKEN"                                                → 204
```

AppError를 거치지 않는 4xx·5xx도 같은 모양이 된다 (`error_envelope`의 효과):

```
GET /nope  (accept-encoding: gzip)  → 404 {"error":{"code":"NOT_FOUND","message":"route (unknown) not found","requestId":"f391c147-..."}}   (gzip 응답 + 본문 재작성 동작)
GET /panic  (x-request-id: panic-1) → 500 {"error":{"code":"INTERNAL","message":"서버 내부 오류가 발생했습니다.","requestId":"panic-1"}}
GET /slow                           → 408 {"error":{"code":"REQUEST_TIMEOUT","message":"요청 처리 시간이 초과되었습니다.","requestId":"9b1ac04c-..."}}
DELETE /health                      → 405 {"error":{"code":"METHOD_NOT_ALLOWED", ...}}
POST /auth/login  (본문 '{bad')     → 400 {"error":{"code":"BAD_REQUEST","message":"Failed to parse the request body as JSON: ..."}}
```

---

## ④ 🔗 연동 포인트 — 공통 API 스펙 v1과 맞추기

### 4-1. 이번에 맞춘 것

| 스펙 v1 | 상태 |
|---|---|
| 오류 본문 `{ error: { code, message, fields, requestId } }` (§5) | ✅ 맞춤 (`AppError` + `error_envelope`) |
| 오류 코드표 (`VALIDATION_ERROR` 400, `UNAUTHORIZED`/`INVALID_CREDENTIALS` 401, `FORBIDDEN` 403, `NOT_FOUND` 404, `EMAIL_TAKEN` 409, `PAYLOAD_TOO_LARGE` 413, `INTERNAL` 500) | ✅ 맞춤 (`RATE_LIMITED` 429는 속도 제한 기능이 없어 미구현) |
| 응답 JSON camelCase (§2) | ✅ 맞춤 (`#[serde(rename_all = "camelCase")]`) |
| `X-Request-Id` 응답 헤더 = 오류 본문의 `requestId` (§7) | ✅ 같은 값 (테스트로 검증) |
| 토큰 만료 1시간 (§6) | ✅ (`TOKEN_TTL_SECONDS = 3600`) |

### 4-2. 확인 결과 기록 — 경로와 목록 응답 형식 (Notion "정리 영역"에 같은 표를 기록했다)

**`demo/` (게시판 API, 포트 3000)**

| 항목 | 스펙 v1 | 현재 `demo/` | 맞춤 |
|---|---|---|---|
| Base URL | `/api` | 없음 (`/posts`, `/auth/*`) | ❌ |
| 헬스체크 | `GET /health` | `GET /health` | ✅ |
| 회원가입 | `POST /api/auth/signup` `{email,password,nickname}` | `POST /auth/register` `{username,email,password}` | ❌ |
| 로그인 | `POST /api/auth/login` `{email,password}` | `POST /auth/login` `{username,password}` | ❌ |
| 로그아웃 / 내 정보 | `POST /api/auth/logout`, `GET /api/auth/me` | `GET /auth/me`만 (로그아웃 없음 — JWT는 서버 상태가 없다) | △ |
| 목록 | `GET /api/posts?q&page&limit&sort` → `{ data: [...], meta: { page, limit, total, totalPages, q } }` | `GET /posts?q&tag&page&per_page` → `{ items, page, perPage, total, totalPages, hasNext }` | ❌ |
| 쿼리 한도 | `limit` 기본 10, **최대 50**, `sort` (`-createdAt` 등) | `per_page` 기본 10, **최대 100**, `sort` 없음, `tag` 추가 | ❌ |
| 단건 / 생성 응답 | `{ data: {...} }`, 작성자는 `author: { id, nickname }`, `commentCount` | 객체 그대로, `author: "이름"` + `authorId` | ❌ |
| 글 필드 이름 | `title`, **`body`**, `tags` | `title`, **`content`**, `tags` (+ `thumbnail`) | ❌ |
| 생성 | 201 + `Location: /api/posts/{id}` | 201 + `Location: /posts/{id}` | △ (접두어) |
| 수정 / 삭제 | `PATCH` 200 / `DELETE` 204 | 동일 | ✅ |
| 댓글 | `/api/posts/{id}/comments`, `DELETE /api/comments/{id}` | **없음** | ❌ |
| 로그인 응답 | `{ data: { user, accessToken } }` | `{ accessToken, tokenType, expiresIn, user }` | ❌ |
| CORS 출처 | 환경변수 `CORS_ORIGIN` | 코드에 `http://localhost:5173` 고정 | ❌ |
| 문서 경로 | `/api-docs` | `/docs` (Swagger UI) + `/api-docs/openapi.json` | △ |
| 오류 본문 / 코드 / 요청 ID / camelCase | 위 4-1 | 맞춤 | ✅ |

**`project/` (최종 프로젝트, 포트 4000)**

| 항목 | 스펙 v1 | 현재 `project/` | 맞춤 |
|---|---|---|---|
| Base URL | `/api` | `/api` | ✅ |
| 헬스체크 | `/health` (접두어 없이) | `/api/health` | △ |
| 회원가입 | `/api/auth/signup` | `/api/auth/register` | ❌ |
| 목록 | `{ data, meta }`, `page`/`limit` | `{ items, page, per_page, total, total_pages }`, `per_page` (snake_case) | ❌ |
| 오류 | `{ error: { code, … } }` | `{ error: "code", message, details? }` (**옛 형태**) | ❌ |
| 인증 | Bearer 또는 (같은 도메인 풀스택이면) httpOnly 쿠키 | httpOnly 쿠키 | ✅ (스펙 §6이 허용) |

> 이 표가 곧 **다음 작업 목록**이다. 오류 본문을 먼저 맞춘 이유: 프론트가 가장 먼저 부딪히는 차이이고, **`AppError` 한 곳만** 바꾸면 되기 때문이다. 나머지(경로 접두어, `{data, meta}` 래핑, 댓글)는 [studyMidpoint](studyMidpoint.md)·[study14](study14.md)에서 다룬다.

### 4-3. 연결 정보는 환경변수로

| 키 | 용도 | 기본값(개발) | 운영 |
|---|---|---|---|
| `DATABASE_URL` | DB 주소 | `sqlite://data.db?mode=rwc` | 실행 시 주입 (PostgreSQL이면 `postgres://…`) |
| `JWT_SECRET` | 토큰 서명 키 | 개발용 문자열 (**운영에서 쓰면 경고**) | **이미지에 굽지 않고** 실행 시 주입 |
| `BIND_ADDR` | 바인딩 주소 | `127.0.0.1:3000` | 컨테이너는 `0.0.0.0:3000` |
| `UPLOAD_DIR` | 업로드 폴더 | `uploads` | 볼륨 경로 |
| `RUST_LOG` | 로그 레벨 | `demo=debug,…` | `info` |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| `unable to open database file` | `create_if_missing` 기본값이 **false** | `.create_if_missing(true)` 또는 URL에 `?mode=rwc` |
| `database is locked` (가끔, 동시 요청에서 랜덤) | 동시 쓰기 + `busy_timeout` 0 + 기본 저널 모드 | WAL + `busy_timeout`. 근본적으로 SQLite는 **동시에 쓰는 트랜잭션이 하나뿐** — 쓰기가 많으면 PostgreSQL (study11) |
| 테스트가 랜덤하게 `no such table` | in-memory SQLite는 **연결마다 별개의 DB**인데 풀이 2개 이상 | 메모리 DB는 `max_connections(1)` |
| `error[E0277]: dynamic SQL strings should be audited for possible injections` | sqlx 0.9는 `query`/`query_as`에 **`&'static str`만** 받음. `format!`로 SQL 조립 | 값은 `$1` 바인딩, 동적 조건은 `QueryBuilder`. 에러 메시지 자체가 안내문이다 |
| `ColumnNotFound("tags")` / `ColumnDecode { index: "created_at" }` | `query_as`는 **런타임에** 매핑. SELECT에 컬럼을 안 넣었거나 형식이 다름 | `SELECT *` 대신 컬럼을 명시. 컴파일 타임 검증이 필요하면 `query_as!` + `cargo sqlx prepare` |
| `VersionMismatch(1)` | **이미 적용된** 마이그레이션 파일을 수정함 (체크섬 불일치) | 개발 중이면 `data.db*` 삭제 후 재실행, 운영이면 **새 마이그레이션 파일 추가** |
| 검증 실패 응답이 422인 줄 알았는데 400 | 스펙 v1은 `VALIDATION_ERROR`를 400으로 정함. axum 기본은 422 | `AppError::status()`가 `InvalidBody`의 422를 400으로 정규화 (study5) |
| 오류 JSON에 `requestId`가 없음 | `error_envelope`가 `SetRequestIdLayer`보다 **바깥**에 있어서 요청에 아직 `x-request-id`가 없음 | 순서: `SetRequestId` → … → `Compression` → `error_envelope` → `CatchPanic` → `Timeout` (아래 study8) |
| gzip 요청에서만 오류 본문 파싱이 깨짐 | `error_envelope`가 **압축된 바이트**를 JSON으로 읽으려 함 | `CompressionLayer`를 `error_envelope`보다 **바깥**으로 |
| 500 응답에 DB 오류 문구가 그대로 노출 | `Database(err)`를 `to_string()`으로 응답에 넣음 | 응답은 고정 문구, 원인은 `tracing::error!`로 **로그에만** |
| `.env`의 `RUST_LOG`가 안 먹음 | `EnvFilter`는 초기화 시점에 한 번만 읽음 | `load_dotenv()`를 `init_tracing()`보다 먼저 |

---

## ⑥ 직접 해보기

1. **스펙 v1에 한 걸음 더**: `GET /posts`의 쿼리 이름 `per_page`를 `limit`으로 바꾸고 최대값을 50으로 맞춘다 (`PageQuery`의 `#[serde(rename)]` 또는 필드명, `validate` 범위, 테스트). 프론트가 바뀐 것이 `?limit=`뿐임을 확인한다.
2. **`/api` 접두어 붙이기**: [demo/src/lib.rs](demo/src/lib.rs)에서 `.nest("/posts", …)`, `.nest("/auth", …)`를 `/api` 아래로 옮기고, 깨지는 테스트를 고친다. (`/health`는 스펙대로 접두어 없이 남긴다)
3. **새 오류 갈래**: `RATE_LIMITED`(429)용 `AppError::RateLimited` 변형을 추가하고 `status()`/`code()`를 채운다. `match`가 **빠뜨린 곳을 컴파일러가 알려 주는지** 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ `create_if_missing`, WAL, `busy_timeout`, in-memory `max_connections(1)`의 이유를 각각 설명할 수 있다
- ☐ 마이그레이션은 **새 파일 추가**가 원칙이고 적용된 파일 수정은 `VersionMismatch`라는 것을 안다
- ☐ `PostRow` ↔ `Post` ↔ `CreatePost`를 나누는 이유를 설명할 수 있다
- ☐ 목록·상세·등록·수정·삭제를 `curl`로 직접 호출해 확인했다
- ☐ `INSERT … RETURNING`, `COALESCE`, `rows_affected`가 각각 무엇을 해결하는지 안다
- ☐ `AppError`의 갈래 → 상태 코드·`code` 표를 보고 새 갈래를 추가할 수 있다
- ☐ 연결 정보·비밀 값이 코드가 아니라 환경변수에 있다
- ☐ `AppError`의 JSON 본문만 `{ error: { code, message, fields, requestId } }`로 맞추면 된다 (이미 enum + `IntoResponse`)
- ☐ 경로와 목록 응답 형식을 확인해 이 페이지 정리 영역에 기록
- ☐ 응답 JSON은 `#[serde(rename_all = "camelCase")]`

---

## ⑧ 다음 단계 연결

CRUD가 돌아가지만 **누구나** 글을 쓰고 지울 수 있다. [study7](study7.md)에서는 회원가입·로그인(Argon2 + JWT)과 `AuthUser` extractor로 쓰기 API를 보호하고, 본인 글만 고치게(403) 하며, CORS·XSS·CSRF의 기초를 다룬다.
