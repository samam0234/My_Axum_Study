# study8 — 구조화 · 재사용

> 🎯 **목표**: 커진 코드를 **폴더·모듈·크레이트 경계**로 정리하고, 오류·extractor·미들웨어 같은 공통 코드를 한 곳에 모아 중복을 줄이며, 설정을 코드에서 분리해 **개발/운영 설정을 나눈다.** 미들웨어는 **순서가 전부**라는 것을 실제 사례로 확인한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [demo/src/lib.rs](demo/src/lib.rs), [demo/src/main.rs](demo/src/main.rs), [demo/src/config.rs](demo/src/config.rs), [demo/src/extract.rs](demo/src/extract.rs), [demo/src/middleware.rs](demo/src/middleware.rs), [demo/src/state.rs](demo/src/state.rs) |

---

## ① 목표

1. 기능별 폴더(`posts/`, `auth/`)와 횡단 관심사 파일(`error`, `extract`, `middleware`, `config`)을 구분한다.
2. **lib + bin 구조**로 나눠 실행 파일과 테스트가 같은 라우터를 쓰게 한다.
3. 중복되는 코드(검증, 오류 변환, 소유권 검사)를 한 곳으로 모은다.
4. 설정(`Config`)을 한 곳에서 읽고 검증하며, 개발과 운영 값을 나눈다.
5. 미들웨어 순서 규칙을 설명할 수 있다.

---

## ② 📖 이론

### 구조의 기준: "무엇이 무엇을 아는가"

```
main.rs ──▶ lib.rs::app() ──▶ posts::router(), auth::router()
                │                    │
                │                    ├─ handler  (HTTP를 안다)
                │                    ├─ repo     (SQL을 안다)
                │                    └─ model    (타입)
                └─ middleware, error, extract, config, state  ← 모든 기능이 공유 (횡단)
```

규칙 두 가지:

1. **핸들러는 SQL을 모르고, repo는 `StatusCode`를 모른다.** 한 층이 바뀌어도 다른 층은 안 바뀐다.
2. **횡단 관심사는 기능 폴더 안에 복붙하지 않는다.** 오류 변환·검증·로깅은 공통 모듈에 한 번만 둔다.

### 왜 lib + bin 인가

```
src/main.rs   ← 얇다: 설정 읽기 → 자원 준비 → axum::serve
src/lib.rs    ← 두껍다: app(state) -> Router 로 라우터·미들웨어 조립
tests/api.rs  ← lib을 외부 사용자처럼 가져다 쓴다
```

`tests/`는 **별도 크레이트**로 컴파일되어 `main.rs`를 import할 수 없다. 앱 조립 로직이 `lib.rs`에 있어야 **운영과 테스트가 똑같은 라우터**를 쓴다 (study9).

### 설정의 원칙

| 원칙 | 이유 |
|---|---|
| 설정은 **환경변수**로 (12-factor) | 같은 빌드를 개발·운영에 그대로 쓰고, 값만 바꾼다 |
| **한 곳에서, 시작할 때 한 번** 읽는다 | `std::env::var`가 코드 곳곳에 흩어지면 "이 앱이 어떤 설정을 쓰는가"를 알 수 없다 |
| **시작 시점에 검증**한다 | 서버가 뜬 뒤에 발견하는 것보다 낫다 (너무 짧은 비밀키는 부팅 거부) |
| 비밀은 코드·이미지·저장소에 없다 | `.env`는 gitignore, 운영은 실행 시 주입 |
| 우선순위: **실제 환경변수 > `.env` > 기본값** | 운영에서 값을 덮어쓰기 쉽다 |

### 미들웨어: 순서가 전부다

`ServiceBuilder`에 **위에 적은 것이 바깥(먼저 실행, 응답은 마지막에 통과)** 이다. 일반 규칙:

> **"모든 응답에 반드시 적용돼야 하는 것(요청 ID·로깅)은 바깥, 특정 상황에서만 동작하는 것(압축·본문 제한)은 안쪽."**

