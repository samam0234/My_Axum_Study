# OpenAPI 문서화 & Docker 배포 (Study 10 · 고급 5)

API를 만들었으면 **남이 쓸 수 있어야** 하고, **어디서든 돌아가야** 합니다. 마지막 고급 문서에서 그 두 가지를 합니다.

**이 문서에서 하는 것**: `utoipa`로 OpenAPI 명세 생성 + Swagger UI + 릴리스 빌드 최적화 + 멀티 스테이지 Dockerfile

```powershell
cargo add utoipa --features axum_extras,chrono
cargo add utoipa-swagger-ui --features axum
```

---

## 1. Rust에는 자동 문서화 마법이 없다

| 프레임워크 | 방식 | 손이 가는 정도 |
|---|---|---|
| **FastAPI** | 타입힌트만으로 **완전 자동** (`/docs` 기본 제공) | 0 |
| **NestJS** | `@nestjs/swagger` + 플러그인 (DTO에서 자동 추론) | 낮음 |
| **Spring Boot** | springdoc (애노테이션에서 추론) | 낮음 |
| **Axum + utoipa** | **`#[utoipa::path(...)]`를 직접 작성** | **높음** |

이유는 명확합니다. **Rust에는 런타임 리플렉션이 없습니다.** 실행 중에 "이 함수의 인자 타입이 뭐지?"를 물어볼 수 없습니다. 그래서 컴파일 타임 매크로로 정보를 **미리 적어 둬야** 합니다.

**장단:**
- 👎 라우트마다 어노테이션을 쓴다. 코드와 문서가 어긋날 수 있다.
- 👍 문서에 나갈 내용(설명, 상태 코드, 예시, 보안 요구사항)을 **정확히 통제**한다. 자동 추론이 만들어내는 이상한 스키마가 없다.

> `utoipa-axum`의 `OpenApiRouter`를 쓰면 라우터 등록과 문서 등록을 한 번에 할 수 있어 어긋남을 줄일 수 있습니다. 여기서는 개념이 잘 드러나도록 기본 방식을 씁니다.

---

## 2. 스키마 등록 — `ToSchema`

```rust
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Post {
    pub id: i64,
    pub title: String,
    // ...
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreatePost { /* ... */ }
```

- `ToSchema`는 **JSON 스키마 조각을 만드는 코드를 생성**합니다.
- `chrono` feature를 켰기 때문에 `DateTime<Utc>`가 `"type": "string", "format": "date-time"`으로 나갑니다.
- **`UserRow`에는 일부러 안 붙였습니다.** study6에서 `Serialize`를 안 붙인 것과 같은 이유 — 비밀번호 해시가 문서에도 나가면 안 됩니다.

### 쿼리 파라미터 — `IntoParams`

```rust
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery { /* page, per_page, q, tag */ }
```

`ToSchema`(본문 스키마)와 `IntoParams`(쿼리/경로 파라미터)는 **다른 trait**입니다. 헷갈리기 쉽습니다.

### 제네릭 스키마 — `Page<T>`

```rust
// utoipa 5는 제네릭 스키마를 그대로 지원한다. (4까지 있던 #[aliases]는 제거됨)
#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T> { /* ... */ }
```

등록할 때 구체 타입으로 적습니다.

```rust
components(schemas(
    crate::pagination::Page<crate::posts::model::Post>,
    // ...
))
```

생성된 스키마 이름은 **`Page_Post`** 가 됩니다(아래 실행 결과 참고).

> ⚠️ utoipa 4까지는 `#[aliases(PostPage = Page<Post>)]`를 썼는데 **5에서 제거**되었습니다. 그대로 쓰면 `cannot find attribute 'aliases' in this scope` 에러가 납니다. 아래 6-1 참고.

---

## 3. 라우트 문서화 — `#[utoipa::path]`

