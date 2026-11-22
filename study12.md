# study12 — 프론트 · UI 연동

> 🎯 **목표**: Axum API를 프론트엔드(UI 라이브러리 · 상태 관리 · 폼 라이브러리)가 **어떻게 소비하는지** 맞물리는 지점을 만든다: 정적 파일 서빙과 SPA fallback, 개발 서버 연동(프록시 / CORS), 쿠키 인증, 그리고 프론트의 **API 클라이언트 한 곳**에서 공통 API 스펙 v1(`{ data, meta }` / `{ error }`)을 해석하는 방법.

| 항목 | 내용 |
|---|---|
| 관련 코드 | 서버: [lab/examples/s12_front.rs](lab/examples/s12_front.rs) (포트 3012) · 프론트: [lab/static/index.html](lab/static/index.html), [lab/static/api.js](lab/static/api.js), [lab/static/api.test.mjs](lab/static/api.test.mjs) |
| 검증 | `node --test` **9개 통과**, 실제 서버 두 개(s12, s14)에 `api.js`를 붙여 확인, `curl`로 CORS preflight·쿠키 확인 |
| 범위 | 이 저장소는 백엔드 학습이다. React/Vue 등 **프레임워크별 프로젝트는 만들지 않고**, 어떤 프론트가 와도 같은 `api.js` 계약을 쓰도록 **서버 쪽 준비와 계약**을 다룬다. 프레임워크 예시는 문서의 스케치이며 이 저장소에서 실행하지 않았다 |

---

## ① 목표

1. 프론트와 백엔드를 **같은 출처로 서빙 / 개발 프록시 / CORS** 세 방식으로 연결할 수 있다.
2. 쿠키 인증이 CORS에서 요구하는 조건(`credentials: "include"`, 정확한 Origin, `SameSite`)을 안다.
3. 프론트의 **API 클라이언트 한 곳**에서 성공/실패 계약을 해석해, 화면 코드가 상태 코드와 JSON 모양을 몰라도 되게 한다.
4. 서버의 `error.fields`를 폼 라이브러리의 필드 오류로 꽂는 방법을 안다.
5. UI 라이브러리·상태 관리·폼 라이브러리가 각각 **어느 자리**를 맡는지 구분한다.

---

## ② 📖 이론

### 프론트 ↔ 백엔드의 접점은 세 군데

```
[프론트: UI 컴포넌트 · 상태 관리 · 폼]
        │ ① 어디서 받아 오나   (정적 파일 서빙 / 개발 서버)
        │ ② 어떻게 부르나     (경로, CORS, 쿠키/토큰)          ← 서버가 준비한다
        │ ③ 무엇을 주고받나   (JSON 계약: data/meta/error)      ← 한 곳(api.js)에서 해석한다
        ▼
[Axum API]
```

### 연결 방식 세 가지

| 방식 | 구성 | 장점 | 주의 |
|---|---|---|---|
| **A. 같은 출처로 서빙** | Axum이 `/api`와 빌드된 정적 파일을 함께 서빙 | CORS 불필요, 쿠키 단순, 배포 하나 | SPA 라우팅을 위해 fallback 필요 |
| **B. 개발 서버 프록시** | Vite `server.proxy`로 `/api`를 Axum에 전달 | 브라우저는 같은 출처로 본다. CORS 설정 불필요 | 운영에서는 A 또는 리버스 프록시로 |
| **C. CORS 허용** | 프론트 `:5173` → API `:3012` 직접 호출 | 서버와 프론트가 완전히 분리 | `CorsLayer` + (쿠키면) `credentials` + **정확한 Origin** |

```ts
// B. vite.config.ts  (프론트 쪽 설정 — 이 저장소에서 실행하지 않음)
export default defineConfig({
  server: { proxy: { "/api": "http://127.0.0.1:3012" } },
});
```

### 프론트의 세 부품이 맡는 자리

