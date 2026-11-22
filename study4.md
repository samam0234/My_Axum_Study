# study4 — 라우팅과 요청 처리

> 🎯 **목표**: URL이 핸들러까지 가는 길(정적/동적/와일드카드 경로, 쿼리, 리다이렉트)을 이해하고, 요청이 들어와 응답이 나가는 **전체 흐름**과 404/405 오류 응답을 직접 제어한다. 프론트와 백엔드의 경로·출처(CORS)를 맞추는 방법까지 익힌다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s04_routing.rs](lab/examples/s04_routing.rs) (포트 3004), 실제 적용은 [demo/src/lib.rs](demo/src/lib.rs) |

---

## ① 목표

- 경로 종류(`/a`, `/a/{id}`, `/files/{*path}`)와 `nest`를 구분해 쓴다.
- `Path`(어디), `Query`(어떻게)의 역할을 나눈다.
- 요청 → 미들웨어 → 핸들러 → 응답의 흐름을 로그로 눈으로 확인한다.
- 404(없는 경로)와 405(없는 메서드)를 **JSON으로** 돌려준다.
- 프론트(다른 포트)가 호출할 수 있게 CORS를 설정한다.

---

## ② 📖 이론

### 요청이 핸들러에 닿기까지

```
브라우저/curl
   │  GET /api/users/42?page=2
   ▼
[미들웨어 계층]  ← 로깅·CORS·압축… (바깥 → 안쪽 순으로 통과)
   ▼
[Router]  경로 + 메서드 매칭
   ├─ 경로 매칭 O, 메서드 매칭 O → 핸들러 실행 (extractor가 먼저 값을 꺼냄)
   ├─ 경로 매칭 O, 메서드 X      → 405 Method Not Allowed
   └─ 경로 매칭 X                → fallback (404)
   ▼
[핸들러] → IntoResponse → 응답
   ▲
[미들웨어 계층]  ← 응답이 나갈 때는 안쪽 → 바깥 순서로 다시 통과
```

### 경로와 쿼리의 역할

| | 예 | 역할 | extractor |
|---|---|---|---|
| **경로** | `/users/42` | **무엇**을 가리키는가 (자원 식별) | `Path<T>` |
| **쿼리** | `?page=2&q=mi` | 그 자원을 **어떻게** 보여 줄까 (필터·정렬·페이지) | `Query<T>` |

REST 이름 규칙(공통 API 스펙 v1): 복수 명사·소문자·하이픈, **동사 금지**(`/api/getPosts` ✗), 수정은 `PATCH`.

### 경로 문법 (axum 0.8)

| 패턴 | 매칭 예 | 비고 |
|---|---|---|
| `/users` | `/users` | 정적 |
| `/users/{id}` | `/users/42` | **한 구간**을 변수로. (0.7까지는 `:id`) |
| `/files/{*path}` | `/files/a/b/c.txt` → `"a/b/c.txt"` | **나머지 전부**를 한 문자열로 |
| `.nest("/api", router)` | `/api/...` | 접두어 붙이기 |

### 상태 코드 — 라우팅 단계에서 나오는 것

| 코드 | 의미 | 누가 만드나 |
|---|---|---|
| 301/308 | 영구 이동 | `Redirect::permanent` (**308**) |
| 302/303/307 | 임시 이동 | `Redirect::to`(303), `temporary`(307) |
| 400 | 경로·쿼리 파싱 실패 | extractor |
| 404 | 없는 경로 | `fallback` |
| 405 | 경로는 있는데 메서드가 다름 | `method_not_allowed_fallback` |

---

## ③ 🛠️ 실기

### 3-1. 전체 코드 — [lab/examples/s04_routing.rs](lab/examples/s04_routing.rs)

