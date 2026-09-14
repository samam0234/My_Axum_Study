# 종합 프로젝트 — 구조를 바꿔서 다시 만들기 (studyFinal)

study1~10에서 만든 `demo/`는 **한 크레이트 안에 모든 모듈**이 있고, **sqlx로 SQL을 직접 쓰고**, **Bearer 헤더로 인증**하는 JSON API였습니다.

이번에는 같은 게시판을 **일부러 다른 구조로** 다시 만듭니다. 목적은 "더 좋은 방법"을 찾는 게 아니라, **구조 선택이 무엇을 바꾸는지 몸으로 아는 것**입니다.

**만드는 것**: `project/` — 로그인해서 글을 쓰고 읽는 **완결된 미니 웹사이트**

---

## 1. 무엇을 바꿨나

| | `demo/` (study1~10) | `project/` (studyFinal) |
|---|---|---|
| **크레이트 구성** | 단일 크레이트 (lib + bin) | **Cargo 워크스페이스, 크레이트 2개** |
| **계층 분리** | 모듈(`mod`)로 분리 | **크레이트로 분리** — 컴파일러가 강제 |
| **DB 접근** | sqlx (SQL 직접 작성) | **SeaORM** (엔티티 + 쿼리 빌더) |
| **마이그레이션** | `.sql` 파일 | **Rust 코드** (`Table::create()`) |
| **오류 타입** | `AppError` 하나가 도메인+HTTP 겸함 | **`DomainError`(HTTP 모름) + 번역 계층** |
| **소유권 검사** | 핸들러에서 `ensure_owner()` 호출 | **서비스 안에서** 자동 |
| **인증 전달** | `Authorization: Bearer` | **HttpOnly 쿠키** (+ Bearer도 허용) |
| **프론트엔드** | 없음 (API만) | **vanilla JS SPA 내장** |
| **문서화** | Swagger UI | 없음 (화면이 곧 문서) |

---

## 2. ⭐ Cargo 워크스페이스 — 계층을 크레이트로 나눈다

```
project/
├─ Cargo.toml              ← 워크스페이스 정의
└─ crates/
   ├─ domain/              ← 비즈니스 규칙. axum이 의존성에 없다.
   └─ web/                 ← HTTP 표현. domain을 의존한다.
```

### 왜 모듈이 아니라 크레이트인가

`demo/`에서도 `posts::repo`(DB)와 `posts::handler`(HTTP)를 나눴습니다. **하지만 같은 크레이트 안이라 서로를 자유롭게 부를 수 있습니다.** 바쁠 때 `repo.rs`에 `StatusCode`를 하나 쓰기 시작하면 계층이 조용히 무너집니다.

크레이트로 나누면 **의존 방향이 물리적으로 강제**됩니다.

```toml
# crates/domain/Cargo.toml — axum이 없다
[dependencies]
sea-orm.workspace = true
thiserror.workspace = true
# axum 없음 → 쓰고 싶어도 못 쓴다

# crates/web/Cargo.toml
[dependencies]
domain = { path = "../domain" }   # 한 방향
axum.workspace = true
```

**domain에서 `StatusCode`를 쓰려고 하면 컴파일이 안 됩니다.** 규율이 아니라 빌드 시스템이 지켜줍니다.

### 워크스페이스 의존성 공유

```toml
[workspace]
resolver = "3"
members = ["crates/domain", "crates/web"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
axum = "0.8.9"
sea-orm = { version = "1.1", default-features = false, features = [...] }
# ...
```

각 크레이트는 이렇게 받아 씁니다.

```toml
[dependencies]
axum.workspace = true
```

- **버전이 한곳에서 관리**됩니다. 크레이트가 10개로 늘어나도 `serde` 버전이 어긋날 일이 없습니다.
- `target/`을 공유하므로 **같은 의존성을 한 번만 컴파일**합니다. Rust에서 이건 큰 이점입니다.
- `[profile.release]`도 워크스페이스 루트에만 적습니다 (멤버 크레이트의 프로필 설정은 무시됩니다).

| | Nest | Spring | Rust 워크스페이스 |
|---|---|---|---|
| 대응 개념 | monorepo (`nest generate library`) | 멀티 모듈 Gradle | Cargo workspace |
| 의존 방향 강제 | 설정으로 (eslint 규칙 등) | 모듈 의존성 선언 | **`Cargo.toml`의 의존성 = 곧 강제** |

