# Axum 기초 개념 정리 (Study 0)

이 문서는 실습 없이 개념만 정리합니다. `Spring_Boot/study0.md`, `python/FastApi/study0.md`, `nest.js/study0.md`를 이미 봤다면 "라우팅 → 핸들러 → 서비스 → DB"라는 큰 그림은 똑같습니다. 하지만 **Axum에는 DI 컨테이너도, 데코레이터도, 런타임 예외도 없습니다.** 그 세 가지가 없는 자리를 무엇이 대신하는지를 이해하는 것이 이 문서의 목표입니다.

---

## 1. Axum이란?

### Rust 웹 생태계에서의 위치

Rust 서버 프레임워크는 크게 이렇게 나뉩니다.

| 프레임워크 | 특징 |
|---|---|
| **actix-web** | 오래되고 빠름. 자체 액터 런타임에서 출발해 독자적인 API가 많음 |
| **axum** | **tokio 팀 공식.** `tower` 생태계를 그대로 쓰고, 매크로를 거의 안 씀 |
| **Rocket** | 매크로(`#[get("/")]`)로 Spring/FastAPI와 가장 비슷한 개발 경험 |
| **warp** | 필터 조합(`filter.and(filter)`) 스타일. axum의 전신 격 |
| **Poem / Salvo** | 후발주자. OpenAPI 자동화 등에 강점 |

- Axum은 **tokio 팀이 직접 만든** 프레임워크입니다. 비동기 런타임(tokio), HTTP 구현(hyper), 미들웨어 추상화(tower)가 전부 같은 팀 손에서 나왔기 때문에 스택이 매끄럽게 맞물립니다.
- **가장 큰 설계 철학은 "매크로를 쓰지 않는다"** 입니다. Spring의 `@GetMapping`, FastAPI의 `@app.get`, Nest의 `@Get()`처럼 라우팅을 애노테이션으로 선언하지 않고, 그냥 **평범한 Rust 함수**를 만들어 라우터에 값으로 등록합니다.

```rust
// 애노테이션이 없다. 그냥 함수를 등록할 뿐이다.
async fn hello() -> &'static str {
    "Hello, Axum!"
}

let app = Router::new().route("/", get(hello));
```

- 매크로가 없다는 건 **컴파일 에러 메시지를 읽기 쉽고, IDE 자동완성과 "정의로 이동"이 정확하게 동작한다**는 뜻입니다. 대신 라우팅 선언이 핸들러 함수와 떨어진 곳(`Router` 조립 지점)에 모여 있어서, "이 URL이 어느 함수로 가는지" 찾으려면 라우터를 봐야 합니다. 이건 장단이 있습니다.

### 왜 굳이 Rust로 웹 서버를?

- **GC(가비지 컬렉터)가 없습니다.** JVM의 GC 일시 정지(stop-the-world)나 Node의 힙 압박이 없어 **응답 시간 꼬리 지연(p99 latency)이 매우 안정적**입니다.
- **메모리 안전성이 컴파일 타임에 보장됩니다.** 데이터 레이스, null 역참조, use-after-free가 구조적으로 막힙니다. 공유 상태를 잘못 다루면 런타임 버그가 아니라 **컴파일 에러**로 먼저 튀어나옵니다.
- 대가는 **컴파일 시간과 학습 곡선**입니다. 첫 빌드는 몇 분이 걸리고, 소유권 개념을 모르면 "왜 이게 안 되지"에서 시간을 많이 씁니다.

---

## 2. 스택 구조: tokio → hyper → tower → axum

Axum을 이해하려면 **혼자 다 하는 프레임워크가 아니라 네 개 층의 맨 위에 얹힌 얇은 레이어**라는 점을 알아야 합니다.

```
┌─────────────────────────────────────────┐
│  내 코드 (핸들러 함수, 라우터 조립)          │
├─────────────────────────────────────────┤
│  axum      라우팅 / Extractor / Response  │  ← 얇음
├─────────────────────────────────────────┤
│  tower     Service·Layer (미들웨어 추상화)  │
├─────────────────────────────────────────┤
│  hyper     HTTP/1.1·HTTP/2 프로토콜 구현    │
├─────────────────────────────────────────┤
│  tokio     비동기 런타임 (이벤트 루프, I/O)  │
└─────────────────────────────────────────┘
```

