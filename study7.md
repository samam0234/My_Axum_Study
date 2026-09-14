# tower 미들웨어 계층 (Study 7 · 고급 2)

study6에서 jsonwebtoken이 패닉했을 때 클라이언트가 본 것은 HTTP 500이 아니라 **"connection was closed unexpectedly"** 였습니다. 그리고 서버에 무슨 요청이 오는지 로그도 없었습니다.

이런 **"모든 요청에 공통으로 적용되는 관심사"** 를 처리하는 곳이 미들웨어입니다. Axum에는 미들웨어 기능이 거의 없고, 전부 **tower / tower-http** 것을 가져다 씁니다.

**이 문서에서 붙이는 것**: 요청 ID · 요청 로깅 · 응답 시간 헤더 · 패닉 처리 · 타임아웃 · gzip 압축 · CORS · 본문 크기 제한

```powershell
cargo add tower --features util,timeout
cargo add tower-http --features trace,cors,compression-gzip,timeout,limit,request-id,catch-panic,normalize-path
cargo add tracing
cargo add tracing-subscriber --features env-filter
cargo add tokio --features time
```

---

## 1. tower의 `Service`와 `Layer`

### `Service` — "요청을 받아 비동기로 응답을 주는 것"

```rust
// 개념적으로 이런 모양이다 (실제 시그니처는 조금 더 복잡)
trait Service<Request> {
    type Response;
    type Error;
    async fn call(&mut self, req: Request) -> Result<Self::Response, Self::Error>;
}
```

axum에서 **핸들러도, `Router`도, 미들웨어가 감싼 결과도 전부 `Service`** 입니다. 타입이 하나로 통일되어 있어서 조합이 자유롭습니다.

### `Layer` — "`Service`를 감싸 새 `Service`를 만드는 것"

```
       요청 →  [ Layer A ]
                   ↓
               [ Layer B ]
                   ↓
               [ Router ]
                   ↓
               [ handler ]
                   ↑
       응답 ←  (거꾸로 올라오며 각 Layer가 응답을 손볼 기회를 얻는다)
```

데코레이터 패턴입니다. 그래서 **요청 방향과 응답 방향 양쪽에 개입**할 수 있습니다.

| | Spring Boot | NestJS | Express | Axum |
|---|---|---|---|---|
| 개념 | Filter / Interceptor / `HandlerInterceptor` | Middleware / Guard / Interceptor / Pipe / Filter (5종) | `app.use(fn)` | **`Layer` 하나** |
| 조합 | 설정 클래스 | 데코레이터 + 모듈 등록 | 호출 순서 | `.layer()` 체인 |

Nest가 5종류로 나눠 놓은 것을 Axum은 **`Layer` 하나로 통합**했습니다. 대신 "이건 Guard고 저건 Interceptor다" 같은 의미 구분은 코드를 읽어야 알 수 있습니다.

---

## 2. ⭐ 순서가 전부다 — `ServiceBuilder`

### `.layer()`를 직접 여러 번 부르면 순서가 거꾸로다

```rust
Router::new()
    .layer(A)   // 가장 안쪽
    .layer(B)
    .layer(C)   // 가장 바깥 (먼저 실행)
// 실제 실행 순서: C → B → A → handler
```

**나중에 붙인 것이 바깥**입니다. 직관과 반대라 헷갈립니다.

### `ServiceBuilder`를 쓰면 위에서 아래로 읽힌다

```rust
.layer(
    ServiceBuilder::new()
        .layer(A)   // 가장 바깥 (먼저 실행)
        .layer(B)
        .layer(C),  // 가장 안쪽
)
// 실제 실행 순서: A → B → C → handler
```

**미들웨어가 3개 이상이면 무조건 `ServiceBuilder`를 쓰세요.** 순서를 눈으로 확인할 수 있어야 합니다.

### 최종 계층 구성

```rust
.layer(
    ServiceBuilder::new()
        // 1. 요청 ID 부여 — 가장 바깥
        .layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
        // 2. 요청 ID를 응답 헤더로 되돌려 보낸다
        .layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
        // 3. 요청/응답 로깅
        .layer(TraceLayer::new_for_http())
        // 4. 처리 시간 측정 헤더
        .layer(axum::middleware::from_fn(middleware::response_time))
        // 5. 패닉을 500 JSON으로
        .layer(CatchPanicLayer::custom(middleware::on_panic))
        // 6. 타임아웃
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        // 7. 응답 압축
        .layer(CompressionLayer::new())
        // 8. CORS
        .layer(cors_layer())
        // 9. 요청 본문 크기 제한 — 가장 안쪽
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES)),
)
```