---

## 3. SeaORM vs sqlx

### 엔티티는 "테이블 그 자체"다

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub title: String,
    #[sea_orm(column_type = "Text")]
    pub body: String,
    pub author_id: i32,
    pub created_at: ChronoDateTimeUtc,
    pub updated_at: ChronoDateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::AuthorId",
        to = "super::user::Column::Id",
        on_delete = "Cascade"
    )]
    Author,
}
```

sqlx의 `#[derive(FromRow)]`는 **"쿼리 결과를 담는 그릇"** 일 뿐이었습니다. SeaORM 엔티티는 **테이블 정의 + 관계**이고, 여기서 쿼리 빌더와 조인이 파생됩니다.

`DeriveEntityModel` 하나가 `Entity`, `Column`, `PrimaryKey`, `ActiveModel` 등을 전부 생성합니다.

### 쿼리에 SQL 문자열이 없다

```rust
// demo/ (sqlx) — SQL을 직접
sqlx::query_as::<_, PostRow>(
    "SELECT ... FROM posts WHERE id = ?"
).bind(id).fetch_optional(db).await

// project/ (SeaORM) — 타입으로
post::Entity::find_by_id(id)
    .find_also_related(user::Entity)   // LEFT JOIN users
    .one(db)
    .await?
```

**`find_also_related`가 `(post::Model, Option<user::Model>)` 튜플을 돌려줍니다.** 조인 결과가 타입으로 표현됩니다. sqlx였다면 `SELECT p.*, u.username` 같은 걸 쓰고 `PostWithAuthor` 구조체를 손으로 만들었을 겁니다.

### 조건 조립 — `Condition`

```rust
let mut condition = Condition::all();          // AND로 묶는다
if let Some(keyword) = q.map(str::trim).filter(|s| !s.is_empty()) {
    condition = condition.add(
        Condition::any()                        // OR로 묶는다
            .add(post::Column::Title.contains(keyword))
            .add(post::Column::Body.contains(keyword)),
    );
}

let paginator = post::Entity::find()
    .filter(condition)
    .find_also_related(user::Entity)
    .order_by_desc(post::Column::Id)
    .paginate(db, per_page);

let total = paginator.num_items().await?;
let total_pages = paginator.num_pages().await?;
let rows = paginator.fetch_page(page - 1).await?;   // ← 0-based!
```

- study8에서 `QueryBuilder`로 `" AND (title LIKE "` 문자열을 이어 붙였던 것과 비교해 보세요. **여기서는 SQL 문자열이 한 글자도 등장하지 않습니다.**
- `paginate()`가 **COUNT 쿼리와 페이지 계산을 알아서** 해줍니다. `total_pages`를 직접 올림 계산하던 코드가 사라졌습니다.
- ⚠️ **`fetch_page`는 0-based**입니다. 1-based 페이지 번호를 쓰면 `page - 1`을 넘겨야 합니다. **흔한 off-by-one 버그 지점**입니다.

### 마이그레이션을 Rust로

```rust
manager.create_table(
    Table::create()
        .table(Posts::Table)
        .if_not_exists()
        .col(pk_auto(Posts::Id))
        .col(string(Posts::Title))
        .col(text(Posts::Body))
        .col(integer(Posts::AuthorId))
        .col(timestamp_with_time_zone(Posts::CreatedAt))
        .foreign_key(
            ForeignKey::create()
                .name("fk_posts_author")
                .from(Posts::Table, Posts::AuthorId)
                .to(Users::Table, Users::Id)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned(),
).await?;

#[derive(DeriveIden)]
enum Posts { Table, Id, Title, Body, AuthorId, CreatedAt, UpdatedAt }
```

- **테이블/컬럼 이름이 문자열이 아니라 enum**입니다. 오타가 컴파일 에러가 되고, IDE 리네임이 통합니다.
- **`down()`(롤백)을 강제**합니다. sqlx 마이그레이션에는 없던 것입니다.
- DB 종류가 바뀌어도(SQLite → Postgres) **같은 코드가 동작**합니다. `AUTOINCREMENT` vs `SERIAL` 같은 방언 차이를 SeaORM이 흡수합니다.

### 그래서 어느 쪽이 나은가

