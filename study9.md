# 테스트 · 설정 · 로깅 (Study 9 · 고급 4)

지금까지 기능을 확인하는 방법은 **서버를 띄우고 PowerShell로 요청을 쏘는 것**이었습니다. 매번 수동이고, DB 상태가 이전 테스트에 오염되고, 무엇이 깨졌는지 자동으로 알 수 없습니다.

이 문서에서 그걸 자동화하고, 흩어져 있던 설정을 한곳으로 모읍니다.

**이 문서에서 하는 것**: 프로젝트를 lib + bin으로 재구성 → `Config` 도입 → 유닛 테스트 → `oneshot` 통합 테스트 → graceful shutdown

```powershell
cargo add dotenvy
cargo add tokio --features signal
cargo add --dev tower --features util
cargo add --dev http-body-util
cargo add --dev serde_json
```

---

## 1. ⭐ 먼저 구조를 바꾼다 — lib + bin

Rust에서 **통합 테스트는 `tests/` 폴더의 별도 크레이트**로 컴파일되고, 대상 크레이트를 **외부 사용자처럼** 가져다 씁니다. 그런데 지금 우리 코드는 전부 `main.rs`(바이너리 크레이트) 안에 있어서 **밖에서 `import`할 수가 없습니다.**

해결책은 표준적입니다. **앱 로직을 라이브러리 크레이트로 옮기고, 바이너리는 얇게 만듭니다.**

```
src/
├─ lib.rs      ← 라이브러리 크레이트 루트. 모듈 선언 + app() 조립
├─ main.rs     ← 바이너리 크레이트 루트. 설정 로딩 + 서버 실행만
├─ config.rs   ← 새로 추가
├─ routes.rs   ← main.rs에 있던 데모 핸들러들을 분리
└─ ... (나머지는 그대로)

tests/
└─ api.rs      ← 통합 테스트 (별도 크레이트)
```

`Cargo.toml`을 고칠 필요는 없습니다. **Cargo가 `src/lib.rs`와 `src/main.rs`를 자동으로 인식**합니다. 이름이 같아도(`demo`) 타깃 종류가 달라서 충돌하지 않습니다.

### `src/lib.rs`

```rust
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
// ... 전부 pub

/// 라우터 조립. `main`에서도, 통합 테스트에서도 **이 함수 하나**를 쓴다.
pub fn app(state: AppState) -> Router { /* study7에서 만든 그대로 */ }

pub fn init_tracing() { /* ... */ }
```

- `mod` → **`pub mod`** 로 바꿔야 테스트에서 보입니다.
- **study1부터 `fn app()`을 따로 빼둔 것이 여기서 값을 합니다.** 미리 그렇게 안 했다면 지금 라우터 조립 코드를 통째로 옮겨야 했을 겁니다.

### `src/main.rs` — 얇아졌다

```rust
use demo::{app, config, config::Config, db, init_tracing, state::AppState, upload};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::load_dotenv()?;
    init_tracing();

    let config = Config::from_env()?;
    let pool = db::init(&config.database_url).await?;
    upload::ensure_dir(&config.upload_dir).await?;

    let state = AppState::new(pool, config.jwt_secret.clone(), config.upload_dir.clone());
    let app = app(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}
```

> 자기 크레이트를 `use demo::...`로 참조하는 게 어색한데, **바이너리 입장에서 라이브러리는 외부 의존성**이기 때문입니다. (라이브러리 안에서는 `crate::`를 씁니다)

### Graceful shutdown

```rust
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("종료 신호를 받았습니다"),
        Err(e) => tracing::error!(%e, "종료 신호를 기다리지 못했습니다"),
    }
}
```

Ctrl+C를 받으면 **새 연결은 거부하되 처리 중인 요청은 끝까지 마치고** 종료합니다. 배포 중 요청이 중간에 끊기는 걸 막습니다. (`tokio` `signal` feature 필요)

---

## 2. 설정을 한곳으로 — `Config`

지금까지는 이랬습니다.

```rust
let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data.db?mode=rwc".into());
// ... 30줄 뒤 ...
let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-only-...".into());
```