### 🔍 순서를 잘못 잡아서 실제로 겪은 일

처음에는 `PropagateRequestIdLayer`를 **맨 아래(가장 안쪽)** 에 뒀습니다. 결과:

```
1 GET /health -> 200 | x-request-id=c0705f40-...  ✅
2 GET /panic  -> 500 | x-request-id=(없음)         ❌
3 GET /slow   -> 408 | x-request-id=(없음)         ❌
```

**정상 응답에만 요청 ID가 붙고, 패닉·타임아웃 응답에는 안 붙었습니다.**

이유: `CatchPanicLayer`와 `TimeoutLayer`는 **아래 계층의 응답을 버리고 자기가 새 응답을 만듭니다.** Propagate가 그보다 **안쪽**에 있으면, 그 새 응답은 Propagate를 거치지 않습니다.

→ Propagate를 **CatchPanic/Timeout보다 바깥**으로 올리니 전부 해결:

```
1 GET /health -> 200 | x-request-id=c2bbbab3-... | x-response-time-ms=0.14
2 GET /panic  -> 500 | x-request-id=32df944e-... | x-response-time-ms=69.45
3 GET /slow   -> 408 | x-request-id=4212380a-... | x-response-time-ms=5015.95
4 GET /nope   -> 404 | x-request-id=9d124baa-... | x-response-time-ms=0.08
```

**이게 미들웨어 순서의 일반 규칙입니다.**

> **"모든 응답에 반드시 적용돼야 하는 것은 바깥에, 특정 상황에서만 동작하는 것은 안쪽에."**
>
> - 요청 ID, 로깅, 보안 헤더 → 바깥
> - 압축, 인증, 본문 파싱 → 안쪽

### `.layer()` vs `.route_layer()`

```rust
Router::new().route("/posts", get(h)).layer(L)        // 404/405 응답에도 L이 적용됨
Router::new().route("/posts", get(h)).route_layer(L)  // 매칭된 라우트에만 L 적용
```

인증 미들웨어를 `.layer()`로 붙이면 **존재하지 않는 경로에 대해서도 401**이 나갑니다(경로 존재 여부를 숨기고 싶다면 오히려 이게 맞을 수도 있습니다). 로깅처럼 전부에 필요한 건 `.layer()`.

---

## 3. 각 계층 뜯어보기

### 3-1. 요청 ID — 분산 추적의 시작

```rust
.layer(SetRequestIdLayer::new(REQUEST_ID_HEADER, MakeRequestUuid))
.layer(PropagateRequestIdLayer::new(REQUEST_ID_HEADER))
```

- `SetRequestId`: 요청에 `x-request-id`가 없으면 **UUID를 만들어 붙입니다.** 이미 있으면(게이트웨이나 상위 서비스가 붙여줬으면) 그대로 둡니다.
- `PropagateRequestId`: 그 ID를 **응답 헤더로 되돌려 보냅니다.**

왜 중요한가: 사용자가 "오류가 났어요"라고 신고할 때 **`x-request-id`만 알려주면 로그에서 그 요청 하나를 정확히 집어낼 수 있습니다.** 마이크로서비스라면 서비스 간에도 전파합니다.

### 3-2. `TraceLayer` — 요청 로깅

```rust
.layer(TraceLayer::new_for_http())
```

`tracing` 구독자(subscriber)를 초기화해야 실제로 출력됩니다.

```rust
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("demo=debug,tower_http=debug,axum::rejection=trace"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .init();
}
```

실제 출력:

```
2026-09-14T06:39:27.592106Z  INFO demo: database ready database_url=sqlite://data.db?mode=rwc
2026-09-14T06:39:27.594171Z  INFO demo: listening addr=127.0.0.1:3000
2026-09-14T06:39:30.473304Z DEBUG request: tower_http::trace::on_request: started processing request method=GET uri=/health version=HTTP/1.1
2026-09-14T06:39:30.474014Z DEBUG request: tower_http::trace::on_response: finished processing request latency=0 ms status=200 method=GET uri=/health version=HTTP/1.1
2026-09-14T06:39:30.587746Z ERROR request: demo::middleware: 핸들러가 패닉했습니다 panic=의도적으로 터뜨린 패닉 method=GET uri=/panic version=HTTP/1.1
2026-09-14T06:39:30.588313Z ERROR request: tower_http::trace::on_failure: response failed classification=Status code: 500 Internal Server Error latency=98 ms method=GET uri=/panic version=HTTP/1.1
2026-09-14T06:39:35.622074Z DEBUG request: ... finished processing request latency=5016 ms status=408 method=GET uri=/slow
```