| | sqlx | SeaORM |
|---|---|---|
| 무슨 쿼리가 나가는지 | **100% 명확** | 로그를 켜봐야 안다 |
| 조인/관계 | 직접 SQL + 매핑 구조체 | **타입으로 표현** |
| 복잡한 쿼리 (윈도 함수, CTE) | **그냥 쓴다** | 빌더 한계에 부딪히면 raw SQL로 탈출 |
| 컴파일 타임 검증 | `query!` 매크로로 가능 (DB 필요) | 엔티티 정의와 스키마가 어긋나면 **런타임** 오류 |
| DB 이식성 | 방언을 직접 처리 | **추상화됨** |
| 학습 곡선 | SQL을 알면 즉시 | SeaORM API를 따로 배워야 |
| 컴파일 시간 | 빠름 | 느림 (매크로가 많다) |

**정답은 없습니다.** 팀이 SQL에 강하고 쿼리 튜닝이 중요하면 sqlx, 모델 관계가 복잡하고 CRUD가 대부분이면 SeaORM이 편합니다.

---

## 4. ⭐ 도메인 오류에 `StatusCode`가 없다

이게 이번 구조 변경에서 **가장 큰 차이**입니다.

### demo/ — 오류가 HTTP에 묶여 있었다

```rust
// demo/src/error.rs
pub enum AppError { NotFound { .. }, Forbidden(String), /* ... */ }

impl IntoResponse for AppError { /* ← axum에 의존 */ }
```

`repo`와 `service`가 이 타입을 반환하므로, **비즈니스 로직이 HTTP를 알게 됩니다.** 이 코드를 CLI 도구에서 재사용하려면 axum을 통째로 끌고 와야 합니다.

### project/ — 도메인은 "무엇이 잘못됐는가"만 말한다

```rust
// crates/domain/src/error.rs  — axum이 의존성에 없다
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("{resource}을(를) 찾을 수 없습니다 (id: {id})")]
    NotFound { resource: &'static str, id: i32 },

    #[error("{0}")]
    Conflict(String),

    #[error("아이디 또는 비밀번호가 올바르지 않습니다")]
    InvalidCredentials,

    #[error("{0}")]
    Forbidden(String),
    // ...
}
```

번역은 **web 크레이트에서만** 합니다.

```rust
// crates/web/src/error.rs
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // ⭐ 이 표가 "도메인 실패 → HTTP" 번역 규칙의 전부다.
        let (status, code) = match &self.0 {
            DomainError::NotFound { .. }    => (StatusCode::NOT_FOUND, "not_found"),
            DomainError::Conflict(_)        => (StatusCode::CONFLICT, "conflict"),
            DomainError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "unauthorized"),
            DomainError::Forbidden(_)       => (StatusCode::FORBIDDEN, "forbidden"),
            DomainError::Invalid(_)         => (StatusCode::UNPROCESSABLE_ENTITY, "validation_failed"),
            DomainError::Database(_) | DomainError::Internal(_)
                                            => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        // ...
    }
}
```

**얻는 것:**
- domain 크레이트를 CLI·배치·gRPC에서 그대로 재사용할 수 있습니다.
- HTTP 매핑 규칙이 **한 파일, 한 `match`** 에 모여 있어 "이 오류가 몇 번으로 나가지?"를 한눈에 봅니다.
- 새 오류를 추가하면 `match` 완전성 검사가 **번역을 빠뜨린 곳을 컴파일 에러로** 알려줍니다.

### `?`가 그대로 동작하게 만들기

```rust
pub struct ApiError(pub DomainError);

impl<E: Into<DomainError>> From<E> for ApiError {
    fn from(value: E) -> Self { Self(value.into()) }
}
```

블랭킷 `From` 덕분에 핸들러에서 `service::find(...).await?` 한 줄이면 끝납니다. 감싸는 코드를 따로 쓸 필요가 없습니다.

---

## 5. 소유권 검사를 서비스 안으로

### demo/ — 핸들러의 책임

```rust
pub async fn update(/* ... */) -> AppResult<Json<Post>> {
    ensure_owner(&state, id, &auth).await?;   // ← 부르는 걸 잊으면 그대로 구멍
    repo::update(&state.db, id, &body).await?
}
```

### project/ — 도메인의 책임

```rust
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    actor_id: i32,          // ← "누가 하려는 건지"를 인자로 받는다
    title: Option<&str>,
    body: Option<&str>,
) -> DomainResult<PostView> {
    let found = post::Entity::find_by_id(id).one(db).await?
        .ok_or(DomainError::NotFound { resource: "게시글", id })?;

    if found.author_id != actor_id {
        return Err(DomainError::Forbidden("본인이 작성한 글만 수정할 수 있습니다".into()));
    }
    // ...
}
```