**"이 앱이 어떤 환경변수를 쓰는가"를 알려면 코드 전체를 뒤져야 합니다.** 한곳으로 모읍니다.

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub bind_addr: String,
    pub upload_dir: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            database_url: env_or("DATABASE_URL", "sqlite://data.db?mode=rwc"),
            jwt_secret: env_or("JWT_SECRET", DEV_JWT_SECRET),
            bind_addr: env_or("BIND_ADDR", "127.0.0.1:3000"),
            upload_dir: env_or("UPLOAD_DIR", "uploads"),
        };
        config.validate()?;
        Ok(config)
    }

    /// 테스트용 — in-memory DB
    pub fn for_test() -> Self { /* ... */ }

    fn validate(&self) -> anyhow::Result<()> {
        if self.jwt_secret.len() < 16 {
            anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)");
        }
        if self.jwt_secret == DEV_JWT_SECRET {
            tracing::warn!("기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 설정하세요.");
        }
        Ok(())
    }
}
```

### 시작할 때 검증한다

**잘못된 설정은 서버가 뜨기 전에 죽는 게 낫습니다.** "3시간 뒤 첫 로그인 요청에서 발견"보다 훨씬 좋습니다. `anyhow::bail!`은 "에러를 만들어 즉시 반환"하는 매크로입니다.

실행 로그에서 경고가 실제로 보입니다.

```
WARN demo::config: 기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요.
```

### `.env`와 `.env.example`

```ini
# .env.example  (이건 커밋한다)
DATABASE_URL=sqlite://data.db?mode=rwc
JWT_SECRET=dev-only-insecure-secret-change-me
BIND_ADDR=127.0.0.1:3000
UPLOAD_DIR=uploads
RUST_LOG=demo=debug,tower_http=debug
```

- **`.env`는 `.gitignore`에, `.env.example`은 커밋**합니다. 새로 합류한 사람이 "무슨 변수가 필요한지" 알 수 있어야 합니다.
- `dotenvy::dotenv()`는 **이미 설정된 환경변수를 덮어쓰지 않습니다.** 우선순위는 **실제 환경변수 > `.env` > 코드 기본값**. 컨테이너 배포에서 `.env` 없이 환경변수만 줘도 그대로 동작합니다.

| | Spring Boot | NestJS | Axum |
|---|---|---|---|
| 도구 | `application.yml` + `@Value` | `@nestjs/config` + `ConfigService` | `dotenvy` + 직접 만든 구조체 |
| 프로파일 | `application-prod.yml` | `NODE_ENV` | 직접 (`APP_ENV` 등) |
| 검증 | `@ConfigurationProperties` + `@Validated` | Joi 스키마 | **직접 `validate()`** |

Axum은 여기서도 프레임워크가 해주는 게 없습니다. 대신 **구조체 하나라서 어디서 뭐가 읽히는지 100% 명확**합니다.

---

## 3. 유닛 테스트 — 같은 파일 안에

Rust는 테스트를 **테스트 대상과 같은 파일**에 둡니다. 처음엔 이상하지만 곧 편해집니다.

```rust
// src/pagination.rs 맨 아래
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_pages는_올림_계산이다() {
        // 25건을 10개씩 → 3페이지 (2.5가 아니라)
        let page = Page::new(Vec::<i32>::new(), &query(1, 10), 25);
        assert_eq!(page.total_pages, 3);
        assert!(page.has_next);
    }

    #[test]
    fn 결과가_없으면_total_pages는_0이다() {
        let page = Page::new(Vec::<i32>::new(), &query(1, 10), 0);
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_next);
    }
}
```

| 요소 | 의미 |
|---|---|
| `#[cfg(test)]` | **`cargo test`일 때만 컴파일**된다. 릴리스 바이너리에 안 들어감 |
| `mod tests` | 관례적인 이름. 같은 파일 안의 **비공개 항목에도 접근 가능** |
| `use super::*;` | 부모 모듈(= 테스트 대상)의 항목을 전부 가져옴 |
| `#[test]` | 테스트 함수 표시 |