**주목할 점 세 가지:**

1. **`request:` 라는 span 이름**이 모든 줄에 붙어 있습니다. `TraceLayer`가 요청마다 span을 열고, 그 안에서 나온 모든 로그가 자동으로 `method=`, `uri=` 같은 **요청 맥락을 물려받습니다.** 내가 `tracing::error!(panic = %detail, ...)`라고만 썼는데 `method=GET uri=/panic`이 함께 찍힌 이유입니다.
2. 로그가 **키=값 구조**입니다(`latency=5016 ms status=408`). 문자열을 정규식으로 파싱할 필요 없이 로그 수집기가 바로 필드로 인식합니다. — `println!`과의 결정적 차이.
3. `on_failure`가 5xx를 **자동으로 ERROR 레벨**로 분류합니다.

`RUST_LOG` 환경변수로 런타임에 레벨을 바꿀 수 있습니다.

```powershell
$env:RUST_LOG = "demo=debug,tower_http=debug"     # 자세히
$env:RUST_LOG = "warn"                            # 조용히
$env:RUST_LOG = "axum::rejection=trace"           # extractor가 왜 거부했는지 추적
```

> `axum::rejection=trace`는 디버깅에 정말 유용합니다. "왜 422가 나오지?" 할 때 거부 이유를 로그로 보여줍니다.

### 3-3. `CatchPanicLayer` — 패닉을 500으로

```rust
.layer(CatchPanicLayer::custom(middleware::on_panic))
```

```rust
pub fn on_panic(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic".to_string()
    };

    tracing::error!(panic = %detail, "핸들러가 패닉했습니다");

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error", "message": "서버 내부 오류가 발생했습니다." })),
    ).into_response()
}
```

**이게 study6에서 겪은 문제의 해결책입니다.**

| | 이전 (study6) | 이후 |
|---|---|---|
| 클라이언트가 보는 것 | `connection was closed unexpectedly` | `500` `{"error":"internal_error",...}` |
| 로그 | stderr에 raw 패닉 | `tracing::error!`로 구조화 |
| 요청 ID | 없음 | 있음 |

`Box<dyn Any>`를 `downcast_ref`로 구체 타입으로 되돌리는 게 낯선데, 패닉 payload는 `panic!("리터럴")`이면 `&str`, `panic!("{}", x)`이면 `String`이라 두 경우를 다 봐야 합니다.

> ⚠️ **패닉을 잡는다고 안전해지는 건 아닙니다.** 패닉은 "여기서 계속 실행하면 안 되는 버그"라는 신호입니다. 이 계층은 **다른 요청까지 끊기는 걸 막는 안전망**일 뿐, 근본 원인은 로그를 보고 고쳐야 합니다.

### 3-4. 커스텀 미들웨어 — `middleware::from_fn`

tower의 `Service`를 직접 구현하는 건 꽤 번거롭습니다. axum이 훨씬 쉬운 길을 줍니다.

```rust
pub async fn response_time(request: Request, next: Next) -> Response {
    let started = Instant::now();

    // next.run(request)가 "아래 계층 전체 + 핸들러"를 실행한다.
    let mut response = next.run(request).await;

    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    if let Ok(value) = HeaderValue::from_str(&format!("{elapsed_ms:.2}")) {
        response.headers_mut().insert(
            HeaderName::from_static("x-response-time-ms"),
            value,
        );
    }

    response
}
```

```rust
.layer(axum::middleware::from_fn(middleware::response_time))
```

- `next.run(request).await`가 Express의 `next()`, Nest 인터셉터의 `handle()`에 해당합니다.
- 이 앞이 **요청 처리**, 뒤가 **응답 처리**입니다.
- 상태가 필요하면 `from_fn_with_state(state, handler)`를 씁니다.
- 요청을 **아예 통과시키지 않고** 바로 응답할 수도 있습니다 — `next.run()`을 호출하지 않고 `Response`를 반환하면 됩니다. (레이트 리밋, 점검 모드 등)

`/slow`의 측정값이 `5015.95ms`인 것을 보세요. **타임아웃(5초)으로 잘린 응답의 시간까지 측정**되었습니다 — `response_time`이 `TimeoutLayer`보다 바깥에 있기 때문입니다.

### 3-5. `TimeoutLayer` — 느린 요청 잘라내기

```rust
.layer(TimeoutLayer::with_status_code(
    StatusCode::REQUEST_TIMEOUT,
    Duration::from_secs(5),
))
```