```rust
use std::time::Instant;
use axum::{
    Json, Router,
    extract::{Path, Query, Request},
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin("http://localhost:5173".parse::<header::HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST]);

    let api = Router::new()
        .route("/users", get(list_users))
        .route("/users/{id}", get(get_user))
        .route("/files/{*path}", get(get_file));

    let app = Router::new()
        .route("/", get(|| async { "라우팅 예제" }))
        .route("/old-users", get(|| async { Redirect::permanent("/api/users") }))
        .nest("/api", api)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(cors)
        .layer(middleware::from_fn(log_requests));
    // ... bind & serve
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default = "one")] page: u32,
    #[serde(default = "ten")] limit: u32,
    q: Option<String>,
}
fn one() -> u32 { 1 }
fn ten() -> u32 { 10 }

async fn list_users(Query(q): Query<ListQuery>) -> Json<serde_json::Value> {
    Json(json!({ "page": q.page, "limit": q.limit.clamp(1, 50), "q": q.q }))
}

async fn get_user(Path(id): Path<u32>) -> Result<Json<serde_json::Value>, StatusCode> {
    if id == 42 { Ok(Json(json!({ "id": 42, "name": "mina" }))) } else { Err(StatusCode::NOT_FOUND) }
}

async fn get_file(Path(path): Path<String>) -> String {
    format!("요청한 경로: {path}")
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND,
     Json(json!({ "error": { "code": "NOT_FOUND", "message": "없는 경로입니다." } }))).into_response()
}

async fn method_not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED,
     Json(json!({ "error": { "code": "METHOD_NOT_ALLOWED", "message": "지원하지 않는 메서드입니다." } }))).into_response()
}

async fn log_requests(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let started = Instant::now();

    let response = next.run(request).await;

    println!("{method} {uri} → {} ({:.1}ms)", response.status().as_u16(),
             started.elapsed().as_secs_f64() * 1000.0);
    response
}
```

### 3-2. 줄별 설명

| 코드 | 설명 |
|---|---|
| `.route("/files/{*path}", ...)` | `*`가 붙은 변수는 슬래시를 포함한 나머지를 모두 받는다 |
| `Redirect::permanent("/api/users")` | **308 Permanent Redirect**. 브라우저·검색엔진이 "옮겨졌다"고 기억한다. 임시라면 `Redirect::temporary`(307) |
| `.nest("/api", api)` | `api`의 모든 경로 앞에 `/api` |
| `.fallback(not_found)` | 어떤 라우트에도 안 걸린 요청. 핸들러가 `Response`를 돌려주므로 **상태 코드와 본문을 직접 정한다** |
| `.method_not_allowed_fallback(...)` | 경로는 있는데 메서드가 없을 때만. 기본 응답은 본문이 빈 405 |
| `#[serde(default = "one")]` | 쿼리에 `page`가 없으면 `one()`의 값(1). 이게 없으면 `page`를 빠뜨린 요청이 400 |
| `q.limit.clamp(1, 50)` | **범위 밖 값은 서버가 잘라 낸다.** `limit=999`가 DB를 괴롭히지 못하게 |
| `.layer(cors).layer(middleware::from_fn(log_requests))` | `.layer()`를 여러 번 부르면 **나중에 붙인 것이 바깥**이다 (아래 순서 주의) |
| `next.run(request).await` | 이 줄 **앞**은 "요청이 들어올 때", **뒤**는 "응답이 나갈 때" |
| `request.method().clone()` | `next.run(request)`가 요청을 **소비(move)** 하므로 필요한 값은 미리 복사해 둔다 |
| `CorsLayer::new().allow_origin(...)` | 지정한 출처에서 온 브라우저 요청을 허용한다 |

### 3-3. 실행 결과

```
$ curl "http://127.0.0.1:3004/api/users?page=2&limit=999&q=mi"
{"limit":50,"page":2,"q":"mi"}                      ← limit이 50으로 잘렸다

$ curl http://127.0.0.1:3004/api/users/42
{"id":42,"name":"mina"}

$ curl -o /dev/null -w "%{http_code}" http://127.0.0.1:3004/api/users/abc
400                                                 ← Path<u32> 파싱 실패

$ curl http://127.0.0.1:3004/api/files/a/b/c.txt
요청한 경로: a/b/c.txt

$ curl -i http://127.0.0.1:3004/old-users
HTTP/1.1 308 Permanent Redirect
location: /api/users

$ curl http://127.0.0.1:3004/nope
{"error":{"code":"NOT_FOUND","message":"없는 경로입니다."}}

$ curl -X POST http://127.0.0.1:3004/
{"error":{"code":"METHOD_NOT_ALLOWED","message":"지원하지 않는 메서드입니다."}}

$ curl -i -H 'Origin: http://localhost:5173' http://127.0.0.1:3004/api/users | grep -i access-control
access-control-allow-origin: http://localhost:5173
```