**`actor_id`가 필수 인자이므로 "누가 하는지 모른 채 수정"이 불가능합니다.** 검사를 빠뜨릴 수가 없습니다. 핸들러는 이렇게 얇아집니다.

```rust
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i32>,
    ValidatedJson(body): ValidatedJson<UpdatePostRequest>,
) -> ApiResult<Json<PostView>> {
    let updated = post_service::update(
        &state.db, id, user.id, body.title.as_deref(), body.body.as_deref(),
    ).await?;
    Ok(Json(updated))
}
```

> **규칙을 "호출하는 쪽의 예의"에서 "타입 시그니처의 요구"로 옮긴 것**입니다. Rust에서 가장 효과적인 안전 장치는 대개 이런 모양입니다.

---

## 6. 인증을 쿠키로

`demo/`는 순수 API라 `Authorization: Bearer`가 맞았습니다. 여기는 **브라우저가 직접 쓰는 웹사이트**라 다릅니다.

| | localStorage + Bearer | **HttpOnly 쿠키** |
|---|---|---|
| XSS로 토큰 탈취 | **가능** (JS가 읽을 수 있음) | 어려움 (JS가 못 읽음) |
| CSRF | 안전 (자동 전송 안 됨) | **주의 필요** → `SameSite=Lax`로 완화 |
| 모바일 앱 | 편함 | 불편함 |
| 프론트 코드 | 토큰을 직접 관리 | **`credentials: "same-origin"` 한 줄** |

```rust
pub fn session_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{COOKIE_NAME}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={TOKEN_TTL_SECONDS}"
    );
    if secure { cookie.push_str("; Secure"); }
    cookie
}
```

| 속성 | 역할 |
|---|---|
| `HttpOnly` | JavaScript에서 읽을 수 없음 → XSS로 토큰을 훔치기 어렵다 |
| `SameSite=Lax` | 다른 사이트에서 시작된 요청에는 쿠키가 안 붙는다 → CSRF 완화 |
| `Secure` | HTTPS에서만 전송. **로컬 http 개발에서는 꺼야** 쿠키가 전달된다 |
| `Max-Age` | 만료 시각 |

extractor는 **두 방식을 모두** 받습니다. 웹 UI는 쿠키로, 스크립트/테스트는 Bearer로 쓸 수 있습니다.

```rust
let token = bearer_token(parts).or_else(|| cookie_token(parts));
```

---

## 7. 프론트엔드 — 빌드 도구 없이

`static/` 폴더에 `index.html` + `app.js` + `style.css` 세 파일뿐입니다. **npm도, 번들러도 없습니다.**

```rust
let index_path = format!("{static_dir}/index.html");
let spa = ServeDir::new(static_dir).fallback(get(move || {
    let path = index_path.clone();
    async move { serve_index(path).await }
}));

Router::new()
    .nest("/api", api::router())
    .with_state(state)
    .fallback_service(spa)      // /api가 아닌 모든 것 → 정적 파일 또는 index.html
    .layer(CompressionLayer::new())
    .layer(TraceLayer::new_for_http())
```

**`cargo run -p web` 하나로 API와 웹사이트가 같이 뜹니다.** 개발 서버 두 개를 띄우고 CORS를 맞출 필요가 없습니다. (그래서 이 프로젝트에는 `CorsLayer`가 아예 없습니다 — 같은 출처니까요.)

프론트 코드에서 인증은 이게 전부입니다.

```js
await fetch(`/api${path}`, {
  method, body,
  credentials: "same-origin",   // 세션 쿠키를 함께 보낸다
});
```

토큰을 저장하지도, 헤더에 붙이지도 않습니다.

---

## 8. 실행 및 검증

```powershell
cd project
cargo run -p web
# http://127.0.0.1:4000
```

### API 동작 확인 (실제 응답)