| 층 | 하는 일 | 다른 생태계에서의 대응 |
|---|---|---|
| **tokio** | async 태스크 스케줄링, 네트워크 I/O | Node의 libuv 이벤트 루프, Python의 asyncio |
| **hyper** | 바이트 스트림 ↔ `Request`/`Response` 변환 | Node의 `http` 모듈, Java의 서블릿 컨테이너(Tomcat) |
| **tower** | `Service` trait 하나로 미들웨어를 조합 | Express의 미들웨어 체인, Spring의 필터/인터셉터 |
| **axum** | URL → 함수 매칭, 요청 파싱, 응답 변환 | Express Router, Spring MVC의 DispatcherServlet |

> 실무적으로 중요한 함의: **로깅, CORS, 압축, 타임아웃, 인증 같은 "가로지르는 관심사"는 axum이 아니라 tower/tower-http 쪽 기능**입니다. 그래서 axum 문서에서 못 찾겠으면 `tower-http` 문서를 봐야 합니다. (study7에서 다룹니다.)

---

## 3. async/await와 tokio 런타임

### 왜 async인가

- 웹 서버는 대부분의 시간을 "DB 응답 기다리기", "네트워크 읽기"로 **놀면서** 보냅니다.
- 요청당 OS 스레드를 하나씩 쓰면(전통적 Spring MVC 방식) 스레드 수만큼만 동시 처리가 가능하고, 스레드는 비쌉니다.
- async는 **기다리는 동안 그 스레드를 다른 요청에 양보**합니다. 스레드 몇 개로 수만 개의 동시 연결을 처리할 수 있습니다.

### Rust의 async는 "런타임이 따로" 있다

```rust
#[tokio::main]           // 이 매크로가 tokio 런타임을 켜고 main을 그 위에서 실행한다
async fn main() {
    // ...
}
```

- Python의 `asyncio`나 JS의 이벤트 루프는 **언어/표준 라이브러리에 내장**되어 있습니다.
- Rust는 `async`/`await` **문법만 언어에 있고, 실제로 돌리는 런타임은 외부 크레이트**입니다. 그래서 `tokio`를 의존성으로 직접 추가해야 합니다.
- `async fn`을 호출하면 **아무 일도 일어나지 않습니다.** `Future`라는 "아직 실행 안 된 작업 설명서"를 반환할 뿐이고, `.await`하거나 런타임에 넘겨야 비로소 실행됩니다. (JS의 Promise는 만드는 즉시 실행되므로 이 부분이 다릅니다.)

### 절대 하면 안 되는 것: async 안에서 블로킹

```rust
async fn bad_handler() {
    std::thread::sleep(Duration::from_secs(5)); // ❌ 워커 스레드 전체가 5초간 멈춘다
}

async fn good_handler() {
    tokio::time::sleep(Duration::from_secs(5)).await; // ✅ 양보한다
}
```

- 무거운 CPU 연산이나 동기 파일 I/O가 필요하면 `tokio::task::spawn_blocking`으로 별도 스레드 풀에 보냅니다. (study6의 비밀번호 해싱에서 실제로 이 상황을 만납니다.)

---

## 4. 핵심 개념 4가지

Axum에서 반드시 이해해야 하는 건 딱 네 개입니다. **Router, Handler, Extractor, IntoResponse.**

### 4-1. Router — URL을 함수에 연결

```rust
use axum::{Router, routing::{get, post}};

let app = Router::new()
    .route("/", get(root))                               // GET /
    .route("/posts", get(list_posts).post(create_post))  // 같은 경로에 메서드별로
    .route("/posts/{id}", get(get_post).delete(delete_post)); // 경로 파라미터
```