---

## ③ 🛠️ 실기

### 3-1. 앱 조립 — [demo/src/lib.rs](demo/src/lib.rs)

```rust
pub fn app(state: AppState) -> Router {
    let upload_dir = state.upload_dir.as_str().to_string();

    Router::new()
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()))
        .route("/", get(routes::root))
        .route("/health", get(routes::health))
        // ... 데모용 라우트
        .nest("/posts", posts::router())
        .nest("/auth", auth::router())
        .nest_service("/uploads", ServeDir::new(upload_dir))
        .fallback(handle_404)
        .method_not_allowed_fallback(handle_405)
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(middleware::REQUEST_ID_HEADER, middleware::request_id_maker()))  // 1
                .layer(PropagateRequestIdLayer::new(middleware::REQUEST_ID_HEADER))                             // 2
                .layer(TraceLayer::new_for_http())                                                              // 3
                .layer(axum::middleware::from_fn(middleware::response_time))                                   // 4
                .layer(CompressionLayer::new())                                                                 // 5
                .layer(axum::middleware::from_fn(middleware::error_envelope))                                   // 6
                .layer(CatchPanicLayer::custom(middleware::on_panic))                                           // 7
                .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, middleware::REQUEST_TIMEOUT))// 8
                .layer(cors_layer())                                                                            // 9
                .layer(DefaultBodyLimit::max(middleware::BODY_LIMIT_BYTES)),                                    // 10
        )
}
```

| 번호 | 계층 | 이 자리에 있는 이유 |
|---|---|---|
| 1 | `SetRequestIdLayer` | 가장 바깥. 이후 모든 로그가 이 ID를 단다 |
| 2 | `PropagateRequestIdLayer` | 요청 ID를 **응답 헤더로** 돌려준다. `CatchPanic`/`Timeout`보다 **바깥**이어야 패닉·타임아웃 응답에도 붙는다 |
| 3 | `TraceLayer` | 요청/응답 로깅 |
| 4 | `response_time` | `x-response-time-ms` 헤더 |
| 5 | `CompressionLayer` | `error_envelope`보다 **바깥**: 압축된 바이트는 JSON으로 읽을 수 없기 때문 |
| 6 | `error_envelope` | 오류 본문에 `requestId`를 채우고 모든 4xx·5xx를 스펙 모양으로 |
| 7 | `CatchPanicLayer` | 핸들러 패닉을 500 JSON으로 (연결이 끊기지 않게) |
| 8 | `TimeoutLayer` | 5초 넘는 요청을 408로 |
| 9 | CORS | |
| 10 | `DefaultBodyLimit` | 가장 안쪽. 2MB. (업로드 라우트만 더 크게 — 더 안쪽의 값이 이긴다) |

### 3-2. 🔍 순서를 잘못 잡아서 실제로 겪은 일 두 가지

**① 요청 ID가 일부 응답에서만 사라짐** — `PropagateRequestIdLayer`를 **맨 안쪽**에 두었을 때:

```
GET /health -> 200 | x-request-id=c0705f40-...  ✅
GET /panic  -> 500 | x-request-id=(없음)         ❌
GET /slow   -> 408 | x-request-id=(없음)         ❌
```

`CatchPanicLayer`와 `TimeoutLayer`는 **아래 계층의 응답을 버리고 자기가 새 응답을 만든다.** Propagate가 그보다 안쪽이면 그 새 응답은 Propagate를 거치지 않는다. 바깥으로 올리자 전부 해결됐다. 증상이 "**가끔** 헤더가 없다"라서 원인을 찾기 어려웠다.

**② 오류 본문 재작성이 gzip에서만 깨질 수 있음** — `error_envelope`(본문을 읽어 다시 쓰는 미들웨어)를 `CompressionLayer`보다 **안쪽**에 두면 응답이 이미 **압축된 상태**로 올라와 JSON으로 읽을 수 없다. 그래서 압축을 `error_envelope`보다 바깥(5번)에 두었고, 실제로 `accept-encoding: gzip` 요청에서도 `{"error":{"code":"NOT_FOUND",...,"requestId":"f391c147-..."}}`로 재작성되는 것을 확인했다 (study6).