```rust
#[utoipa::path(
    get,
    path = "/posts",
    tag = "posts",
    params(PageQuery),
    responses(
        (status = 200, description = "게시글 페이지", body = Page<Post>),
        (status = 422, description = "페이지 파라미터 범위 오류"),
    )
)]
pub async fn list(/* ... */) { }
```

```rust
#[utoipa::path(
    post,
    path = "/posts",
    tag = "posts",
    request_body = CreatePost,
    security(("bearer_auth" = [])),        // ← 이 라우트는 토큰이 필요하다
    responses(
        (status = 201, description = "생성됨", body = Post),
        (status = 401, description = "인증 필요"),
        (status = 422, description = "검증 실패"),
    )
)]
pub async fn create(/* ... */) { }
```

- **`path`는 라우터에 등록한 실제 경로 전체**를 적습니다. `nest("/posts", ...)` 아래에 있어도 `/posts`라고 씁니다. **여기가 어긋나기 가장 쉬운 지점**입니다.
- 경로 파라미터는 `params(("id" = i64, Path, description = "게시글 id"))`.
- `security(...)`를 적은 라우트만 Swagger UI에서 자물쇠 아이콘이 붙습니다.

---

## 4. 명세 조립 — `#[derive(OpenApi)]`

`src/openapi.rs`:

```rust
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Axum 학습용 게시판 API",
        version = "0.1.0",
        description = "study1~10에서 단계적으로 만든 게시판 + 인증 API",
    ),
    paths(
        crate::auth::handler::register,
        crate::auth::handler::login,
        crate::auth::handler::me,
        crate::posts::handler::list,
        // ... 전부 나열
    ),
    components(schemas(
        crate::posts::model::Post,
        crate::pagination::Page<crate::posts::model::Post>,
        // ...
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "회원가입 / 로그인 / 내 정보"),
        (name = "posts", description = "게시판 CRUD"),
    )
)]
pub struct ApiDoc;
```

### 보안 스킴 — Swagger UI의 "Authorize" 버튼

```rust
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}
```

이게 있어야 Swagger UI 우상단에 **Authorize** 버튼이 생기고, 거기에 토큰을 한 번 넣으면 이후 모든 요청에 `Authorization: Bearer ...`가 자동으로 붙습니다. **문서를 읽는 도구가 아니라 실제로 API를 쳐보는 도구**가 되는 지점입니다.

### 라우터에 장착

```rust
Router::new()
    .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()))
    .route("/", get(routes::root))
    // ...
```

`SwaggerUi`는 `Router`로 변환 가능해서 `.merge()`로 붙입니다.

> ⚠️ 운영에서는 **문서를 공개할지** 결정해야 합니다. 내부 API라면 `cfg!(debug_assertions)`나 환경변수로 이 라인을 감싸서 개발 환경에서만 노출하세요. API 구조 전체가 공격자에게 지도가 됩니다.

---

## 5. 실행 결과 — 생성된 명세

```powershell
cargo run
```

```
GET /api-docs/openapi.json → 200, 9146 bytes

openapi=3.1.0  title=Axum 학습용 게시판 API  version=0.1.0

paths:   /auth/login, /auth/me, /auth/register,
         /posts, /posts/{id}, /posts/{id}/thumbnail

schemas: CreatePost, LoginRequest, Page_Post, Post,
         RegisterRequest, TokenResponse, UpdatePost, UserPublic

securitySchemes: bearer_auth
```

Swagger UI:

```
GET /docs → 200, HTML (swagger 자산 로드)
```

브라우저에서 `http://127.0.0.1:3000/docs`를 열면 실제로 요청을 쏴볼 수 있습니다.

- **`Page_Post`** — 제네릭 `Page<Post>`가 이 이름으로 생성됐습니다.
- **`UserRow`는 목록에 없습니다.** `ToSchema`를 안 붙였으니 문서에도, 응답에도 나가지 않습니다.
- `openapi=3.1.0` — utoipa 5는 OpenAPI 3.1을 생성합니다.

---

## 6. 릴리스 빌드 최적화