- `get(handler)`는 **`MethodRouter`라는 값**을 만듭니다. `.post(...)`로 이어 붙여 한 경로에 여러 메서드를 매핑합니다.
- 경로 파라미터는 **중괄호** `{id}`를 씁니다. (⚠️ axum 0.7까지는 `:id`였습니다. 인터넷 예제 대부분이 아직 `:id`라서 그대로 붙여 넣으면 안 됩니다. study1에서 다시 짚습니다.)
- 라우터끼리 합칠 수 있습니다.
  - `.nest("/api", api_router)` — 접두사를 붙여 하위 라우터로 장착
  - `.merge(other_router)` — 같은 레벨에서 합치기
  - `.fallback(handler)` — 매칭되는 경로가 없을 때 (404 담당)

### 4-2. Handler — 그냥 async 함수

```rust
async fn create_post(
    State(state): State<AppState>,   // 공유 상태
    Json(body): Json<CreatePost>,    // 요청 본문 (반드시 마지막 인자)
) -> impl IntoResponse {
    // ...
}
```

- 핸들러는 **인자가 전부 Extractor이고 반환값이 `IntoResponse`인 async 함수**면 됩니다. 그 외 규칙이 없습니다.
- 이 조건을 만족하면 axum이 `Handler` trait을 자동으로 구현해 줍니다. 조건을 못 맞추면 그 유명한 **"the trait bound ... is not satisfied"** 에러를 봅니다. (study1 정리 참고)

### 4-3. Extractor — 요청에서 필요한 것만 꺼내 오기

**Axum의 진짜 핵심입니다.** "요청 객체(`req`)를 통째로 받아서 안에서 파싱"하는 대신, **함수 시그니처에 필요한 타입을 적으면 axum이 파싱해서 넣어줍니다.**

```rust
async fn handler(
    Path(id): Path<i64>,              // URL 경로에서
    Query(q): Query<Pagination>,      // 쿼리스트링에서
    headers: HeaderMap,               // 헤더에서
    State(db): State<AppState>,       // 앱 상태에서
    Json(body): Json<CreatePost>,     // 본문 JSON에서
) -> impl IntoResponse { /* ... */ }
```

| Extractor | 꺼내는 것 | FastAPI 대응 | Spring 대응 |
|---|---|---|---|
| `Path<T>` | URL 경로 변수 | `id: int` 경로 파라미터 | `@PathVariable` |
| `Query<T>` | 쿼리스트링 | `q: str = None` | `@RequestParam` |
| `Json<T>` | 본문 JSON → 구조체 | Pydantic 모델 인자 | `@RequestBody` |
| `Form<T>` | `application/x-www-form-urlencoded` | `Form(...)` | `@ModelAttribute` |
| `State<T>` | 앱 전역 공유 상태 | `Depends(get_db)` | DI (`@Autowired`) |
| `HeaderMap` | 헤더 전체 | `Header(...)` | `@RequestHeader` |
| `Multipart` | 파일 업로드 | `UploadFile` | `MultipartFile` |

**꼭 기억할 규칙 두 가지:**

1. **본문을 소비하는 extractor(`Json`, `Form`, `String`, `Bytes`, `Multipart`)는 반드시 마지막 인자여야 합니다.** 본문은 스트림이라 한 번만 읽을 수 있기 때문입니다. 타입 수준에서 `FromRequest`(마지막 전용)와 `FromRequestParts`(그 외)로 구분되어 있어서, 순서를 틀리면 컴파일 에러가 납니다.
2. **Extractor는 직접 만들 수 있습니다.** `FromRequestParts`를 구현하면 "Authorization 헤더에서 JWT를 꺼내 검증한 뒤 `AuthUser`를 주입"하는 식의 재사용 가능한 조각이 됩니다. **이게 Nest의 Guard, Spring Security의 필터를 대체하는 자리**입니다. (study6에서 직접 만듭니다.)

### 4-4. IntoResponse — 무엇이든 HTTP 응답으로

```rust
async fn a() -> &'static str { "text" }        // 200 text/plain
async fn b() -> Json<Post> { Json(post) }      // 200 application/json
async fn c() -> (StatusCode, Json<Post>) {     // 201 Created
    (StatusCode::CREATED, Json(post))
}
async fn d() -> Result<Json<Post>, AppError> { /* 성공/실패 분기 */ }
async fn e() -> impl IntoResponse { /* 반환 타입이 복잡할 때 */ }
```

