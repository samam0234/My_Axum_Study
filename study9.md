# study9 — 테스트 · 디버깅

> 🎯 **목표**: 단위 테스트와 **서버를 띄우지 않는** 요청 테스트(`oneshot`)를 작성하고, 로그·디버거로 문제를 추적하며, 지금까지 겪은 **자주 나는 오류 TOP 10**을 정리한다. 테스트에서 DB·외부 API를 **가짜(mock)** 로 바꾸는 방법을 익힌다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | 단위: [demo/src/pagination.rs](demo/src/pagination.rs) 하단, 요청 테스트: [demo/tests/api.rs](demo/tests/api.rs), mock: [lab/src/mock_demo.rs](lab/src/mock_demo.rs) |
| 실행 결과 | `demo`: 단위 12 + 통합 31 통과(약 1.6초). `lab`: mock 테스트 3 + 서명 검증 3 + 프롬프트/스트림 3 통과 |

---

## ① 목표

1. 같은 파일 안의 **단위 테스트**와 `tests/` 폴더의 **통합(요청) 테스트**를 구분해 쓴다.
2. `tower::ServiceExt::oneshot`으로 라우터에 요청을 **직접** 넣는다.
3. `RUST_LOG`, `--nocapture`, `RUST_BACKTRACE`, 디버거로 원인을 좁힌다.
4. 오류 TOP 10을 증상만 보고 원인까지 짚을 수 있다.
5. trait로 경계를 만들고 테스트에서 가짜 구현을 끼운다.

---

## ② 📖 이론

### 테스트의 층

| 층 | 무엇을 | 속도 | 이 저장소 |
|---|---|---|---|
| **단위** | 함수 하나 (offset 계산, 서명 검증) | 즉시 | `#[cfg(test)] mod tests` — **같은 파일 안** |
| **요청(통합)** | 라우터 + 미들웨어 + 핸들러 + DB를 요청 하나로 | 수 ms | `tests/api.rs` |
| **E2E** | 실제 서버 + 브라우저 | 느림 | `curl` 스모크, 브라우저 수동 확인 |

Rust는 단위 테스트를 **테스트 대상과 같은 파일**에 둔다. `#[cfg(test)]`라서 일반 빌드에는 포함되지 않고, 같은 모듈이라 비공개 함수도 시험할 수 있다. `tests/`의 각 파일은 **별도 크레이트**라서 `pub`인 것만 보인다 — 외부 사용자가 되어 쓰는 시험이다.

### `oneshot` — 서버 없이 요청 보내기

`Router`는 tower의 `Service`다. 소켓을 열지 않고 `router.oneshot(request)`로 요청을 **함수 호출처럼** 넣고 응답을 받는다.

- 포트 충돌이 없어 **병렬 실행**이 된다.
- 네트워크 왕복이 없어 빠르다 (통합 31개 + 단위 12개가 1.6초).
- 미들웨어 계층(요청 ID, 오류 재작성, 압축)까지 **전부 탄다.**
- 매 테스트가 **새 in-memory DB**를 만들어 서로 격리된다.

### Mock / Fake — 무엇을 가짜로 바꾸나

| 용어 | 뜻 | 예 |
|---|---|---|
| **Fake** | 실제로 동작하는 가벼운 대체물 | in-memory SQLite, `Vec`로 만든 저장소 |
| **Stub** | 정해진 답만 돌려주는 대체물 | 항상 `Ok(())`를 주는 알림기 |
| **Mock/Spy** | 호출 기록을 남겨 "불렸는가"를 검사 | `FakeNotifier.sent` |

**원칙**: 테스트가 의존하는 바깥세상(DB, 외부 HTTP)을 **trait 뒤로 숨기고**, 테스트에서는 그 trait의 가짜 구현을 넣는다. DI 컨테이너가 없으므로 "무엇을 넣을지"는 `router::<R, N>(state)`를 부르는 쪽이 **타입 파라미터로** 정한다.

---

## ③ 🛠️ 실기