> 💡 **함수 이름을 한글로 쓸 수 있습니다.** Rust 식별자는 유니코드를 허용합니다. 테스트 이름은 실행 결과에 그대로 출력되므로 **"무엇을 검증하는지" 문장으로 쓰면 리포트가 곧 명세서**가 됩니다. (일반 코드에서는 영어를 쓰는 게 관례입니다)

---

## 4. ⭐ 통합 테스트 — `oneshot`으로 서버 없이

`tests/api.rs`:

```rust
use axum::{Router, body::Body, http::{Request, StatusCode, header}};
use demo::{app, config::Config, db, state::AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;   // oneshot

async fn test_app() -> Router {
    let config = Config::for_test();          // sqlite::memory:
    let pool = db::init(&config.database_url).await.expect("테스트 DB 초기화 실패");
    app(AppState::new(pool, config.jwt_secret, config.upload_dir))
}
```

### 서버를 띄우지 않는다

```rust
let response = app.clone().oneshot(request).await.unwrap();
```

`Router`는 tower `Service`입니다. **`oneshot`은 요청 하나를 그 `Service`에 직접 밀어 넣고 응답을 받습니다.** TCP도, 포트도, 네트워크도 없습니다.

| | 실제 서버를 띄우는 테스트 | `oneshot` |
|---|---|---|
| 포트 충돌 | 있음 (병렬 실행 어려움) | **없음** |
| 속도 | 네트워크 왕복 | **함수 호출** |
| 정리 | 프로세스 종료 관리 필요 | 자동 |
| 커버리지 | 미들웨어 포함 전 구간 | **미들웨어 포함 전 구간** (동일) |

Spring의 MockMvc, Nest의 Supertest와 같은 자리입니다. **`app()` 함수를 그대로 쓰므로 미들웨어 계층까지 전부 테스트됩니다.**

- `app.clone()`이 필요한 이유: `oneshot`은 `self`를 소비합니다. `Router`의 `Clone`은 저렴하므로 문제없습니다.

### 테스트마다 새 DB

```rust
let config = Config::for_test();   // database_url = "sqlite::memory:"
```

**in-memory SQLite는 테스트 격리에 완벽합니다.** 파일도 안 남고, 마이그레이션부터 새로 돌고, 테스트 간 순서 의존이 없습니다.

단, `db::init`에 처리가 필요합니다.

```rust
// in-memory DB는 **연결마다 별개의 DB**가 만들어진다.
// 풀에 연결이 2개 이상이면 "어떤 연결을 받았느냐"에 따라 테이블이 있기도 없기도 하다.
let in_memory = database_url.contains(":memory:") || database_url.contains("mode=memory");

let pool = SqlitePoolOptions::new()
    .max_connections(if in_memory { 1 } else { 5 })
    // ...
```

**이걸 모르면 "테스트가 가끔 `no such table: posts`로 실패하는" 지옥을 겪습니다.** 마이그레이션은 연결 A에서 돌았는데 쿼리는 연결 B로 나가기 때문입니다.

### 헬퍼로 반복을 줄인다

```rust
async fn request(app: &Router, method: &str, uri: &str, body: Option<Value>, token: Option<&str>)
    -> (StatusCode, Value)
{
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let request = match body {
        Some(json) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() { Value::Null }
               else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, json)
}
```

`response.into_body().collect().await.unwrap().to_bytes()` — axum 0.7+의 본문은 **스트림**이라 한 번에 못 읽습니다. `http_body_util::BodyExt::collect()`로 모읍니다.

### 실제 테스트들

```rust
#[tokio::test]                              // ← #[test]가 아니라 #[tokio::test]
async fn 게시글_crud_전체_흐름() {
    let app = test_app().await;
    let token = signup_and_login(&app, "alice").await;

    let (status, created) = request(&app, "POST", "/posts",
        Some(json!({ "title": "첫 글", "content": "본문입니다", "tags": ["rust"] })),
        Some(&token)).await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["author"], "alice");   // 작성자는 토큰에서 나온다

    let id = created["id"].as_i64().unwrap();
    // ... 조회 → 수정 → 삭제 → 404 확인
}

#[tokio::test]
async fn 남의_글은_수정할_수_없다() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;
    // alice가 쓴 글을 bob이 수정 시도 → 403
}

#[tokio::test]
async fn 로그인_실패_메시지는_아이디_존재_여부를_알려주지_않는다() {
    // ...
    assert_eq!(s1, StatusCode::UNAUTHORIZED);
    assert_eq!(s2, StatusCode::UNAUTHORIZED);
    assert_eq!(b1, b2);   // 두 응답이 완전히 같아야 한다
}
```