- `IntoResponse`를 구현한 타입이면 뭐든 반환할 수 있습니다. 튜플로 `(상태코드, 헤더, 본문)`을 조합하는 패턴이 관용구입니다.
- **`Result<T, E>`도 `IntoResponse`입니다** (T, E 둘 다 `IntoResponse`라면). 그래서 `?` 연산자를 핸들러 안에서 그대로 쓸 수 있고, 이게 오류 응답 통일의 기반이 됩니다. (study4)

---

## 5. DI 컨테이너가 없다 — `State`로 직접 넘긴다

Spring/Nest/FastAPI에서는 "DB 커넥션 풀이 필요하다"고 선언만 하면 컨테이너가 알아서 꽂아줍니다. **Axum에는 그런 컨테이너가 없습니다.** 대신 상태를 하나의 구조체로 묶어 라우터에 붙이고, 핸들러에서 `State`로 꺼냅니다.

```rust
#[derive(Clone)]                 // ← 필수. 요청마다 복제되기 때문
struct AppState {
    db: SqlitePool,              // 내부적으로 Arc라서 복제가 저렴하다
    jwt_secret: Arc<String>,
}

let state = AppState { db, jwt_secret: Arc::new(secret) };
let app = Router::new()
    .route("/posts", get(list_posts))
    .with_state(state);          // ← 라우터에 상태를 주입

async fn list_posts(State(st): State<AppState>) -> impl IntoResponse { /* ... */ }
```

- `AppState`는 `Clone + Send + Sync + 'static`이어야 합니다. 여러 스레드에서 동시에 쓰이기 때문입니다.
- 값이 큰 필드는 `Arc<T>`(원자적 참조 카운트 포인터)로 감싸서 복제 비용을 포인터 복사 수준으로 낮춥니다.
- **가변 상태**(예: in-memory 게시글 목록)는 `Arc<RwLock<Vec<Post>>>`처럼 감쌉니다. Rust는 "여러 스레드에서 동시에 수정 가능한 데이터"를 그냥 허용하지 않기 때문에, 락을 타입에 명시해야만 컴파일됩니다. (study2에서 직접 씁니다.)

| | Spring Boot | NestJS | FastAPI | Axum |
|---|---|---|---|---|
| 의존성 주입 | `@Autowired` 컨테이너 | `@Injectable` + 생성자 | `Depends()` | **없음 — `State<T>`로 직접 전달** |
| 공유 자원 등록 | `@Bean` | `providers: []` | 모듈 전역 변수 / lifespan | `.with_state(state)` |
| 동시성 안전 | 개발자 책임 | 싱글 스레드라 대체로 무관 | 개발자 책임 | **컴파일러가 강제 (`Send`/`Sync`)** |

---

## 6. 미들웨어 = tower의 `Layer`

```rust
let app = Router::new()
    .route("/posts", get(list_posts))
    .layer(TraceLayer::new_for_http())   // 요청 로깅
    .layer(CorsLayer::permissive())      // CORS
    .layer(TimeoutLayer::new(Duration::from_secs(10)));
```

- tower의 `Service` trait은 한마디로 **"요청을 받아 비동기로 응답을 돌려주는 것"** 입니다. 핸들러도, 라우터도, 미들웨어가 감싼 결과물도 전부 `Service`입니다.
- `Layer`는 `Service`를 감싸 새 `Service`를 만드는 **데코레이터**입니다. 그래서 무한히 겹쳐 쌓을 수 있습니다.
- **적용 순서 주의:** `.layer()`는 **나중에 붙인 것이 바깥쪽(먼저 실행)** 입니다. 위 예시에서 실제 요청 처리 순서는 `Timeout → Cors → Trace → 핸들러`입니다. 여러 개를 순서대로 쌓을 땐 `ServiceBuilder`를 쓰면 "위에서 아래로" 읽히도록 뒤집어 줍니다.
- `.route_layer()`는 **매칭된 라우트에만** 적용됩니다. 인증 미들웨어처럼 404에는 적용하고 싶지 않은 경우에 씁니다.