| # | 요청 | 응답 |
|---|---|---|
| 1 | `GET /api/health` | `200` `{"status":"ok"}` |
| 2 | `POST /api/auth/register` (while) | `201` `{"id":1,"username":"while","email":"while@example.com"}` |
| 3 | `POST /api/auth/register` (bob) | `201` `{"id":2,...}` |
| 4 | while로 재가입 | `409` `{"error":"conflict","message":"이미 사용 중인 아이디 또는 이메일입니다"}` |
| 5 | `GET /api/auth/me` (비로그인) | `401` `{"error":"unauthorized","message":"로그인이 필요합니다"}` |
| 6 | `POST /api/posts` (비로그인) | `401` 동일 |
| 7 | `POST /api/auth/login` | `200` + **`Set-Cookie: session=...; HttpOnly`** |
| 8 | `GET /api/auth/me` (쿠키 있음) | `200` `{"id":1,"username":"while",...}` |
| 9 | `POST /api/posts` | `201` `{"id":1,"title":"SeaORM is here","author_id":1,"author_name":"while",...}` |
| 11 | `GET /api/posts?page=1&per_page=5` | `200` `{"items":[...],"page":1,"per_page":5,"total":2,"total_pages":1}` |
| 12 | `GET /api/posts?q=searchable` | `200` `{"items":[{"id":2,...}],"total":1,...}` |
| 14 | `PATCH /api/posts/1` (본인) | `200` `{"title":"SeaORM is here (edited)","updated_at":"...11.199398800Z"}` |
| 15 | `POST /api/posts` `{"title":"","body":""}` | `422` `{"details":{"title":["제목은 1~100자여야 합니다"],"body":["본문은 1~5000자여야 합니다"]}}` |
| 16 | `GET /api/nope` | `404` `{"error":"not_found","message":"없는 경로입니다."}` |
| 17 | `PATCH /api/posts/1` (**bob**) | `403` `{"error":"forbidden","message":"본인이 작성한 글만 수정할 수 있습니다"}` |
| 18 | `DELETE /api/posts/1` (**bob**) | `403` `{"error":"forbidden","message":"본인이 작성한 글만 삭제할 수 있습니다"}` |
| 19 | `POST /api/auth/logout` | `204` + 쿠키 만료 |
| 20 | 로그아웃 후 `GET /api/auth/me` | `401` |

**9번의 `author_name`** 을 보세요. `posts` 테이블에는 `author_id`밖에 없는데 이름이 함께 나옵니다. `find_also_related`가 만든 조인 결과입니다.

**17·18번**: 소유권 검사가 서비스 안에서 동작합니다. 핸들러에는 관련 코드가 한 줄도 없습니다.

### 정적 서빙 / SPA

```
GET /                  → 200  text/html
GET /style.css         → 200  text/css
GET /app.js            → 200  text/javascript
GET /some/deep/route   → 200  text/html  (index.html — SPA fallback)
GET /api/nope          → 404  {"error":"not_found",...}  (API는 JSON 404)
```

**마지막 두 줄이 중요합니다.** SPA fallback이 `/api` 요청까지 삼키면, 프론트엔드가 JSON 대신 HTML을 받고 `JSON.parse` 에러가 납니다. `/api` 라우터에 자체 `fallback`을 붙여 막았습니다.

### 화면

브라우저에서 `http://127.0.0.1:4000`을 열면:

- **목록** — 검색창 + 5개씩 페이지네이션, 이전/다음 버튼
- **회원가입 / 로그인** — 서버 검증 실패 시 필드별 메시지 표시
- **글쓰기 / 수정 / 삭제** — 본인 글에만 수정·삭제 버튼이 보임
- 라이트/다크 테마 자동 대응 (`prefers-color-scheme`)

---

## 9. 처음 만난 오류

### 9-1. ⭐ `not_found_service`가 상태 코드를 404로 덮어쓴다

SPA fallback을 이렇게 만들었더니:

```rust
ServeDir::new(static_dir).not_found_service(ServeFile::new("static/index.html"))
```

```
GET /some/deep/route → 404  (본문은 index.html)
```

index.html은 잘 나오는데 **상태가 404**였습니다. tower-http 소스를 열어 보니 원인이 명확했습니다.

```rust
// tower-http-0.7.1/src/services/fs/serve_dir/mod.rs:350
pub fn not_found_service<F2>(self, new_fallback: F2) -> ServeDir<SetStatus<F2>, B> {
    self.fallback(SetStatus::new(new_fallback, StatusCode::NOT_FOUND))
}
```

**`not_found_service`는 내부적으로 `SetStatus`로 감싸 응답 상태를 404로 강제**합니다. 바로 위 `fallback`의 문서 주석에도 적혀 있습니다: *"The status code returned by the fallback will not be altered. Use `not_found_service` to ... always respond with 404."*