**마지막 테스트가 특히 좋습니다.** study6에서 "메시지를 구분하면 안 된다"고 정한 보안 규칙을 **자동으로 지켜지게 만들었습니다.** 누군가 나중에 "디버깅하기 불편하네" 하며 메시지를 바꾸면 테스트가 깨집니다. — **테스트는 문서이자 가드레일입니다.**

---

## 5. 실행 결과

```powershell
cargo test
```

```
running 5 tests
test pagination::tests::offset은_페이지와_크기로_계산된다 ... ok
test pagination::tests::total_pages는_올림_계산이다 ... ok
test pagination::tests::결과가_없으면_total_pages는_0이다 ... ok
test pagination::tests::마지막_페이지에서는_has_next가_false다 ... ok
test pagination::tests::검증_규칙이_범위를_막는다 ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests\api.rs

running 13 tests
test health_체크는_200을_돌려준다 ... ok
test 메서드가_다르면_405다 ... ok
test 페이지_파라미터_범위를_벗어나면_422다 ... ok
test 토큰_없이는_글을_쓸_수_없다 ... ok
test 패닉해도_500_json이_나간다 ... ok
test 없는_경로는_통일된_404_json을_돌려준다 ... ok
test 게시글_crud_전체_흐름 ... ok
test sql_인젝션_시도는_그냥_검색어로_취급된다 ... ok
test 검증_실패는_422와_필드별_상세를_준다 ... ok
test 페이지네이션이_동작한다 ... ok
test 로그인_실패_메시지는_아이디_존재_여부를_알려주지_않는다 ... ok
test 중복_가입은_409다 ... ok
test 남의_글은_수정할_수_없다 ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.71s
```

**18개 테스트가 1.7초 만에 끝납니다.** 각각 독립된 in-memory DB를 쓰면서도요. study1~8에서 손으로 확인했던 것들이 이제 전부 자동입니다.

### 자주 쓰는 테스트 명령

```powershell
cargo test                      # 전부
cargo test 페이지네이션            # 이름에 그 문자열이 들어간 것만
cargo test --lib                # 유닛 테스트만
cargo test --test api           # tests/api.rs만
cargo test -- --nocapture       # println!/로그 출력 보기 (기본은 감춤)
cargo test -- --test-threads=1  # 순차 실행 (병렬이 기본)
```

> `cargo test`는 **기본적으로 테스트를 병렬 실행**합니다. 그래서 공유 자원(같은 DB 파일, 같은 포트)을 쓰면 깨집니다. in-memory DB + `oneshot` 조합이 이 문제를 애초에 없앱니다.

---

## 6. 처음 만난 오류

### 6-1. ⭐ `.env`의 `RUST_LOG`가 적용되지 않았다

`.env`에 `RUST_LOG=demo=info`를 넣고 실행했는데 DEBUG 로그가 그대로 나왔습니다.

```
DEBUG demo::config: .env 파일을 읽었습니다 ...
DEBUG request: tower_http::trace::on_request: started processing request ...
```

**원인은 순서였습니다.**

```rust
init_tracing();               // ① EnvFilter가 여기서 RUST_LOG를 읽는다 (아직 .env 미적용)
let config = Config::from_env()?;   // ② 이 안에서 dotenv()를 호출했다 — 너무 늦었다
```

`EnvFilter`는 **초기화 시점의 환경변수를 한 번만** 읽고 고정됩니다. `.env`를 그 뒤에 읽어봐야 소용이 없습니다.

**해결**: `.env` 로딩을 별도 함수로 빼서 가장 먼저 부릅니다.