서버 콘솔(`log_requests`가 찍은 흐름):

```
GET / → 200 (0.1ms)
GET /api/users?page=2&limit=999&q=mi → 200 (0.1ms)
GET /api/users/abc → 400 (0.0ms)
GET /old-users → 308 (0.0ms)
GET /nope → 404 (0.0ms)
```

---

## ④ 🔗 연동 포인트

### 쿼리 파라미터와 페이지 이동

- 목록 화면의 상태(검색어, 페이지)는 **URL 쿼리에 둔다.** 새로고침·뒤로가기·링크 공유가 공짜로 된다.
- 프론트: `/posts?page=2&q=axum` ↔ 백엔드: `GET /api/posts?page=2&q=axum` — **같은 이름**을 쓰면 프론트가 URL의 쿼리를 그대로 API에 넘길 수 있다.
- 페이지 이동은 프론트 라우터가, 데이터 가져오기는 `fetch`가 맡는다. 서버는 `/api` 아래만 신경 쓴다.

### 프론트와 백엔드 경로 맞추기

| 방식 | 구성 | 장단점 |
|---|---|---|
| **같은 출처로 서빙** | Axum이 `/api`와 정적 파일(`/`)을 함께 서빙 | CORS 불필요, 쿠키 단순. 배포도 하나 (**studyFinal이 이 방식**) |
| **개발 서버 프록시** | Vite `server.proxy: { "/api": "http://127.0.0.1:3000" }` | 브라우저는 같은 출처로 보인다. CORS 설정 불필요 |
| **CORS 허용** | 프론트 `:5173` → API `:3000` 직접 호출 + `CorsLayer` | 설정 필요, 쿠키를 쓰면 `credentials` + 정확한 Origin (study12) |

경로 합의는 문서 한 장으로 한다 — **공통 API 스펙 v1**(Base URL `/api`, `/api/posts`, `/api/posts/{id}`, `/api/auth/login` …). 이 저장소의 `demo/`와 `project/`는 이 경로를 그대로 쓴다 (study6 ④의 대조표). 이 단계의 `lab/` 예제(`s04_routing`)는 라우팅 연습용이라 `/api` 접두어 없이 단순한 경로를 쓴다.

### CORS는 서버가 아니라 **브라우저**가 막는다