> Nest의 Guard/Interceptor/Filter, Spring의 Filter/Interceptor가 전부 이 하나의 `Layer` 개념으로 통합되어 있습니다.

---

## 7. 오류 처리: 예외가 아니라 값

Rust에는 예외(exception)가 없습니다. 실패할 수 있는 함수는 `Result<T, E>`를 반환합니다.

```rust
enum Result<T, E> { Ok(T), Err(E) }
```

- `?` 연산자는 "`Err`면 즉시 반환, `Ok`면 값을 꺼냄"의 축약입니다. `try/catch` 없이도 오류 전파가 한 글자로 끝납니다.
- 웹에서의 관용 패턴은 **앱 전용 에러 enum을 만들고 거기에 `IntoResponse`를 구현**하는 것입니다.

```rust
enum AppError { NotFound, Validation(String), Db(sqlx::Error) }

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            AppError::Validation(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Db(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal".to_string()),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

// 이제 핸들러가 이렇게 깔끔해진다
async fn get_post(
    Path(id): Path<i64>,
    State(st): State<AppState>,
) -> Result<Json<Post>, AppError> {
    let post = repo::find(&st.db, id).await?;   // DB 에러는 ?로 자동 변환되어 500
    Ok(Json(post))
}
```

- **이게 Nest의 `ExceptionFilter`, Spring의 `@RestControllerAdvice`에 해당하는 자리**입니다. 차이는 "전역에 등록해서 가로챈다"가 아니라 **"반환 타입에 적혀 있어서 컴파일러가 빠뜨림을 잡아준다"** 는 점입니다. (study4에서 완성합니다.)
- **패닉(panic)** 은 예외가 아니라 "복구 불가능한 버그"입니다. `unwrap()`/`expect()`는 실패 시 패닉하므로, **요청 처리 경로에서는 쓰지 않는 것이 원칙**입니다. (서버 시작 시 설정 로딩처럼 "실패하면 어차피 못 뜨는" 곳에서는 씁니다.)

---

## 8. 다른 프레임워크와의 대응표

| 개념 | Spring Boot | NestJS | FastAPI | **Axum** |
|---|---|---|---|---|
| 진입점 | `SpringApplication.run` | `NestFactory.create` | `uvicorn.run(app)` | `axum::serve(listener, app)` |
| 라우팅 선언 | `@GetMapping("/posts")` | `@Get()` | `@app.get("/posts")` | `.route("/posts", get(h))` |
| 컨트롤러 | `@RestController` 클래스 | `@Controller` 클래스 | 라우터 함수 모음 | **평범한 함수 + `Router` 조립** |
| 요청 파싱 | `@RequestBody`, `@PathVariable` | `@Body()`, `@Param()` | Pydantic 모델 인자 | **Extractor (`Json`, `Path`)** |
| 응답 변환 | Jackson 자동 직렬화 | 반환값 자동 직렬화 | 반환값 자동 직렬화 | **`IntoResponse` trait** |
| 검증 | `@Valid` + Bean Validation | class-validator | Pydantic | **`validator` 크레이트 + 커스텀 extractor** |
| DI | `@Autowired` | 생성자 주입 | `Depends()` | **`State<T>` (컨테이너 없음)** |
| 미들웨어 | Filter / Interceptor | Middleware / Interceptor | Middleware / `Depends` | **tower `Layer`** |
| 인증 가드 | Spring Security 필터 | `Guard` | `Depends(get_current_user)` | **`FromRequestParts` 커스텀 extractor** |
| 예외 처리 | `@RestControllerAdvice` | `ExceptionFilter` | `exception_handler` | **`Result<T, AppError>` + `IntoResponse`** |
| ORM | JPA/Hibernate | TypeORM / Prisma | SQLAlchemy | **sqlx(쿼리 우선) / SeaORM / Diesel** |
| 문서 자동화 | springdoc | `@nestjs/swagger` | 내장 | **`utoipa` (수동 어노테이션)** |
| 테스트 | MockMvc | Supertest | TestClient | **`tower::ServiceExt::oneshot`** |