→ **`fallback`** 을 쓰면 해결됩니다.

```rust
ServeDir::new(static_dir).fallback(get(move || { /* index.html을 200으로 */ }))
```

```
GET /some/deep/route → 200  text/html  (index.html)
```

> 지금은 해시 라우팅(`#/posts/1`)이라 티가 안 나지만, History API 라우팅으로 바꾸면 **모든 딥링크가 404**가 됩니다. 그리고 크롤러는 404 페이지를 색인하지 않습니다.
>
> **교훈: 이름이 비슷한 두 메서드의 차이는 문서(그리고 소스)에 적혀 있습니다.** Rust는 의존성 소스가 `~/.cargo/registry/src/...`에 그대로 있어서 5초면 확인됩니다.

### 9-2. `&Migrator: IntoSchemaManagerConnection is not satisfied`

```rust
migration::MigratorTrait::up(&migration::Migrator, &db, None).await?;   // ❌
```

```
error[E0061]: this function takes 2 arguments but 3 arguments were supplied
note: async fn up<'c, C>(db: C, steps: Option<u32>) -> Result<(), DbErr>
```

`MigratorTrait::up`은 **`self`를 받지 않는 연관 함수**입니다. `Migrator::up(&db, None)` 형태로 부릅니다.

그런데 그렇게 고쳤더니 또:

```
error[E0599]: no associated function or constant named `up` found for struct `Migrator`
```

**trait 메서드를 쓰려면 그 trait이 스코프에 있어야 합니다.**

```rust
use crate::migration::MigratorTrait;   // ← 이 한 줄이 필요
```

> Rust 초심자가 자주 막히는 지점입니다. "분명 구현했는데 메서드가 없다"면 **십중팔구 trait import 누락**입니다.

### 9-3. `paginate().fetch_page()`는 0-based

```rust
let rows = paginator.fetch_page(page - 1).await?;   // 1-based 페이지 번호라면
```

에러가 안 나고 **조용히 한 페이지씩 밀립니다.** `page`가 1일 때 `fetch_page(1)`을 부르면 2페이지가 나옵니다. `saturating_sub(1)`을 써서 0에서 언더플로도 막았습니다.

### 9-4. SeaORM은 UNIQUE 위반을 구조화해서 주지 않는다

sqlx에는 `db_err.is_unique_violation()`이 있었습니다(study6). SeaORM의 `DbErr`에는 그런 편의 메서드가 없어서 메시지로 판단했습니다.

```rust
if err.to_string().contains("UNIQUE constraint failed") {
    DomainError::Conflict("이미 사용 중인 아이디 또는 이메일입니다".to_string())
}
```

**문자열 매칭은 깨지기 쉽습니다.** DB를 Postgres로 바꾸면 메시지가 달라집니다(`duplicate key value violates unique constraint`). 실무라면 `DbErr::Query(RuntimeErr::SqlxError(e))`까지 내려가 sqlx의 에러 코드를 보는 게 안전합니다.

> **추상화가 한 겹 늘면 아래층의 정보가 일부 사라집니다.** ORM을 쓸 때 늘 따라오는 대가입니다.

### 9-5. 워크스페이스 멤버에 소스 파일이 없으면 전체가 안 열린다

```
error: failed to load manifest for workspace member `.../crates/web`
Caused by: no targets specified in the manifest
  either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present
```

`crates/web/Cargo.toml`만 만들고 `src/main.rs`를 아직 안 만든 상태에서 `cargo check -p domain`을 했더니, **domain과 무관한 이유로 워크스페이스 전체가 로드되지 못했습니다.** 워크스페이스는 항상 모든 멤버의 매니페스트를 먼저 읽습니다.

---

## 10. 두 구조를 다 만들어 보고 느낀 것

### 크레이트 분리는 공짜가 아니다

- 👍 **의존 방향이 컴파일러로 강제**된다. domain에 HTTP가 섞일 수 없다.
- 👍 도메인만 떼어 CLI·배치·다른 프로토콜에서 재사용할 수 있다.
- 👎 **타입을 옮기는 일이 늘어난다.** `DomainError` → `ApiError`, `Model` → `Response`처럼 경계마다 변환이 필요하다.
- 👎 작은 프로젝트에는 과하다. `demo/` 규모라면 모듈 분리로 충분하다.