- `TimeoutLayer::new(...)`는 tower-http 0.7에서 **deprecated**입니다. 반환할 상태 코드를 명시하는 `with_status_code`를 씁니다. (deprecated 경고가 그대로 알려줍니다)
- 실제 결과: `GET /slow` → **408 Request Timeout** (핸들러는 10초 `sleep`)
- 타임아웃이 의미가 있으려면 **핸들러가 `.await`로 양보해야** 합니다. `std::thread::sleep`처럼 스레드를 블로킹하면 타이머가 돌 기회조차 없습니다. — study0에서 말한 그 이야기입니다.

### 3-6. `CompressionLayer` — gzip 압축

```rust
.layer(CompressionLayer::new())
```

`Accept-Encoding` 헤더를 보고 **알아서 판단**합니다. 실측:

```
압축 없이   : 4246 bytes, content-encoding=(없음)
gzip 요청 시:   94 bytes, content-encoding=gzip
```

반복되는 JSON이라 **45배**나 줄었습니다(실제 데이터는 보통 3~10배). CPU를 조금 쓰고 대역폭을 크게 아낍니다.

> `compression-br`(Brotli), `compression-zstd` feature도 있습니다. 클라이언트가 지원하면 더 좋은 압축률을 냅니다.

### 3-7. `CorsLayer` — 브라우저 교차 출처 요청

```rust
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin("http://localhost:5173".parse::<HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .max_age(Duration::from_secs(3600))
}
```

실제 preflight 응답:

```
OPTIONS /posts  (Origin: http://localhost:5173)
→ 200  access-control-allow-origin: http://localhost:5173
       access-control-allow-methods: GET,POST,PATCH,DELETE,OPTIONS
```

**⚠️ 두 가지 함정:**

1. **`CorsLayer::permissive()`를 운영에 쓰지 마세요.** 모든 출처를 허용합니다. 개발 중에만.
2. **CORS는 서버가 강제하는 게 아니라 브라우저가 강제합니다.** 허용되지 않은 출처로 요청해도 **서버는 정상 응답**하고, 브라우저가 응답을 JS에 넘겨주지 않을 뿐입니다. 실제로 `Origin: http://evil.example`로 preflight를 보내도 서버는 `200`을 돌려줍니다. **CORS는 인증/인가가 아닙니다.** curl이나 서버-서버 요청에는 아무 영향이 없습니다.
3. 쿠키 기반 인증이면 `.allow_credentials(true)`가 필요하고, 그럴 때는 **`allow_origin`에 와일드카드를 쓸 수 없습니다**(브라우저가 거부).

### 3-8. `DefaultBodyLimit` — 본문 크기 제한

```rust
.layer(DefaultBodyLimit::max(2 * 1024 * 1024))   // 2MB
```

없으면 누군가 1GB짜리 JSON을 보내 메모리를 고갈시킬 수 있습니다. axum 기본값도 2MB지만, **명시해 두면 나중에 "왜 413이 나오지?" 할 때 찾기 쉽습니다.**

study8에서 파일 업로드를 붙일 때 **이 값만 큰 라우트**를 따로 만들게 됩니다.

---

## 4. 인가(Authorization) 다시 보기

study6에서 소유권 검사를 핸들러 안의 `ensure_owner()`로 했습니다. "미들웨어로 빼면 안 되나?" 싶지만, **대부분 안 됩니다.**

```rust
async fn ensure_owner(state: &AppState, post_id: i64, auth: &AuthUser) -> AppResult<()> {
    let row = repo::find(&state.db, post_id).await?
        .ok_or_else(|| AppError::not_found("post", post_id))?;

    match row.author_id {
        Some(owner_id) if owner_id == auth.id => Ok(()),
        _ => Err(AppError::Forbidden("본인이 작성한 글만 수정/삭제할 수 있습니다".into())),
    }
}
```

| 종류 | 판단에 필요한 것 | 어디서 |
|---|---|---|
| **인증** | 토큰만 | **extractor** (`AuthUser`) |
| **역할 기반 인가** (관리자만) | 토큰의 role 클레임 | **extractor** 또는 `route_layer` |
| **리소스 소유권** (내 글만) | **DB 조회 결과 + 사용자 id** | **핸들러/서비스** ← 미들웨어로 못 뺌 |

미들웨어는 URL과 헤더만 보고 판단합니다. "이 글의 주인이 누구인지"는 **DB를 읽어야** 알 수 있고, 그건 이미 비즈니스 로직입니다. Spring Security의 `@PreAuthorize("#post.author == authentication.name")` 같은 표현식도 결국 내부적으로 같은 일을 합니다.