`curl`로는 항상 되는 요청이 브라우저에서만 실패하면 CORS다. 서버는 `access-control-allow-origin` 헤더로 "이 출처는 허용"이라고 **알려 줄 뿐**이고, 헤더가 요청 출처와 맞지 않으면 **브라우저가 응답을 JS에 넘기지 않는다.** (서버 로그에는 정상 200이 찍힌다)

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 서버 시작 시 패닉 `Path segments must not start with ':'` | 0.8에서 `:id` → `{id}` | `{id}` 사용 |
| `/api/users/abc` → 400, 본문이 **평문** | `Path<u32>` 파싱 실패의 기본 응답 | study5/6에서 `Path`를 감싸 JSON 오류로 통일 (demo의 `extract::Path`) |
| `limit` 없이 호출했더니 400 | `#[serde(default)]`를 안 붙임 | 선택 파라미터는 `Option<T>` 또는 `#[serde(default = "...")]` |
| `/users/`(끝 슬래시)가 404 | axum은 `/users`와 `/users/`를 **다르게** 본다 | 한 가지로 통일해 두거나 `tower-http`의 `NormalizePathLayer` (demo의 `normalize-path` feature) |
| 브라우저 콘솔 `blocked by CORS policy` 인데 curl은 성공 | 허용 출처 불일치 (포트·스킴·`localhost` vs `127.0.0.1` 포함) | `allow_origin`을 **브라우저 주소창의 출처와 글자 그대로** 맞춘다 |
| CORS 설정을 했는데 **POST만** 실패 | `Content-Type: application/json`은 "단순 요청"이 아니라 **사전 요청(OPTIONS preflight)** 을 먼저 보냄 | `allow_methods`/`allow_headers`에 필요한 것을 모두 포함. `OPTIONS`는 `CorsLayer`가 자동 응답 |
| 존재하는 경로인데 404 | `nest` 안에서 `"/"`를 빼먹음 / 접두어를 두 번 붙임 (`nest("/api")` 안에 `/api/users`) | 최종 경로를 손으로 이어 붙여 확인 |
| 404 JSON 대신 빈 본문 404 | `fallback`을 등록하지 않았거나, **정적 파일 서비스**(`ServeDir`)가 404를 만듦 | 미들웨어에서 4xx를 감싸 통일 (demo의 `error_envelope`, study14) |
| `.layer()`를 붙였는데 일부 경로에 적용 안 됨 | `.layer()`는 **그 시점까지 등록된 라우트**에만 적용된다 | 공통 계층은 **라우트 등록을 모두 끝낸 뒤** 마지막에 붙인다 |
| `GET /old-users`가 301이 아니라 308 | `Redirect::permanent`는 308 (메서드를 유지하는 영구 이동) | 정상. 301을 원하면 직접 상태 코드와 `Location`을 만든다 |

---

## ⑥ 직접 해보기

1. **끝 슬래시 실험**: `/api/users/`를 호출해 404가 나는지 확인하고, 두 형태를 모두 받도록 만든다. (`NormalizePathLayer`, 또는 두 경로 등록)
2. **`/api/users/{id}/posts/{post_id}`**: `Path<(u32, u32)>`로 두 파라미터를 받는다. 구조체(`#[derive(Deserialize)] struct Ids { id: u32, post_id: u32 }`)로 받는 방식과 비교해 본다.
3. **CORS 직접 깨 보기**: [study12](study12.md)의 `s12_front` 또는 간단한 HTML을 다른 포트로 띄워 `fetch`하고, 개발자도구 Network 탭에서 `OPTIONS` 사전 요청과 응답 헤더를 확인한다. `allow_origin`을 일부러 틀리게 바꿔 콘솔 오류 문구를 읽는다.

---

## ⑦ ☐ 체크리스트

- ☐ `/x`, `/x/{id}`, `/x/{*rest}`, `nest`의 차이를 설명할 수 있다
- ☐ `Path`와 `Query`의 역할 차이를 설명할 수 있다
- ☐ 404와 405가 어디서 나오는지, 각각을 JSON으로 바꾸는 방법을 안다
- ☐ 미들웨어에서 `next.run(request)` 앞뒤가 각각 언제 실행되는지 설명할 수 있다
- ☐ `.layer()`는 **나중에 붙인 것이 바깥**이고, 그 시점까지 등록된 라우트에만 적용된다는 것을 안다
- ☐ `Redirect::permanent`(308)와 `temporary`(307)의 차이를 안다
- ☐ CORS가 서버가 아니라 브라우저의 검사이고, preflight가 무엇인지 설명할 수 있다
- ☐ 프론트와 백엔드가 같은 쿼리 이름을 쓰면 왜 편한지 설명할 수 있다
- ☐ 프론트와 백엔드 경로를 맞추는 세 방법(같은 출처 / 프록시 / CORS)의 차이를 안다

---

## ⑧ 다음 단계 연결

경로를 통해 요청이 들어오는 길을 만들었으니, [study5](study5.md)에서는 그 요청의 **본문(사용자 입력)** 을 받아 검증하고, 틀렸을 때 필드별 오류 메시지를 돌려주고, 중복 제출을 막는 폼 흐름을 만든다.