```rust
config::load_dotenv()?;   // ← 가장 먼저
init_tracing();
let config = Config::from_env()?;
```

수정 후 로그 (DEBUG 줄이 사라졌습니다):

```
WARN demo::config: 기본 JWT_SECRET을 쓰고 있습니다. ...
INFO demo: 설정을 불러왔습니다 config.bind_addr="127.0.0.1:3001"
INFO demo: database ready database_url=sqlite://data.db?mode=rwc
INFO demo: listening addr=127.0.0.1:3001
```

> 이런 버그는 **에러가 안 나고 조용히 동작이 달라져서** 발견이 늦습니다. "왜 로그 레벨이 안 먹지?" 하고 한참 헤맬 수 있습니다.

### 6-2. `Arc<String>: AsRef<Path> is not satisfied`

```rust
.nest_service("/uploads", ServeDir::new(state.upload_dir.clone()))
```

```
error[E0277]: the trait bound `Arc<String>: AsRef<Path>` is not satisfied
```

`ServeDir::new`는 `AsRef<Path>`를 요구하는데 `Arc<String>`은 그걸 구현하지 않습니다. `&str`을 거쳐 `String`으로 풀어주면 됩니다.

```rust
let upload_dir = state.upload_dir.as_str().to_string();
```

> `Arc<T>`는 `Deref<Target = T>`라서 **메서드 호출은 자동으로 통과**하지만, **trait 제약은 자동으로 만족되지 않습니다.** 이 둘의 차이가 Rust 초심자에게 가장 헷갈리는 지점 중 하나입니다.

### 6-3. `cannot find 'signal' in 'tokio'`

```
error[E0433]: cannot find `signal` in `tokio`
note: found an item that was configured out
  --> tokio-1.53.1\src\lib.rs:553:13
```

**"found an item that was configured out"** — 존재하는데 **feature가 꺼져 있다**는 뜻입니다. 컴파일러가 아주 친절하게 알려줍니다.

```powershell
cargo add tokio --features signal
```

tokio는 기능이 feature로 잘게 쪼개져 있어서 이 에러를 자주 만납니다. 메시지의 이 문구를 보면 바로 feature 문제임을 알 수 있습니다.

### 6-4. `tracing_subscriber ... init()`을 테스트에서 두 번 부르면 패닉

```rust
pub fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().compact())
        .try_init();       // ← init()이 아니라 try_init()
}
```

전역 subscriber는 **한 번만** 설정할 수 있습니다. `init()`은 두 번째 호출에서 패닉하므로, 테스트에서 여러 번 불릴 수 있는 함수는 `try_init()`을 쓰고 결과를 무시합니다.

---

## 7. 이번 study 정리

- 통합 테스트를 하려면 **lib + bin 구조**가 필요하다. 앱 조립은 `lib.rs`의 `pub fn app()`에, `main.rs`는 얇게.
- **`fn app()`을 처음부터 분리해 둔 것**이 여기서 값을 한다.
- 설정은 **`Config` 구조체 하나**로 모으고 **시작할 때 검증**한다. `.env`는 무시, `.env.example`은 커밋.
- **`.env` 로딩은 `init_tracing()`보다 먼저.** `EnvFilter`는 초기화 시점의 환경변수만 본다.
- 유닛 테스트는 `#[cfg(test)] mod tests`로 **같은 파일 안**에. 함수 이름을 한글 문장으로 쓰면 리포트가 명세서가 된다.
- 통합 테스트는 **`tower::ServiceExt::oneshot`** 으로 서버 없이. 포트 충돌이 없고 빠르며 미들웨어까지 전부 탄다.
- **in-memory SQLite는 반드시 `max_connections(1)`.** 연결마다 별개의 DB라서 안 그러면 랜덤하게 깨진다.
- 보안 규칙(로그인 메시지 동일성 등)을 **테스트로 고정**해 두면 나중에 누가 되돌리는 걸 막을 수 있다.
- `axum::serve(...).with_graceful_shutdown(...)`으로 배포 중 요청이 끊기지 않게 한다.

다음: [study10.md](study10.md) — OpenAPI 문서화와 Docker 배포. (고급 5)