> 일반화: **응답을 만들거나 고치는 계층의 순서**가 동작을 바꾼다. "이 계층은 아래 계층의 응답을 보는가, 못 보는가?"를 항상 묻는다.

### 3-3. 설정 — [demo/src/config.rs](demo/src/config.rs)

```rust
#[derive(Debug, Clone)]
pub struct Config { pub database_url: String, pub jwt_secret: String, pub bind_addr: String, pub upload_dir: String }

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            database_url: env_or("DATABASE_URL", DEFAULT_DATABASE_URL),
            jwt_secret:   env_or("JWT_SECRET", DEV_JWT_SECRET),
            bind_addr:    env_or("BIND_ADDR", "127.0.0.1:3000"),
            upload_dir:   env_or("UPLOAD_DIR", "uploads"),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn for_test() -> Self { /* in-memory DB, 고정 비밀키, 포트 0 */ }

    fn validate(&self) -> anyhow::Result<()> {
        if self.jwt_secret.len() < 16 { anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)"); }
        if self.jwt_secret == DEV_JWT_SECRET {
            tracing::warn!("기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요.");
        }
        Ok(())
    }
}
```

`main.rs`는 이 설정을 받아 **조립만** 한다:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::load_dotenv()?;          // ⚠️ 반드시 tracing 초기화보다 먼저
    init_tracing();

    let config = Config::from_env()?;
    let pool = db::init(&config.database_url).await?;
    upload::ensure_dir(&config.upload_dir).await?;

    let state = AppState::new(pool, config.jwt_secret.clone(), config.upload_dir.clone());
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;

    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())     // Ctrl+C/종료 신호를 받으면 진행 중 요청을 마치고 종료
        .await?;
    Ok(())
}
```

### 3-4. 중복을 줄이는 도구들 — 이 저장소에서 쓴 것

| 중복되던 것 | 모은 곳 | 효과 |
|---|---|---|
| `Json` 파싱 + 검증 + 오류 변환 | `extract::ValidatedJson` / `ValidatedQuery` | 핸들러마다 `validate()`를 부르다 잊는 일이 없다 |
| `Path`/`Query` 실패의 평문 400 | `extract::Path` / `Query` 래퍼 | 모든 실패가 `AppError` JSON |
| 오류 → 응답 변환 | `AppError: IntoResponse` 한 곳 | 핸들러는 `?`만 쓴다 |
| `Result<Json<T>, AppError>` 반복 | `type AppResult<T>` | 시그니처가 짧아진다 |
| "글이 있고 내 글인가" 확인 | `ensure_owner` | 수정·삭제·업로드가 모두 첫 줄에서 호출 |
| `sqlx::Error` → 409/500 변환 | `#[from]` + `map_unique_violation` | `?` 한 글자 |
| DB 종류 | `db::Db`, `DbPool` 별칭 | PostgreSQL로 바꿀 때 repo가 안 바뀐다 (study11) |
| 환경변수 읽기 | `Config` | 한 곳, 검증 포함 |

**복붙 대신 추출하는 신호**: 같은 3줄이 두 번째 나타났을 때. 그 전에 추상화하면 오히려 읽기 어려워진다.

### 3-5. 개발 / 운영 설정 나누기