### 3-1. 단위 테스트 — [demo/src/pagination.rs](demo/src/pagination.rs)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn query(page: u32, limit: u32) -> PageQuery { PageQuery { page, limit, q: None, sort: None, tag: None } }

    #[test]
    fn offset은_페이지와_크기로_계산된다() {
        assert_eq!(query(1, 10).offset(), 0);
        assert_eq!(query(2, 10).offset(), 10);
        assert_eq!(query(3, 25).offset(), 50);
    }

    #[test]
    fn total_pages는_올림_계산이다() {
        let page = Paged::new(Vec::<i32>::new(), &query(1, 10), 25);   // 25건 / 10 → 3페이지
        assert_eq!(page.meta.total_pages, 3);
    }

    #[test]
    fn limit이_50을_넘으면_50으로_줄인다() {          // 스펙 §4: 오류가 아니라 보정
        assert_eq!(query(1, 1000).effective_limit(), 50);
        assert_eq!(query(2, 1000).offset(), 50);       // 줄인 값으로 offset도 계산
    }

    #[test]
    fn 검증_규칙이_범위를_막는다() {
        use validator::Validate;
        assert!(query(1, 10).validate().is_ok());
        assert!(query(0, 10).validate().is_err());     // page >= 1
        assert!(query(1, 0).validate().is_err());      // limit >= 1
    }
}
```

- 함수 이름을 **한글 문장**으로 쓰면 실패 로그가 곧 명세서가 된다 (`test pagination::tests::offset은_페이지와_크기로_계산된다 ... ok`).
- 경계값(0, 1, 100, 101, 결과 0건)을 꼭 넣는다. 버그는 경계에서 난다.

### 3-2. 요청 테스트 — [demo/tests/api.rs](demo/tests/api.rs)

```rust
async fn test_app() -> Router {
    let config = Config::for_test();                      // sqlite::memory:, 고정 비밀키
    let pool = db::init(&config.database_url).await.expect("테스트 DB 초기화 실패");
    app(AppState::new(pool, config.jwt_secret, config.upload_dir))     // 운영과 같은 app()
}

/// 상태를 조금 바꿔서 앱을 만든다. (예: 호출 제한을 켠다) 기본 AppState::new는 제한 없음·기본 CORS 출처
async fn test_app_with(tweak: impl FnOnce(AppState) -> AppState) -> Router { /* test_app과 같고 tweak(...)만 거친다 */ }