| 부품 | 맡는 일 | 서버와의 접점 |
|---|---|---|
| **UI 라이브러리** (컴포넌트 모음) | 버튼·표·모달 등 화면 조각 | 없음 (서버 몰라도 된다) |
| **상태 관리** | ① **서버 상태**(목록·상세 — 가져오고 캐시하고 갱신) ② **클라이언트 상태**(로그인한 사용자, 모달 열림) | 서버 상태는 **캐시 키 = URL(+쿼리)**, 쓰기 성공 후 **무효화**. 이 규칙이 API의 경로·쿼리 설계에 맞물린다 |
| **폼 라이브러리** | 입력값·검증·제출 상태 | 서버가 준 `error.fields`를 **필드 오류로 꽂는다** |

### 계약을 한 곳에서 해석하는 이유

화면 컴포넌트가 곳곳에서 `fetch(...)` → `res.status` → `res.json()` → `data.error.message`를 직접 다루면, 오류 모양이 바뀔 때 **수십 곳**을 고쳐야 한다. `api()` 한 함수가 `{ data, meta }`와 `ApiError`로 정규화하면 화면은 둘만 안다.

---

## ③ 🛠️ 실기

### 3-1. 서버 — [lab/examples/s12_front.rs](lab/examples/s12_front.rs)

```rust
let origin = std::env::var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".into());

let cors = CorsLayer::new()
    // ⚠️ 쿠키를 주고받는 CORS에서는 `Any`(*)를 쓸 수 없다. 출처를 정확히 적어야 한다.
    .allow_origin(origin.parse::<HeaderValue>().expect("CORS_ORIGIN 형식 오류"))
    .allow_credentials(true)
    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
    .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

let api = Router::new()
    .route("/hello", get(|| async { Json(json!({ "message": "안녕하세요" })) }))
    .route("/login", post(login))
    .route("/me", get(me))
    .route("/logout", post(logout))
    // `/api` 아래의 없는 경로는 SPA가 아니라 JSON 404여야 한다 (아래 오류 노트)
    .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({ "error": { "code": "NOT_FOUND", "message": "없는 API 경로입니다." } }))) })
    .layer(cors);                                   // CORS는 API에만. 정적 파일에는 필요 없다

// SPA: 파일이 없으면 index.html을 200으로 돌려준다.
let spa = ServeDir::new("static").fallback(ServeFile::new("static/index.html"));

let app = Router::new().nest("/api", api).fallback_service(spa);
```

| 코드 | 설명 |
|---|---|
| `CORS_ORIGIN` 환경변수 | 허용 출처를 코드가 아닌 설정으로 (공통 API 스펙 v1 §7). 운영에서는 실제 프론트 주소 |
| `.allow_credentials(true)` | 응답에 `access-control-allow-credentials: true`. 브라우저가 **쿠키를 요청에 싣고 응답의 `Set-Cookie`를 저장**하도록 허락 |
| `.layer(cors)`가 `api`에만 | `nest("/api", ...)` 이전에 붙여서 API 경로에만 적용 |
| `ServeDir::new("static").fallback(ServeFile::new("static/index.html"))` | 파일이 없으면 `index.html`(SPA 진입점)을 **200으로**. 딥링크(`/posts/3`)로 새로고침해도 SPA가 뜬다 |
| `.fallback_service(spa)` | `/api` 이외의 모든 경로는 정적 서빙으로 |

> ⚠️ `ServeDir::not_found_service`가 아니라 **`fallback`** 이다. `not_found_service`는 내부에서 **상태를 404로 강제**한다. SPA 진입점은 200이어야 하고, 404 페이지는 검색엔진이 색인하지 않는다. (소스를 열어 확인한 사실이다)

로그인·쿠키:

```rust
async fn login() -> impl IntoResponse {
    (StatusCode::OK,
     [(header::SET_COOKIE, "sid=demo-session-1; HttpOnly; SameSite=Lax; Path=/; Max-Age=3600")],
     Json(json!({ "data": { "nickname": "mina" } })))
}
```

`HttpOnly`(JS가 못 읽음), `SameSite=Lax`(다른 사이트의 POST에는 안 실림), `Max-Age`. 운영 HTTPS에서는 `Secure` 추가 (study7).