| 항목 | 개발 | 운영 |
|---|---|---|
| 값을 넣는 곳 | `.env` 파일 (gitignore) | **실행 시 주입** (`docker run -e`, 시크릿 관리) |
| `JWT_SECRET` | 개발용 기본값(경고 출력) | 길고 무작위. 기본값이면 경고, 짧으면 **부팅 거부** |
| `BIND_ADDR` | `127.0.0.1:3000` (내 컴퓨터만) | `0.0.0.0:3000` (**127.0.0.1이면 컨테이너 밖에서 접속 불가**) |
| `DATABASE_URL` | `sqlite://data.db?mode=rwc` | 볼륨 위 SQLite 또는 `postgres://…` |
| `RUST_LOG` | `demo=debug,tower_http=debug` | `demo=info,tower_http=info` |
| 빌드 | `cargo run` (빠른 컴파일, 느린 실행) | `cargo build --release` (`lto="thin"`, `codegen-units=1`, `strip=true`) |
| 쿠키 `Secure` | 끔 (http) | **켬** (https) |
| Swagger UI | 켬 | 필요하면 인증 뒤에 두거나 끈다 |

운영용 기본값을 코드의 **기본값**으로 두지 않는다: 설정이 빠졌을 때 "안전한 쪽"으로 실패해야 하기 때문이다(예: 시크릿이 짧으면 부팅 실패).

> ⚠️ `Cargo.toml`의 `[profile.release]`에 `panic = "abort"`를 **켜면 안 된다.** `CatchPanicLayer`가 무력화되어 패닉 하나에 프로세스가 죽는다. 주석으로 남겨 둔 이유다.

---

## ④ 🔗 연동 포인트

### 개발·운영 설정을 나누는 흐름

```
.env.example (커밋)  ──복사──▶  .env (개발자 로컬, gitignore)
                                     │ dotenvy
실제 환경변수 (운영: docker -e / compose / CI 시크릿) ──우선──▶  Config::from_env()  ──▶  AppState
                                                                       │
                                                      validate(): 부적절하면 부팅 거부
```

- `compose.yaml`은 `JWT_SECRET: ${JWT_SECRET:?JWT_SECRET 환경변수를 설정하세요}` — **설정을 안 주면 컨테이너 시작 자체를 거부**한다.
- 같은 이미지(`Dockerfile`의 `ENV DATABASE_URL=… BIND_ADDR=0.0.0.0:3000`)가 개발·스테이징·운영에 그대로 간다. 달라지는 것은 **주입하는 값**뿐이다 (study10).

### 앞뒤 단계와 이어지는 것

- study9: `Config::for_test()`와 `lib.rs`의 `app()`이 **테스트의 입구**가 된다.
- study10: 위 표의 "운영" 열이 Dockerfile·compose의 환경변수가 된다.
- study11: `db.rs`만 바꾸면 DB가 바뀐다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 패닉·타임아웃 응답에만 `x-request-id`가 없음 | `PropagateRequestIdLayer`가 `CatchPanic`/`Timeout`보다 **안쪽** | 더 바깥(위)으로 올린다 |
| gzip을 받는 클라이언트에서만 오류 본문이 이상함 | 본문을 읽어 다시 쓰는 계층(`error_envelope`)이 `CompressionLayer`보다 **바깥(위)** 에 있어서 **이미 압축된 바이트**를 JSON으로 읽으려 함 | 본문을 읽는 계층은 압축보다 **안쪽(아래)** 에 둔다. 즉 `ServiceBuilder`에서 `CompressionLayer`가 위, `error_envelope`가 아래 |
| `.layer()`를 여러 번 부르면 순서가 거꾸로 | `Router::layer`를 연달아 부르면 **나중에 붙인 것이 바깥** | `ServiceBuilder`로 묶으면 **위에서 아래로** 읽힌다 |
| `TimeoutLayer::new`가 deprecated 경고 | API 변경 | 경고가 대체 API를 알려 준다: `TimeoutLayer::with_status_code(...)` |
| `TraceLayer`를 붙였는데 로그가 안 나옴 | `tracing`은 facade — **subscriber를 초기화하지 않으면** 전부 버려진다. 기본 필터가 `INFO`라 `DEBUG` 로그도 안 보임 | `tracing_subscriber::registry()...init()` + `RUST_LOG=tower_http=debug` |
| `.env`의 `RUST_LOG`가 무시됨 | `EnvFilter`는 초기화 시점에 한 번만 읽는데 `.env`를 그 뒤에 읽음. **에러가 안 나고** 동작만 달라진다 | `load_dotenv()` → `init_tracing()` 순서 |
| 테스트에서 `init_tracing()`을 두 번 불렀더니 패닉 | `init()`은 두 번째 호출에서 패닉 | `try_init()` (이 저장소가 쓰는 방식) |
| `Arc<String>: AsRef<Path>` 미충족 (`ServeDir::new(state.upload_dir.clone())`) | `Arc<T>`는 메서드 호출은 자동으로 통과하지만 **trait 제약은 자동 충족되지 않는다** | `state.upload_dir.as_str().to_string()`으로 풀어서 넘긴다 |
| `cannot find 'signal' in 'tokio'` — `found an item that was configured out` | tokio **feature 누락** | `cargo add tokio --features signal` |
| 컨테이너에서 실행했는데 밖에서 접속이 안 됨 | `BIND_ADDR=127.0.0.1` | `0.0.0.0:3000` |
| `[profile.release]`에 `panic = "abort"` 후 패닉 하나에 서버 종료 | `CatchPanicLayer`는 unwind가 필요 | 켜지 않는다 |
| 모듈이 서로를 import해서 순환 의존 | 공통 코드가 기능 폴더 안에 있음 | 횡단 코드(`error`, `extract`)는 기능 폴더와 **같은 레벨**의 모듈로 |