async fn request(app: &Router, method: &str, uri: &str, body: Option<Value>, token: Option<&str>)
    -> (StatusCode, Value)
{
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token { builder = builder.header(header::AUTHORIZATION, format!("Bearer {t}")); }

    let request = match body {
        Some(json) => builder.header(header::CONTENT_TYPE, "application/json")
                             .body(Body::from(json.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };

    let response = app.clone().oneshot(request).await.unwrap();      // ← 서버 없이 호출
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, json)
}
```

| 코드 | 설명 |
|---|---|
| `Config::for_test()` | in-memory DB. 파일을 만들지 않고, 테스트가 끝나면 사라진다 |
| `app(AppState::new(...))` | **`main`과 같은 `app()`을 호출**한다. lib+bin 구조의 이유 |
| `app.clone().oneshot(request)` | `oneshot`은 서비스를 **소비**한다. `Router`의 `Clone`은 싸서 복제해 넘긴다. `use tower::ServiceExt;` 필요 |
| `.collect().await.to_bytes()` | 응답 본문은 스트림이다. `http_body_util::BodyExt`로 끝까지 모은다 |
| `(StatusCode, Value)` 반환 | 대부분의 시험이 "상태 코드 + JSON 필드 몇 개"만 보면 되므로 헬퍼로 줄인다 |

시험 목록 31개 (한 줄이 한 가지 약속이다. **공통 API 스펙 v1의 각 항목이 시험 하나씩 대응한다**):

```
[공통]   health_체크는_접두어_없이_200을_돌려준다          없는_경로는_통일된_404_json을_돌려준다
         접두어_없는_옛_경로는_더_이상_없다               메서드가_다르면_405다
         패닉해도_500_json이_나간다                      오류_본문의_request_id는_응답_헤더와_같다
[인증]   가입_응답은_data로_감싸고_비밀번호가_없다          로그인_응답은_data_user_access_token이다
         me는_data로_내_정보를_준다                      로그아웃은_인증이_필요하고_204다
         중복_이메일은_409_email_taken이다               중복_닉네임은_이메일_중복과_구분되는_409다
         가입_검증_실패는_fields에_필드별_메시지를_준다     로그인_실패_메시지는_이메일_존재_여부를_알려주지_않는다
[글]     토큰_없이는_글을_쓸_수_없다                      게시글_crud_전체_흐름 (201 + Location, 204 본문 없음)
         남의_글은_수정할_수_없다                         글_검증_실패는_400과_필드별_fields를_준다
         숫자가_아닌_id는_400_bad_request다               목록은_data와_meta를_준다
         검색어는_meta_q로_돌아오고_대소문자를_무시한다     limit이_50을_넘으면_오류가_아니라_50으로_줄인다
         잘못된_page_limit_sort는_400이다                 sort로_오래된_순과_댓글_많은_순을_고를_수_있다
         sql_인젝션_시도는_그냥_검색어로_취급된다           응답_json은_camel_case다
[댓글]   댓글_작성_목록_삭제_흐름                         없는_글의_댓글은_404고_빈_본문은_400이다
         글을_지우면_댓글도_함께_사라진다
[운영]   호출_제한을_넘으면_429_rate_limited와_retry_after를_준다     cors_출처는_설정값을_따른다
```

시험 하나의 모양:

```rust
#[tokio::test]
async fn 남의_글은_수정할_수_없다() {
    let app = test_app().await;
    let alice = signup_and_login(&app, "alice").await;
    let bob = signup_and_login(&app, "bob").await;

    let id = create_post(&app, &alice, "앨리스의 글", "본문").await;     // POST /api/posts → data.id

    let (status, body) = request(&app, "PATCH", &format!("/api/posts/{id}"),
        Some(json!({ "title": "탈취 시도" })), Some(&bob)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "FORBIDDEN");
}
```

실행:

```
$ cargo test
running 12 tests ... test result: ok. 12 passed
running 31 tests ... test result: ok. 31 passed; 0 failed; finished in 1.58s
```

### 3-3. DB·외부 API를 가짜로 — [lab/src/mock_demo.rs](lab/src/mock_demo.rs)

```rust
/// DB 접근 경계. 운영 구현은 sqlx 풀, 테스트 구현은 Vec.
pub trait OrderRepo: Clone + Send + Sync + 'static {
    fn insert(&self, item: String, qty: u32) -> impl Future<Output = Result<u32, String>> + Send;
}
/// 외부 API 경계 (메일·슬랙·결제 알림 …)
pub trait Notifier: Clone + Send + Sync + 'static {
    fn notify(&self, message: String) -> impl Future<Output = Result<(), String>> + Send;
}

#[derive(Clone)]
pub struct AppState<R, N> { pub repo: R, pub notifier: N }

pub fn router<R: OrderRepo, N: Notifier>(state: AppState<R, N>) -> Router {
    Router::new().route("/orders", post(create_order::<R, N>)).with_state(state)
}