**한 줄 요약:** 다른 세 프레임워크가 *"런타임 리플렉션/데코레이터로 마법을 부린다"* 면, Axum은 *"전부 타입과 trait으로 컴파일 타임에 해결한다"* 입니다. 마법이 없어서 처음엔 손이 더 가지만, 한번 컴파일되면 런타임에 놀랄 일이 적습니다.

---

## 9. 프로젝트 구조와 Cargo

Rust는 정해진 폴더 규칙(Spring의 `src/main/java`, Nest의 `src/모듈명`)이 프레임워크 차원에서 강제되지 않습니다. 다만 Cargo의 관례는 있습니다.

```
demo/
├─ Cargo.toml        ← 의존성 + 메타데이터 (package.json / pom.xml 역할)
├─ Cargo.lock        ← 의존성 버전 잠금 (package-lock.json 역할, 앱이면 커밋)
├─ .env              ← 환경변수
├─ migrations/       ← sqlx 마이그레이션 SQL
└─ src/
   ├─ main.rs        ← 진입점 (크레이트 루트)
   ├─ state.rs       ← AppState 정의
   ├─ error.rs       ← AppError 정의
   └─ posts/
      ├─ mod.rs      ← 모듈 선언
      ├─ handler.rs  ← 컨트롤러 역할
      ├─ model.rs    ← 엔티티/DTO
      └─ repo.rs     ← DB 접근
```

- **크레이트(crate)**: 컴파일 단위. 하나의 실행 파일 또는 라이브러리.
- **모듈(module)**: 크레이트 안의 네임스페이스. `mod posts;`라고 선언해야 `posts.rs` 또는 `posts/mod.rs`가 컴파일에 포함됩니다. **파일을 만들어 두기만 하면 자동 인식되는 Nest/Python과 다릅니다.** 처음에 자주 틀리는 부분입니다.
- **워크스페이스(workspace)**: 여러 크레이트를 묶은 상위 단위. `studyFinal`에서 이 구조를 씁니다.

---

## 10. 선행 Rust 문법 최소 정리

Axum 실습에서 실제로 계속 마주치는 것만 추렸습니다.

| 문법 | 의미 | 언제 만나나 |
|---|---|---|
| `let x = 5;` / `let mut x = 5;` | 기본은 **불변**, 바꾸려면 `mut` | 어디서나 |
| 소유권(ownership) | 값의 주인은 항상 하나. 대입/전달하면 **이동(move)** | 핸들러 간 데이터 전달 |
| `&T` / `&mut T` | 빌림(borrow). 읽기 참조는 여럿, 쓰기 참조는 하나만 | DB 풀 전달(`&pool`) |
| `Option<T>` | 값이 있거나(`Some`) 없거나(`None`). **null 대체** | "게시글 없음" 표현 |
| `Result<T, E>` | 성공(`Ok`) 또는 실패(`Err`). **예외 대체** | 모든 I/O |
| `?` | `Err`면 조기 반환 | 핸들러/리포지토리 전반 |
| `match` | 패턴 매칭. 모든 경우를 다뤘는지 컴파일러가 검사 | `AppError` 분기 |
| `impl Trait for Type` | 타입에 trait 구현 | `IntoResponse for AppError` |
| `#[derive(...)]` | trait 자동 구현 매크로 | `Serialize`, `Deserialize`, `Clone`, `Debug` |
| `Arc<T>` | 원자적 참조 카운트. **여러 스레드가 공유** | `AppState` 필드 |
| `Mutex<T>` / `RwLock<T>` | 배타 잠금 / 읽기-쓰기 잠금 | in-memory 저장소 |
| `async fn` / `.await` | 비동기 함수 / 대기 | 모든 핸들러 |
| `Send` / `Sync` | 스레드 간 이동/공유 가능 표식 | 상태에 이상한 타입을 넣으면 에러로 등장 |
| `'static` | "프로그램 전체 수명만큼 살 수 있다" | 핸들러/상태 제약 조건 |

> 위 표에서 `Send`, `Sync`, `'static`은 **직접 쓸 일은 거의 없는데 에러 메시지에는 자주 나옵니다.** "핸들러가 `Handler` trait을 만족하지 않는다"는 에러의 90%는 이 셋 중 하나가 원인입니다. (`Rc` 대신 `Arc`를 써야 하는 이유, `std::sync::Mutex`를 `.await` 너머로 들고 가면 안 되는 이유가 전부 여기서 나옵니다.)