### 3-2. 실행 결과 (`curl`)

```
$ curl -o /dev/null -w "%{http_code}" http://127.0.0.1:3012/                 → 200   (index.html)
$ curl -o /dev/null -w "%{http_code}" http://127.0.0.1:3012/some/deep/route  → 200   (SPA fallback)
$ curl -o /dev/null -w "%{http_code}" http://127.0.0.1:3012/api/me           → 401   (쿠키 없음)

$ curl -c jar -i -X POST http://127.0.0.1:3012/api/login
HTTP/1.1 200 OK
set-cookie: sid=demo-session-1; HttpOnly; SameSite=Lax; Path=/; Max-Age=3600

$ curl -b jar http://127.0.0.1:3012/api/me
{"data":{"nickname":"mina"}}

# preflight (브라우저가 POST/JSON/쿠키 요청 전에 먼저 보내는 OPTIONS)
$ curl -i -X OPTIONS http://127.0.0.1:3012/api/me -H 'Origin: http://localhost:5173' -H 'Access-Control-Request-Method: GET'
HTTP/1.1 200 OK
access-control-allow-credentials: true
access-control-allow-methods: GET,POST,PATCH,DELETE
access-control-allow-headers: content-type,authorization
access-control-allow-origin: http://localhost:5173
```

**다른 출처의 요청**(`Origin: http://evil.example`)에도 서버는 `access-control-allow-origin: http://localhost:5173`을 **그대로** 돌려준다. 요청 출처와 다르다는 판단과 **차단은 브라우저가** 한다(서버는 200으로 응답한다). 그래서 CORS는 서버를 보호하는 장치가 아니라, **다른 사이트의 스크립트가 응답을 읽지 못하게** 하는 장치다.

### 3-3. 프론트 API 클라이언트 — [lab/static/api.js](lab/static/api.js)

```js
export class ApiError extends Error {
  constructor(status, error = {}) {
    super(error.message ?? `요청 실패 (${status})`);
    this.name = "ApiError";
    this.status = status;                       // HTTP 상태 (네트워크 오류면 0)
    this.code = error.code ?? "UNKNOWN";        // 기계가 읽는 코드 → 화면 분기는 이것으로
    this.fields = error.fields ?? null;         // { 필드명: [메시지, ...] } → 입력칸 아래에
    this.requestId = error.requestId ?? null;   // 문의/로그 추적용
  }
}

export async function api(path, { method = "GET", body, token, base = "", credentials = false, signal, fetchImpl = fetch } = {}) {
  const headers = { Accept: "application/json" };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  if (token) headers.Authorization = `Bearer ${token}`;

  let res;
  try {
    res = await fetchImpl(base + path, {
      method, headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: credentials ? "include" : "same-origin",
      signal,
    });
  } catch (cause) {
    if (cause?.name === "AbortError") throw cause;      // 사용자가 취소한 것은 오류로 취급하지 않는다
    throw new ApiError(0, { code: "NETWORK_ERROR", message: "서버에 연결할 수 없습니다." });
  }

  if (res.status === 204) return { data: null };        // 삭제 등 본문 없는 성공

  const text = await res.text();
  let json = null;
  try { json = text ? JSON.parse(text) : null; } catch { /* JSON이 아닌 응답 (프록시 오류 페이지 등) */ }

  if (!res.ok) {
    throw new ApiError(res.status, json?.error ?? { code: "BAD_RESPONSE", message: `요청 실패 (${res.status})` });
  }
  return json ?? { data: null };
}
```