async fn create_order<R: OrderRepo, N: Notifier>(State(state): State<AppState<R, N>>, Json(body): Json<NewOrder>) -> Response {
    if body.item.trim().is_empty() || body.qty == 0 { return StatusCode::BAD_REQUEST.into_response(); }

    let id = match state.repo.insert(body.item.clone(), body.qty).await {
        Ok(id) => id,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // 외부 서비스 장애가 핵심 기능(주문 저장)을 막으면 안 된다.
    let notified = state.notifier.notify(format!("새 주문 #{id}: {} x {}", body.item, body.qty)).await.is_ok();
    (StatusCode::CREATED, Json(OrderCreated { id, notified })).into_response()
}

// ── 테스트용 가짜 ──
#[derive(Clone, Default)]
pub struct MemRepo { pub rows: Arc<Mutex<Vec<(u32, String, u32)>>> }

#[derive(Clone, Default)]
pub struct FakeNotifier { pub sent: Arc<Mutex<Vec<String>>>, pub fail: bool }
```

| 코드 | 설명 |
|---|---|
| `trait OrderRepo` / `Notifier` | **경계.** 핸들러는 sqlx도 reqwest도 모른다. trait만 안다 |
| `impl Future<…> + Send` | trait 안의 `async` 메서드(RPITIT). `Send`를 요구해야 tokio가 다른 스레드로 옮길 수 있다 |
| `router::<R, N>(state)` | **무엇을 주입할지를 타입으로 정한다.** 운영은 `router(AppState { repo: SqlRepo(..), notifier: HttpNotifier(..) })`, 테스트는 `MemRepo`/`FakeNotifier` |
| `create_order::<R, N>` | 제네릭 핸들러는 등록할 때 타입을 명시한다 |
| `Arc<Mutex<Vec<…>>>` | 가짜가 **내부 상태를 공유**하므로 테스트가 `repo.clone()`을 들고 있다가 나중에 `rows`를 검사할 수 있다 |
| `FakeNotifier { fail: true }` | **외부 서비스 장애를 연출**한다. 실제 서비스를 끊을 필요가 없다 |

세 가지 시험이 약속을 고정한다:

```
test mock_demo::tests::주문하면_저장되고_알림이_간다 ... ok          ← repo.rows 1건, sent[0] == "새 주문 #1: keyboard x 2"
test mock_demo::tests::알림이_실패해도_주문은_성공한다 ... ok          ← 201 + notified:false
test mock_demo::tests::잘못된_입력은_저장도_알림도_하지_않는다 ... ok    ← 400, rows·sent 모두 비어 있음
```

`demo/`에서의 같은 원리: **DB는 in-memory SQLite가 곧 fake**다. 그래서 별도 mock 없이도 진짜 SQL과 마이그레이션까지 시험한다. 외부 HTTP(결제·AI 서버)는 위 trait 방식으로 갈아 끼운다 (study13, study18).

### 3-4. 디버깅 도구

| 도구 | 쓰는 법 | 언제 |
|---|---|---|
| `cargo test -- --nocapture` | 테스트의 `println!`/로그를 그대로 출력 | 시험이 왜 실패하는지 볼 때 |
| `cargo test 이름조각` | 이름에 조각이 들어간 시험만 | 한 개만 반복 실행 |
| `RUST_BACKTRACE=1` | 패닉 시 호출 스택 출력 (`=full`은 더 자세히) | 패닉 위치를 모를 때 |
| `RUST_LOG=demo=debug,tower_http=debug,axum::rejection=trace` | 로그 레벨 조절. `axum::rejection=trace`는 **extractor가 거절한 이유**를 보여 준다 | 400/415/422의 원인 |
| `dbg!(&값)` | 파일·줄·값을 stderr에 | 값 하나가 궁금할 때 |
| `#[axum::debug_handler]` | `Handler is not satisfied`의 **정확한 원인**을 컴파일러가 짚게 한다 | 핸들러 시그니처 에러 |
| `curl -v` / `-i` | 요청·응답 헤더까지 | 상태 코드·헤더·쿠키 |
| `x-request-id` | 응답 헤더 = 오류 본문 `requestId` = 서버 로그의 같은 ID | 한 요청을 추적 |
| **CodeLLDB** (VS Code) | 줄 왼쪽 클릭으로 중단점 → "Debug" | 변수 값을 단계별로 |

CodeLLDB 시작 설정(`.vscode/launch.json`, 저장소에는 커밋하지 않음):

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "demo 디버그",
      "cargo": { "args": ["build", "--manifest-path", "demo/Cargo.toml"] },
      "cwd": "${workspaceFolder}/demo",
      "env": { "RUST_LOG": "demo=debug" }
    }
  ]
}
```

### 3-5. 로그 읽는 법

```
2026-11-22T15:09:36Z  WARN demo::config: 기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요.
2026-11-22T15:09:36Z  INFO demo: 설정을 불러왔습니다 config.bind_addr="127.0.0.1:3100"
```

`시각  레벨  모듈경로: 메시지 key=value …` — **모듈 경로**로 어느 계층의 로그인지, **레벨**로 심각도(ERROR > WARN > INFO > DEBUG > TRACE)를 본다. 구조적 필드(`key=value`)는 `tracing::info!(addr = %listener.local_addr()?, "listening")`처럼 남긴다. **500의 원인은 응답이 아니라 서버 로그에 있다** (응답은 일부러 고정 문구).

### 3-6. ⭐ 자주 나는 오류 TOP 10 (이 저장소에서 실제로 겪은 것)

| # | 증상 | 원인 | 해결 | 단계 |
|---|---|---|---|---|
| 1 | 실행하자마자 패닉 `Path segments must not start with ':'` | axum 0.8: `:id` → `{id}` | 경로를 `{id}`로. 컴파일은 통과하고 **실행할 때** 알려 줌 | study1 |
| 2 | `the trait bound ... Handler<_, _> is not satisfied` | ① 본문 extractor가 마지막이 아님 ② `AppState`에 `Clone` 없음 ③ `.with_state()` 누락 | `#[axum::debug_handler]`로 원인 확인 | study1·2·5 |
| 3 | `target\debug\*.exe: Access is denied (os error 5)` | 서버가 실행 중인 채 재빌드 (Windows 파일 잠금) | 서버 종료 후 빌드 | study0 |
| 4 | 로그인 요청에서 `connection closed unexpectedly` (500도 아님) | jsonwebtoken 11에 `rust_crypto` feature 없음 → 런타임 패닉 → 연결 끊김 | `cargo add jsonwebtoken --features rust_crypto`. 이 일 때문에 `CatchPanicLayer`를 붙임 | study7·8 |
| 5 | 테스트가 랜덤하게 `no such table` | in-memory SQLite는 연결마다 별개의 DB, 풀 연결 2개 이상 | `max_connections(1)` | study6 |
| 6 | `dynamic SQL strings should be audited for possible injections` | sqlx 0.9: `format!`로 만든 SQL 거부 | `$1` 바인딩, 동적 조건은 `QueryBuilder` | study6 |
| 7 | `unable to open database file` / `database is locked` | `create_if_missing` 기본 false / WAL·`busy_timeout` 없음 | 연결 옵션 3종 세트 | study6 |
| 8 | `found an item that was configured out` | tokio(등) **feature 누락** | `cargo add tokio --features signal` | study8 |
| 9 | `.env`의 `RUST_LOG`가 조용히 무시됨 | `EnvFilter`는 초기화 시점에 한 번 읽음, `.env`를 뒤에 읽음 | `load_dotenv()` → `init_tracing()` | study0·8 |
| 10 | SPA 딥링크가 404 (`ServeDir::not_found_service`) | `not_found_service`는 상태를 **404로 강제** | `.fallback(...)`을 쓴다 (소스를 열어 확인) | study12 |