---

## 11. 용어 사전

| 용어 | 뜻 |
|---|---|
| **크레이트(crate)** | Rust의 패키지/컴파일 단위. npm 패키지에 해당 |
| **Cargo** | Rust의 빌드 도구 겸 패키지 매니저. npm + webpack + jest 역할을 한 번에 |
| **crates.io** | Rust 공식 패키지 저장소 (npm registry, PyPI, Maven Central에 해당) |
| **tokio** | 비동기 런타임. async 코드를 실제로 실행하는 엔진 |
| **hyper** | HTTP 프로토콜 구현체 |
| **tower** | `Service`/`Layer` 기반 미들웨어 추상화 |
| **tower-http** | tower 위에 만들어진 HTTP 전용 미들웨어 모음 (CORS, 압축, 로깅, 정적 파일…) |
| **serde** | 직렬화/역직렬화 프레임워크. `Serialize`/`Deserialize` derive로 씀 |
| **Extractor** | 요청에서 원하는 데이터를 꺼내 핸들러 인자로 넣어주는 타입 |
| **IntoResponse** | HTTP 응답으로 변환 가능함을 나타내는 trait |
| **Handler** | extractor를 인자로, `IntoResponse`를 반환값으로 갖는 async 함수 |
| **Layer** | `Service`를 감싸 기능을 추가하는 미들웨어 팩토리 |
| **sqlx** | 비동기 SQL 툴킷. ORM이 아니라 **쿼리를 직접 쓰는** 방식 |
| **SeaORM** | sqlx 위에 얹힌 본격 ORM (엔티티/관계/쿼리 빌더 제공) |
| **thiserror / anyhow** | 에러 타입 정의(라이브러리용) / 에러 뭉뚱그리기(앱용) 크레이트 |
| **tracing** | 구조적 로깅·추적 프레임워크 (`log`의 async 친화적 후속) |

---

## 12. 앞으로의 학습 로드맵

| 단계 | 문서 | 목표 |
|---|---|---|
| 기본기 | study1 | 설치, cargo 명령어, 라우터와 기본 extractor |
| | study2 | `Arc<RwLock>` 공유 상태로 게시판 CRUD |
| | study3 | serde 세부 제어 + `validator`로 입력 검증 |
| | study4 | `AppError`로 오류 응답 통일 |
| | study5 | sqlx + SQLite로 실제 영속화 |
| 고급 | study6 | 회원 가입/로그인, Argon2 해시, JWT extractor |
| | study7 | tower 미들웨어 계층, 인가(소유권 검사) |
| | study8 | 페이지네이션, 파일 업로드, 정적 파일 서빙 |
| | study9 | 통합 테스트, 설정 관리, tracing 로깅 |
| | study10 | OpenAPI 문서화, Docker 멀티 스테이지 배포 |
| 종합 | studyFinal | 구조를 바꿔서(워크스페이스 + SeaORM) 웹사이트 완성 |

---

## 13. 이 문서에서 꼭 가져갈 것 5가지

1. **Axum은 얇다.** 진짜 일은 tokio/hyper/tower가 한다. 기능을 못 찾으면 `tower-http`를 먼저 뒤져라.
2. **매크로도 DI도 없다.** 라우팅은 값으로 조립하고, 의존성은 `State<T>`로 직접 넘긴다.
3. **Extractor가 전부다.** 요청에서 뭘 꺼내든, 심지어 인증까지도 extractor로 표현한다.
4. **오류는 예외가 아니라 값이다.** `Result<T, AppError>` + `IntoResponse`가 전역 예외 핸들러를 대신한다.
5. **컴파일러가 동시성 리뷰어다.** `Send`/`Sync`/`Clone` 에러는 잔소리가 아니라 "지금 이 코드 데이터 레이스 납니다"라는 경고다.

다음: [study1.md](study1.md) — Rust 설치부터 첫 Axum 서버 띄우기.