```toml
[profile.release]
opt-level = 3
lto = "thin"        # 크레이트 경계를 넘는 인라이닝
codegen-units = 1   # 병렬 컴파일을 포기하고 최적화 기회를 늘린다
strip = true        # 심볼 제거 (백트레이스는 잃는다)
# panic = "abort"   # ⚠️ 절대 켜지 말 것
```

### 실측

| | 빌드 시간 | 바이너리 크기 |
|---|---|---|
| debug | (증분 수 초) | 16.3 MB |
| release (기본) | 57.9s | 19.6 MB |
| release (위 설정) | **89.4s** | **18.5 MB** |

**관찰 두 가지:**

1. **release가 debug보다 큽니다.** 의외인데, Windows에서는 디버그 정보가 `.exe`가 아니라 별도 `.pdb`로 빠지고, release는 공격적인 인라이닝으로 코드가 늘어나기 때문입니다.
2. **`strip` + `lto`로 1.1MB 줄고, 빌드 시간은 1.5배가 되었습니다.** 크게 줄지 않은 이유는 **Swagger UI 정적 자산이 바이너리에 통째로 박혀 있기** 때문입니다. `utoipa-swagger-ui`를 빼면 훨씬 작아집니다.

### ⚠️ `panic = "abort"`를 켜면 안 되는 이유

바이너리를 더 줄이려고 흔히 켜는 옵션인데, **study7에서 붙인 `CatchPanicLayer`가 동작하지 않게 됩니다.** `abort`는 되감기(unwind)를 아예 하지 않으므로 잡을 패닉 자체가 없고, 프로세스가 즉시 죽습니다. 패닉 하나에 **서버 전체가 내려갑니다.**

> 이런 식으로 **설정 하나가 아키텍처 결정을 무효화하는 경우**가 있습니다. 옵션을 켜기 전에 "이게 무엇을 전제로 하는가"를 확인해야 합니다.

---

## 7. Docker — 멀티 스테이지 빌드

### 왜 멀티 스테이지인가

Rust 빌드에는 컴파일러 + 의존성 소스 + 중간 산출물이 필요합니다. **다 합쳐 수 GB**입니다. 하지만 **실행에는 바이너리 하나면 충분**합니다.

```
[builder 스테이지]  rust:1.98-slim  →  수 GB, 바이너리 생성
        ↓ 바이너리만 복사
[runtime 스테이지]  debian:bookworm-slim  →  ~100MB
```

### `Dockerfile`

```dockerfile
# ── 1단계: 빌드 ──────────────────────────────────────────────
FROM rust:1.98-slim-bookworm AS builder
WORKDIR /app

# 의존성 레이어 캐싱 트릭
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs src/lib.rs && cargo build --release

# ── 2단계: 실행 ──────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /usr/sbin/nologin appuser
WORKDIR /app

COPY --from=builder /app/target/release/demo /usr/local/bin/demo

RUN mkdir -p /app/data /app/uploads && chown -R appuser:appuser /app
VOLUME ["/app/data", "/app/uploads"]
USER appuser

ENV DATABASE_URL="sqlite:///app/data/data.db?mode=rwc" \
    UPLOAD_DIR="/app/uploads" \
    BIND_ADDR="0.0.0.0:3000" \
    RUST_LOG="demo=info,tower_http=info"

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1

CMD ["demo"]
```

### ⭐ 의존성 레이어 캐싱 트릭

```dockerfile
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release && rm -rf src
```

**Rust는 컴파일이 느려서 이 트릭의 효과가 특히 큽니다.**

- Docker는 레이어 단위로 캐시합니다. `Cargo.toml`이 안 바뀌면 이 `RUN`은 **캐시에서 재사용**됩니다.
- 우리 프로젝트는 의존성이 300개가 넘고 처음 컴파일에 1분 이상 걸립니다. 소스를 한 글자 고칠 때마다 그걸 다시 하면 개발이 불가능합니다.
- `touch`가 필요한 이유: 더미 빌드의 산출물이 남아 있으면 Cargo가 "이미 최신"이라고 판단합니다. 타임스탬프를 갱신해 재컴파일을 강제합니다.