**기준:** 도메인 로직을 HTTP 밖에서도 쓸 일이 있는가? 팀이 여럿이라 계층이 무너질 위험이 있는가? 둘 다 아니라면 단일 크레이트가 낫습니다.

### ORM은 "덜 쓰는 대신 덜 보인다"

SeaORM 쪽 코드가 확실히 짧고, 조인·페이지네이션이 편했습니다. 대신 **무슨 SQL이 나가는지 모릅니다.** `find_also_related`가 JOIN인지 두 번 쿼리인지 로그를 켜야 압니다. 성능이 중요한 지점에서는 sqlx의 투명함이 그리워집니다.

### 오류 타입을 계층으로 나눈 게 가장 만족스러웠다

`DomainError`에 `StatusCode`가 없다는 것 하나로 **"이 코드는 웹 전용인가?"라는 질문이 사라졌습니다.** 번역 표 하나만 보면 전체 HTTP 계약이 파악됩니다. 다음에 비슷한 프로젝트를 만든다면, 단일 크레이트를 쓰더라도 **오류 타입만은 이렇게 나누겠습니다.**

### 정적 파일을 서버가 직접 서빙하는 건 생각보다 좋다

`cargo run` 하나로 사이트 전체가 뜨고, CORS 설정이 필요 없고, 배포는 **바이너리 + `static/` 폴더**면 끝입니다(`rust-embed`를 쓰면 바이너리 하나). 프론트가 복잡해지기 전까지는 이 구성이 가장 단순합니다.

---

## 11. 전체 학습 요약

| 문서 | 핵심 한 줄 |
|---|---|
| [study0](study0.md) | Axum은 tokio/hyper/tower 위의 얇은 층. DI도 매크로도 없다 |
| [study1](study1.md) | 라우터는 값으로 조립하고, extractor가 파싱과 400까지 처리한다 |
| [study2](study2.md) | 공유 가변 상태 = `Arc<RwLock<T>>`. 컴파일러가 동시성을 검사한다 |
| [study3](study3.md) | 파싱(serde)과 검증(validator)은 다른 단계. 커스텀 extractor가 파이프를 대신한다 |
| [study4](study4.md) | `AppError` + `IntoResponse`가 전역 예외 핸들러를 대신한다 |
| [study5](study5.md) | sqlx는 ORM이 아니다. `fetch_optional`로 404와 500을 구분한다 |
| [study6](study6.md) | `FromRequestParts` 구현체가 Guard다. 인증(401)과 인가(403)는 다르다 |
| [study7](study7.md) | 미들웨어는 tower의 것. **순서가 전부다** |
| [study8](study8.md) | `QueryBuilder`의 `push`/`push_bind` 분리가 인젝션을 원천 차단한다 |
| [study9](study9.md) | `oneshot`으로 서버 없이 통합 테스트. in-memory DB는 연결 1개로 |
| [study10](study10.md) | 문서화는 수동. Docker는 멀티 스테이지 + 의존성 레이어 캐싱 |
| **studyFinal** | **구조 선택(크레이트 분리·ORM·오류 계층)이 무엇을 바꾸는지** |

### Rust로 웹을 해보고 남는 인상

1. **컴파일러가 리뷰어다.** `Send`/`Sync`/`Clone`/`match` 완전성 에러는 잔소리가 아니라 "여기 버그 납니다"라는 경고였습니다.
2. **마법이 없어서 손이 간다.** DI도, 자동 문서화도, 스키마 자동 동기화도 없습니다. 대신 **런타임에 놀랄 일이 거의 없습니다.**
3. **에러 메시지가 좋다.** "found an item that was configured out"(feature 누락), "this trait is only implemented for `&'static str`"(SQL 인젝션 방지) 같은 메시지는 해결책까지 담고 있었습니다.
4. **버전 차이가 크다.** axum 0.7→0.8(경로 문법), sqlx 0.8→0.9(`SqlSafeStr`), utoipa 4→5(`aliases` 제거), argon2 0.5→0.6(API 변경). **블로그 예제를 붙여넣기 전에 `Cargo.toml`의 버전을 확인하세요.**
5. **의존성 소스가 로컬에 다 있다.** 문서가 애매하면 `~/.cargo/registry/src/...`를 직접 열면 됩니다. 이번에도 `not_found_service` 문제를 그렇게 해결했습니다.