> 역할 기반이라면 extractor로 뺄 수 있습니다. `struct AdminUser`를 만들고 `FromRequestParts`에서 role 클레임을 확인하면, 핸들러 인자에 `_admin: AdminUser`를 적는 것만으로 관리자 전용이 됩니다.

---

## 5. 실행 및 검증

| # | 요청 | 상태 | `x-request-id` | `x-response-time-ms` |
|---|---|---|---|---|
| 1 | `GET /health` | `200` | `c2bbbab3-c349-...` | `0.14` |
| 2 | `GET /panic` | `500` | `32df944e-1ae5-...` | `69.45` |
| 3 | `GET /slow` | `408` | `4212380a-4779-...` | `5015.95` |
| 4 | `GET /nope` | `404` | `9d124baa-a859-...` | `0.08` |

2번 응답 본문:

```json
{"error":"internal_error","message":"서버 내부 오류가 발생했습니다."}
```

**패닉이 났는데도 연결이 살아 있고, study4에서 정한 오류 형식 그대로입니다.**

압축:

```
압축 없이   : 4246 bytes
gzip 요청 시:   94 bytes  (content-encoding: gzip)
```

> 💡 PowerShell의 `Invoke-WebRequest`는 gzip을 **자동으로 풀어버려서** 압축 여부를 눈으로 확인하기 어렵습니다. 위 수치는 `HttpClientHandler.AutomaticDecompression = None`으로 끄고 측정했습니다.

---

## 6. 처음 만난 오류 / 주의점

### 6-1. 미들웨어 순서 (위 2절 참고)

`PropagateRequestIdLayer`가 `CatchPanicLayer`보다 안쪽에 있으면 **패닉·타임아웃 응답에 요청 ID가 빠집니다.** 증상이 "가끔 헤더가 없다"라서 원인을 찾기 어렵습니다.

### 6-2. `TimeoutLayer::new` deprecated

```
warning: use of deprecated associated function `tower_http::timeout::TimeoutLayer::new`:
         Use `TimeoutLayer::with_status_code` instead
```

**Rust의 deprecated 경고는 대체 API까지 알려줍니다.** 경고를 방치하지 말고 그때그때 고치는 게 좋습니다.

### 6-3. `TraceLayer`를 붙였는데 로그가 안 나온다

`tracing`은 **facade**입니다. 로그를 실제로 출력하는 **subscriber를 초기화하지 않으면 전부 버려집니다.** `tracing_subscriber::registry()...init()`이 필요합니다.

또, 기본 필터가 `INFO`라서 `TraceLayer`의 DEBUG 로그가 안 보일 수 있습니다. `RUST_LOG=tower_http=debug`로 올리세요.

### 6-4. Windows에서 로그의 한글이 깨져 보인다

파일로 리다이렉트한 뒤 PowerShell로 읽으면 깨집니다.

```powershell
Get-Content trace.log                  # ❌ ANSI로 읽어서 깨짐
Get-Content trace.log -Encoding UTF8   # ✅
```

**프로그램이 UTF-8로 쓴 것을 PowerShell 5.1이 시스템 코드페이지로 읽기 때문**입니다. 서버가 잘못 출력한 게 아닙니다.

---

## 7. 이번 study 정리

- 미들웨어는 axum이 아니라 **tower / tower-http**의 것이다. 기능을 찾을 때 `tower-http` 문서를 먼저 뒤진다.
- **순서가 전부다.** `.layer()`는 나중 것이 바깥, `ServiceBuilder`는 위가 바깥. 3개 이상이면 `ServiceBuilder`.
- **"모든 응답에 반드시 적용돼야 하는 것은 바깥"** — 요청 ID·로깅은 CatchPanic/Timeout보다 바깥에.
- `CatchPanicLayer`로 패닉을 500 JSON으로 바꿔 **연결이 끊기는 것을 막는다.**
- 커스텀 미들웨어는 `axum::middleware::from_fn`이 가장 쉽다. `next.run(req).await` 앞뒤가 요청/응답 처리.
- `TraceLayer` + `tracing`은 **span으로 요청 맥락이 모든 로그에 자동 전파**된다. `println!`과 여기서 갈린다.
- **CORS는 브라우저가 강제하는 것**이지 보안 장치가 아니다. `permissive()`는 개발용.
- **리소스 소유권 인가는 미들웨어로 뺄 수 없다.** DB를 읽어야 하므로 핸들러/서비스의 일이다.

다음: [study8.md](study8.md) — 페이지네이션, 파일 업로드, 정적 파일 서빙. (고급 3)