> 이 10개의 공통점: **버전 차이**가 절반이다. 블로그 예제를 붙여넣기 전에 `Cargo.toml`의 버전부터 확인하고, 의심스러우면 `cargo doc --open`으로 **내 버전의** 문서를 연다. 그리고 **컴파일러 메시지는 대부분 해결책을 말해 준다**(`note:`, `help:` 줄을 끝까지 읽는다).

---

## ④ 🔗 연동 포인트 — 테스트에서 DB·API를 가짜로 바꾸기

| 바깥세상 | 운영 | 테스트 | 방법 |
|---|---|---|---|
| **DB (sqlx)** | 파일 SQLite / PostgreSQL | **in-memory SQLite** | `Config::for_test()` → `sqlite::memory:` (`max_connections(1)`) |
| **DB (trait로 감싼 경우)** | `SqlRepo` | `MemRepo` (`Vec`) | `router::<MemRepo, _>` |
| **외부 HTTP API** (결제·메일·AI) | `reqwest` 기반 구현 | `FakeNotifier`, 또는 **같은 프로세스의 가짜 HTTP 서버** | trait 경계 + 가짜 구현 (study13: 가짜 OAuth 서버, study18: 가짜 AI 서버) |
| **시간·난수** | `Utc::now()`, `Uuid::new_v4()` | 고정값 | 값을 인자로 받거나 trait로 주입 |
| **파일 시스템** | `uploads/` | 임시 디렉터리 | `Config.upload_dir`을 테스트에서 따로 지정 |
| **PostgreSQL** | 실제 서버 | `TEST_DATABASE_URL` | `cargo test --no-default-features --features postgres` (study11) |