---

## ⑥ 직접 해보기

1. **`CORS_ORIGIN` 환경변수화**: 코드에 고정된 `http://localhost:5173`을 `Config`로 옮기고 쉼표로 여러 출처를 받게 한다. `validate()`에서 **파싱 가능한 URL인지** 시작 시점에 검사한다 (study6 ④의 ❌ 항목 하나가 사라진다).
2. **미들웨어 순서 실험**: `PropagateRequestIdLayer`를 일부러 맨 안쪽으로 옮기고 `/health`, `/panic`, `/slow`의 `x-request-id`를 `curl -i`로 비교한다. 다시 복구한다.
3. **중복 찾기**: `demo/src/`에서 같은 모양의 코드가 두 번 이상 나오는 곳을 하나 찾아 함수로 추출하고 테스트가 그대로 통과하는지 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ `handler / repo / model`과 횡단 모듈(`error`, `extract`, `middleware`, `config`)의 경계를 그림으로 그릴 수 있다
- ☐ lib + bin 구조의 이유(테스트가 같은 라우터를 쓴다)를 설명할 수 있다
- ☐ 설정을 `Config` 한 곳에서 읽고 **시작 시점에 검증**한다
- ☐ 우선순위 `실제 환경변수 > .env > 기본값`을 설명할 수 있다
- ☐ `.env`를 `tracing` 초기화보다 먼저 읽어야 하는 이유를 안다
- ☐ `ServiceBuilder`에서 "위가 바깥"이라는 규칙과 일반 원칙(전체 적용은 바깥, 조건부는 안쪽)을 설명할 수 있다
- ☐ 요청 ID가 패닉·타임아웃 응답에 빠지는 사례의 원인을 설명할 수 있다
- ☐ 개발/운영에서 달라지는 값(`BIND_ADDR`, `JWT_SECRET`, `RUST_LOG`, 빌드 프로필, `Secure` 쿠키)을 표 없이 말할 수 있다
- ☐ `panic = "abort"`를 켜면 안 되는 이유를 설명할 수 있다

---

## ⑧ 다음 단계 연결

구조가 정리됐으니 이제 **믿고 고칠 수 있게** 만든다. [study9](study9.md)에서는 단위 테스트와 `oneshot` 요청 테스트를 쓰고, 로그와 디버거로 문제를 추적하며, 테스트에서 DB·외부 API를 가짜로 바꾸는 방법을 다룬다.