> `cargo-chef` 크레이트를 쓰면 이 트릭을 더 정확하게 자동화할 수 있습니다. 워크스페이스가 여러 개면 특히 유용합니다.

### 배포 시 체크포인트 5가지

| 항목 | Dockerfile에서 | 왜 |
|---|---|---|
| **root로 실행 안 함** | `useradd` + `USER appuser` | 컨테이너 탈출 시 피해 최소화 |
| **`BIND_ADDR=0.0.0.0:3000`** | `ENV` | `127.0.0.1`로 두면 **컨테이너 밖에서 접속 불가** |
| **비밀은 이미지에 안 굽는다** | `JWT_SECRET`을 ENV에 **안** 적음 | 이미지 레이어는 누구나 열어볼 수 있음 |
| **데이터는 볼륨에** | `VOLUME ["/app/data", ...]` | 컨테이너를 지워도 DB가 남아야 함 |
| **헬스체크** | `HEALTHCHECK` | 오케스트레이터가 재시작 판단을 함 |

**`BIND_ADDR`가 특히 흔한 실수입니다.** 로컬에서 `127.0.0.1`로 잘 돌던 서버가 컨테이너에서는 "연결 거부"가 납니다. 컨테이너 안의 루프백은 호스트와 다른 네임스페이스이기 때문입니다.

**`migrations/` 폴더를 runtime 이미지에 복사하지 않은 것**도 눈여겨보세요. `sqlx::migrate!`가 **컴파일 시점에 SQL을 바이너리에 박아 넣었기** 때문입니다(study5). 실행 파일 하나면 끝입니다.

### `.dockerignore`

```
target/        ← 이걸 빼지 않으면 빌드 컨텍스트 전송만으로 몇 분이 걸린다
*.db
uploads/
.env
.git/
```

`target/`은 수 GB입니다. **`.dockerignore`를 빠뜨리면 `docker build`가 시작도 못 하고 몇 분을 씁니다.** Rust 프로젝트에서 가장 중요한 한 줄입니다.

### `compose.yaml`

```yaml
services:
  api:
    build: .
    image: axum-demo:latest
    ports:
      - "3000:3000"
    environment:
      JWT_SECRET: ${JWT_SECRET:?JWT_SECRET 환경변수를 설정하세요}
      RUST_LOG: demo=info,tower_http=info
    volumes:
      - api-data:/app/data
      - api-uploads:/app/uploads
    restart: unless-stopped

volumes:
  api-data:
  api-uploads:
```

`${JWT_SECRET:?메시지}` 문법은 **변수가 없으면 실행을 거부**합니다. "시크릿 설정을 깜빡하고 배포"를 막아줍니다.

### 실행 명령

```powershell
# 빌드
docker build -t axum-demo:latest .

# 실행 (시크릿은 실행 시점에 주입)
docker run -d --name axum-demo -p 3000:3000 `
  -e JWT_SECRET="$([Convert]::ToBase64String((1..48 | % { Get-Random -Max 256 })))" `
  -v axum-data:/app/data -v axum-uploads:/app/uploads `
  axum-demo:latest

# 또는 compose
$env:JWT_SECRET = "..."
docker compose up -d --build

docker logs -f axum-demo
docker compose down
```

> ⚠️ **이 문서의 Docker 부분은 실제로 빌드해 보지 못했습니다.** 작업 환경에 Docker CLI는 설치되어 있었지만 Docker Desktop 데몬이 실행 중이 아니라서(`open //./pipe/dockerDesktopLinuxEngine: The system cannot find the file specified`) 이미지를 만들 수 없었습니다. Dockerfile 자체는 위 설명대로 작성해 두었으니, Docker Desktop을 켠 뒤 `docker build -t axum-demo:latest .`로 직접 확인해 보세요. 다른 문서들의 실행 결과는 전부 실제로 돌려서 얻은 것입니다.