| 코드 | 설명 |
|---|---|
| `ApiError`의 네 필드 | 화면이 쓰는 것은 `code`(분기), `fields`(입력칸), `message`(보조 문구), `requestId`(문의) |
| `base` | 다른 출처의 API를 부를 때(방식 C). 같은 출처/프록시면 빈 문자열 |
| `credentials: "include"` | 쿠키 인증일 때. **서버 CORS의 `allow_credentials(true)`와 정확한 출처가 짝**이다 |
| `AbortError`는 그대로 던짐 | 화면 이동·재검색으로 이전 요청을 취소(`AbortController`)한 것을 "서버 오류"로 보여 주면 안 된다 |
| `res.status === 204` | 본문이 없다. `res.json()`을 부르면 터진다 |
| `try { JSON.parse }` | 프록시의 502 HTML 페이지처럼 **JSON이 아닌 오류 응답**도 `BAD_RESPONSE`로 정규화 |
| `fetchImpl` 주입 | 테스트에서 가짜 `fetch`를 넣는 자리 (study9의 같은 원리) |

### 3-4. 테스트 결과 (`node --test lab/static/api.test.mjs`)

```
✔ 성공 응답은 { data, meta } 를 그대로 돌려준다
✔ 본문이 있으면 JSON으로 보내고 토큰은 Bearer 헤더로
✔ 검증 실패(400)는 code·fields·requestId를 가진 ApiError
✔ 401은 화면이 code로 분기할 수 있다
✔ 204는 본문 없이 성공
✔ 네트워크 오류는 status 0 / NETWORK_ERROR
✔ JSON이 아닌 오류 응답(프록시 502 등)도 ApiError로
✔ 취소(AbortError)는 오류 화면을 띄우지 않도록 그대로 던진다
✔ 쿠키 인증이면 credentials: include
ℹ tests 9  pass 9  fail 0
```

실제 서버에 붙여 본 결과 (s14 `/api/v1/posts`, s12 `/api/me`):

```
list:    { limit: 2, page: 2, q: null, total: 37, totalPages: 19 } [ 35, 34 ]
error:   400 VALIDATION_ERROR [ 'sort: createdAt, -createdAt, -commentCount 중 하나여야 합니다.' ]
401:     401 UNAUTHORIZED
network: 0 NETWORK_ERROR
서버가 계약을 안 지키는 404(본문이 JSON 오류가 아님):  404 BAD_RESPONSE
/api.js 의 content-type: text/javascript            ← ES 모듈로 import 가능
```

### 3-5. 화면에서 쓰는 모습 — [lab/static/index.html](lab/static/index.html)

```js
import { api, ApiError } from "/api.js";

const run = async (path, method = "GET") => {
  try {
    const res = await api(path, { method, credentials: true });
    out.textContent = JSON.stringify(res, null, 2);
  } catch (e) {
    if (!(e instanceof ApiError)) throw e;                       // 예상 못 한 예외는 숨기지 않는다
    out.textContent = `${e.status} ${e.code}\n${e.message}` + (e.requestId ? `\nrequestId: ${e.requestId}` : "");
  }
};
```

### 3-6. UI 라이브러리 · 상태 관리 · 폼 라이브러리에 꽂는 자리 (스케치)

> 아래는 **프론트 쪽 코드의 모양**이다. 이 저장소에서 실행하지 않았고, `api.js`의 계약(테스트 9개)에만 의존한다. 라이브러리 이름과 API는 사용하는 버전의 문서를 확인한다.

**서버 상태(목록·상세) — 캐시 키는 URL, 쓰기 성공 후 무효화**

```ts
// 키가 곧 요청 URL이라서, API의 쿼리 이름(page, limit, q)이 그대로 키가 된다
const posts = useQuery({ queryKey: ["posts", { page, q }], queryFn: () => api(`/api/posts?page=${page}&q=${q}`) });

const create = useMutation({
  mutationFn: (body) => api("/api/posts", { method: "POST", body, token }),
  onSuccess: () => queryClient.invalidateQueries({ queryKey: ["posts"] }),   // 목록 캐시를 낡은 것으로 표시 → 다시 가져옴
});
```

**폼 — 서버의 `fields`를 필드 오류로**