CI(study16)에서는 `cargo test`가 **서버·네트워크 없이** 돌아야 한다. 위 가짜들이 그것을 가능하게 한다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| `tests/`에서 `use demo::...`가 안 됨 | `main.rs`만 있고 `lib.rs`가 없음 | lib+bin 구조로 (study8). `Cargo.toml`의 `[lib]` 이름 확인 |
| `oneshot` 호출이 `no method named oneshot` | `tower::ServiceExt`를 import하지 않음 | `use tower::ServiceExt;` + dev-dependency에 `tower = { features = ["util"] }` |
| 응답 본문을 못 읽음 | `BodyExt`가 없음 | `http-body-util`을 dev-dependency에 추가하고 `use http_body_util::BodyExt;` |
| 테스트가 가끔만 실패 | 시험들이 **같은 파일·같은 DB·같은 포트**를 공유 | 시험마다 in-memory DB, 서버 대신 `oneshot`. 환경변수를 바꾸는 시험은 직렬로 |
| 한 시험의 `init_tracing()`이 패닉 | `init()`은 두 번째 호출에서 패닉 | `try_init()` |
| 한글 시험 이름이 깨짐 (PowerShell) | 터미널 인코딩 | 시험은 정상. 보기만 UTF-8로 (`chcp 65001`, `Get-Content -Encoding UTF8`) |
| `cargo test`가 `tests/api.rs`를 건너뜀 | `--no-default-features --features postgres`로 빌드하면 `#![cfg(feature = "sqlite")]`로 제외됨 | 의도된 동작. PostgreSQL 시험은 별도 DB가 필요 |
| mock이 항상 성공해서 **실제 장애 경로를 못 시험** | 가짜에 `fail` 스위치가 없음 | 실패·지연·잘못된 응답을 연출하는 필드를 둔다 (`FakeNotifier.fail`) |
| 가짜와 진짜가 **다르게 동작**해서 운영에서만 버그 | 가짜가 현실을 단순화함 (예: `LIKE` 대소문자) | 가능하면 **진짜 DB**(in-memory SQLite, 컨테이너 PostgreSQL)로 핵심 경로를 시험. study11에서 SQLite/PostgreSQL 차이를 실제로 겪었다 |
| 타임아웃 시험이 5초씩 걸림 | `/slow`가 진짜로 5초 대기 | 시험용으로 타임아웃 값을 주입 가능하게 하거나, 그 시험만 따로 둔다 |

---

## ⑥ 직접 해보기

1. **시험 먼저 쓰기**: `GET /api/posts?page=0`이 400 `VALIDATION_ERROR`와 `fields.page`를 주는 시험을 **먼저** 쓰고 실패(빨강)를 확인한 뒤, 통과(초록)하는지 본다. 이미 통과한다면 왜 통과하는지 `PageQuery`의 `validate`에서 찾는다.
2. **`FakeNotifier` 활용**: `lab/src/mock_demo.rs`에 "알림 메시지에 수량이 0이면 안 나간다" 같은 규칙을 하나 추가하고, `fail: true`와 정상 두 경로를 시험한다.
3. **`RUST_LOG=axum::rejection=trace`**: 서버를 띄우고 잘못된 JSON을 보내 거절 이유가 로그에 찍히는 것을 확인한다. 같은 요청이 응답에서는 어떻게 보이는지 비교한다.

---

## ⑦ ☐ 체크리스트

- ☐ 단위 테스트(같은 파일)와 통합 테스트(`tests/`)의 차이를 설명할 수 있다
- ☐ `oneshot`이 서버 없이 라우터를 호출한다는 것과 그 장점(병렬·빠름·미들웨어 포함)을 안다
- ☐ `cargo test`를 실행해 `demo` 43개(단위 12 + 통합 31)가 통과하는 것을 확인했다
- ☐ 호출 제한·CORS처럼 **설정에 따라 달라지는 동작**은 `test_app_with(|s| s.with_…)`로 상태를 바꿔 시험한다는 것을 안다
- ☐ 시험마다 새 in-memory DB를 쓰는 이유(격리)를 설명할 수 있다
- ☐ `RUST_LOG`, `--nocapture`, `RUST_BACKTRACE`, `#[axum::debug_handler]`를 상황에 맞게 고를 수 있다
- ☐ 로그 한 줄에서 시각·레벨·모듈·필드를 읽을 수 있고, 500의 원인은 서버 로그에 있다는 것을 안다
- ☐ 오류 TOP 10 중 증상을 보고 원인을 말할 수 있는 것이 8개 이상이다
- ☐ trait 경계 + 가짜 구현으로 DB·외부 API를 갈아 끼우는 코드를 직접 만들어 봤다
- ☐ 가짜가 현실을 단순화해서 생기는 위험(예: `LIKE` 대소문자)을 안다

---

## ⑧ 다음 단계 연결

시험이 통과하면 **밖으로 내보낼** 차례다. [study10](study10.md)에서는 릴리스 빌드, 멀티 스테이지 Docker 이미지, 도메인·HTTPS 개념, 배포 후 점검표를 정리하고 기초 구간(study0~10)을 마무리한다.