---

## 8. 처음 만난 오류

### 8-1. `cannot find attribute 'aliases' in this scope`

```rust
#[derive(Debug, Serialize, ToSchema)]
#[aliases(PostPage = Page<crate::posts::model::Post>)]   // ❌ utoipa 5에서 제거됨
pub struct Page<T> { /* ... */ }
```

```
error: cannot find attribute `aliases` in this scope
error[E0425]: cannot find type `PostPage` in module `crate::pagination`
```

utoipa 4까지의 관용구인데 **5에서 제네릭 스키마를 직접 지원하게 되면서 제거**되었습니다. 등록할 때 구체 타입을 그대로 쓰면 됩니다.

```rust
components(schemas(crate::pagination::Page<crate::posts::model::Post>))
```

> 확인 방법: 크레이트 소스가 `~/.cargo/registry/src/...`에 그대로 있으니 `grep -rn "aliases" utoipa-gen-5.5.0/src/lib.rs` 로 존재 여부를 바로 확인할 수 있었습니다. **Rust는 의존성 소스가 로컬에 다 있어서 이런 확인이 빠릅니다.**

### 8-2. `ToSchema`와 `IntoParams`를 혼동

쿼리 파라미터 구조체에 `ToSchema`만 붙이고 `params(PageQuery)`를 쓰면 컴파일 에러가 납니다. **본문 스키마는 `ToSchema`, 쿼리/경로 파라미터는 `IntoParams`** 입니다.

### 8-3. `path`가 실제 라우트와 어긋남

```rust
// 라우터: .nest("/posts", ...) 안의 .route("/{id}", ...)
#[utoipa::path(get, path = "/{id}", ...)]   // ❌ 문서에는 /{id}로 나감
#[utoipa::path(get, path = "/posts/{id}", ...)]  // ✅
```

**컴파일 에러가 안 납니다.** 문서만 조용히 틀립니다. `utoipa-axum`의 `OpenApiRouter`를 쓰면 라우터 등록과 문서를 한 번에 처리해 이 문제를 줄일 수 있습니다.

---

## 9. 이번 study 정리

- **Rust에는 런타임 리플렉션이 없어서 문서 자동화가 공짜가 아니다.** `#[utoipa::path]`를 직접 쓴다. 대신 통제력이 높다.
- `ToSchema`(본문) / `IntoParams`(쿼리·경로)를 구분한다. utoipa 5는 **제네릭 스키마를 그대로 지원**한다(`#[aliases]`는 제거됨).
- `Modify`로 보안 스킴을 추가해야 Swagger UI에 **Authorize** 버튼이 생긴다.
- **`ToSchema`를 안 붙이면 문서에도 안 나간다.** `UserRow`가 그 예 — 타입 설계가 곧 노출 통제다.
- 릴리스 빌드는 `lto` + `codegen-units=1` + `strip`. **`panic = "abort"`는 `CatchPanicLayer`를 무력화하므로 쓰지 않는다.**
- Docker는 **멀티 스테이지**로. 빌드 스테이지 수 GB → 런타임 이미지 ~100MB.
- **Rust에서는 의존성 레이어 캐싱 트릭이 필수**다. 없으면 소스 한 줄 고칠 때마다 전체 재컴파일.
- `.dockerignore`에 **`target/`**. 이거 하나로 빌드 시작 속도가 갈린다.
- 컨테이너에서는 **`BIND_ADDR=0.0.0.0`**, 실행은 **비root**, 비밀은 **이미지에 굽지 않기**, 데이터는 **볼륨**.
- `sqlx::migrate!` 덕분에 **마이그레이션 폴더를 이미지에 넣지 않아도 된다.**

다음: [studyFinal.md](studyFinal.md) — 지금까지 배운 것을 **구조를 바꿔서** 다시 조립한 최종 프로젝트.