```ts
try {
  await api("/api/auth/signup", { method: "POST", body: values });
} catch (e) {
  if (e instanceof ApiError && e.code === "VALIDATION_ERROR") {
    for (const [name, msgs] of Object.entries(e.fields ?? {})) setError(name, { message: msgs.join(" ") });   // 폼 라이브러리의 setError
  } else if (e.code === "EMAIL_TAKEN") {
    setError("email", { message: e.message });
  } else {
    setFormError(e.message);        // 그 밖의 오류는 폼 상단에
  }
}
```

**클라이언트 상태 — 로그인 사용자**: 로그인 성공 시 사용자 객체를 전역 저장소(예: 스토어)에 넣고, **401을 받으면 비우고 로그인 화면으로** 보낸다(`api()`를 감싸는 한 곳에서 처리). 토큰은 `localStorage`에 두지 않는다(study7) — 쿠키 방식이면 프론트는 토큰을 만지지 않는다.

**타입 공유(선택)**: `demo/`는 `GET /api-docs/openapi.json`을 제공한다(utoipa). 이 JSON에서 TypeScript 타입을 생성하는 도구를 쓰면 서버의 `camelCase` DTO와 프론트 타입이 어긋나지 않는다(study14).

---

## ④ 🔗 연동 포인트

| 접점 | 서버가 하는 일 | 프론트가 하는 일 |
|---|---|---|
| 경로 | `/api/...` 접두어 (스펙 v1) | `base`와 경로 한 곳에 |
| CORS | `CORS_ORIGIN` 환경변수로 정확한 출처 | 개발은 프록시로 우회하거나 `base`+`credentials` |
| 인증 (쿠키) | `HttpOnly; SameSite=Lax` (운영 `Secure`) | `credentials: "include"`, 토큰을 만지지 않음 |
| 인증 (Bearer) | `Authorization` 헤더 검증 | 토큰을 **메모리**에 두고 헤더로 |
| 성공 | `{ data, meta }` | `res.data`, `res.meta`로 화면 |
| 실패 | `{ error: { code, message, fields, requestId } }` | `ApiError.code`로 분기, `fields`는 입력칸에, `requestId`는 문의 화면에 |
| 캐시 | `ETag`/`304` (study14) | 서버 상태 캐시 키 = URL |
| SPA 새로고침 | `fallback`으로 `index.html` 200 | History 라우팅 |

**공통 API 스펙 v1이 프론트를 지킨다**: 백엔드를 바꿔 끼워도(SQLite→PostgreSQL→MongoDB, study11) `api.js`는 한 줄도 안 바뀐다. 반대로 서버가 스펙을 어기면(예: 평문 400) `BAD_RESPONSE`로 **드러난다.** 그 차이 목록은 [study6 ④](study6.md)에 있다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 브라우저 콘솔 `blocked by CORS policy: No 'Access-Control-Allow-Origin'` (curl은 성공) | 허용 출처가 요청 출처와 **글자 그대로** 다름 (`localhost` vs `127.0.0.1`, 포트, `http`/`https`) | `CORS_ORIGIN`을 브라우저 주소창의 출처와 똑같이 |
| `Credential is not supported if the CORS header 'Access-Control-Allow-Origin' is '*'` | 쿠키를 쓰는데 허용 출처가 `*` | 출처를 정확히 적고 `allow_credentials(true)` |
| 로그인은 200인데 다음 요청에서 또 401 (다른 출처) | 쿠키가 저장/전송되지 않음: `credentials: "include"` 누락, 또는 `SameSite`/`Secure` 조건 불충족 | `fetch` 옵션 + 서버 CORS 둘 다. 로컬 `http`에서 `Secure` 쿠키는 저장되지 않는다 |
| POST만 CORS로 실패 | `application/json`은 **사전 요청(OPTIONS)** 이 먼저 나간다 | `allow_methods`/`allow_headers`에 필요한 것 포함 |
| SPA 새로고침하면 404 | `ServeDir`의 `not_found_service`는 **상태를 404로 강제** / fallback 없음 | `.fallback(ServeFile::new("index.html"))` |
| `/api/없는경로`가 **`index.html`(200)** 로 응답 (실제로 겪음) | nest된 라우터에 자기 `fallback`이 없으면 **바깥의 SPA fallback**이 `/api/nope`까지 받아 감 → API 클라이언트가 HTML을 JSON으로 파싱하려다 실패 | API 라우터에 **자기 `fallback`(JSON 404)** 을 둔다. 실측: 수정 후 `/api/nope` → 404 JSON, `/posts/3`(딥링크) → 200 |
| 개발 프록시에서는 되는데 운영에서 실패 | 프록시가 숨겨 준 CORS/경로 문제 | 운영 구성(같은 출처 or 리버스 프록시)에서 한 번 점검 |
| 화면에 "서버 오류"가 뜨는데 실제로는 사용자가 이동하며 취소 | `AbortError`를 오류로 취급 | `api()`처럼 `AbortError`는 그대로 던지고 화면은 무시 |
| 204 응답에서 `Unexpected end of JSON input` | 본문 없는 응답에 `res.json()` | `status === 204`는 파싱하지 않는다 |
| 오류 응답이 HTML이라 `JSON.parse`가 터짐 | 앞단 프록시/로드밸런서의 502·504 페이지 | JSON 파싱을 `try`로 감싸 `BAD_RESPONSE`로 (`api.js`) |
| 날짜가 하루 어긋나 보임 | 서버는 UTC ISO 8601, 프론트가 시간대를 무시하고 잘라 씀 | `new Date(createdAt)`로 파싱해 **표시할 때** 지역 시간으로 |

---

## ⑥ 직접 해보기

1. **방식 B 체험**: 아무 정적 폴더를 `python -m http.server 5173`으로 띄우고 `s12_front`를 `CORS_ORIGIN=http://localhost:5173`으로 실행한 뒤, 그 페이지에서 `fetch("http://127.0.0.1:3012/api/me", { credentials: "include" })`를 호출한다. `allow_credentials`를 빼면 어떤 콘솔 메시지가 나오는지 읽는다.
2. **`fieldMessages` 확장**: `ApiError.fields`를 받아 `{ title: "…" }`처럼 필드당 한 문장으로 합치는 `firstMessages(error)`를 `api.js`에 추가하고 `api.test.mjs`에 시험을 쓴다.
3. **401 처리를 한 곳에**: `api()`를 감싸 `status === 401`이면 저장된 사용자 정보를 비우고 `onUnauthorized()` 콜백을 부르게 한다. 화면 코드에는 401 분기가 하나도 없어야 한다.

---

## ⑦ ☐ 체크리스트

- ☐ 같은 출처 서빙 / 개발 프록시 / CORS 세 방식의 차이와 쓰는 때를 설명할 수 있다
- ☐ `ServeDir`에서 `fallback`과 `not_found_service`의 차이(상태 코드)를 안다
- ☐ 쿠키 인증 CORS의 조건 세 가지(`credentials: "include"`, 정확한 Origin, `allow_credentials(true)`)를 말할 수 있다
- ☐ CORS가 서버가 아니라 **브라우저의** 검사라는 것을 `curl` 결과로 확인했다
- ☐ preflight(OPTIONS)가 언제 나가고 서버가 무엇을 답해야 하는지 안다
- ☐ `api()`/`ApiError`로 성공·실패 계약을 한 곳에서 해석했고, `node --test`가 통과한다
- ☐ `error.fields`를 폼 라이브러리의 필드 오류로 꽂는 흐름을 설명할 수 있다
- ☐ 서버 상태(목록)의 캐시 키가 URL이고 쓰기 후 무효화한다는 것을 안다
- ☐ 토큰을 `localStorage`에 두지 않는 이유를 설명할 수 있다

---

## ⑧ 다음 단계 연결

프론트가 붙었으니 이제 서버가 **바깥 서비스**와 이야기할 차례다. [study13](study13.md)에서는 결제 웹훅(서명 검증), 메일 전송, 파일 업로드, 소셜 로그인(OAuth2)을 연결하고, 외부 장애가 우리 서비스를 막지 않게 만든다.
