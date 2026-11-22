# study1 — Hello World와 기본 문법

> 🎯 **목표**: 첫 응답을 문자열·JSON·HTML 세 가지 모양으로 띄우고, 핸들러의 기본 문법(경로 파라미터·쿼리·반환 타입)과 "서버가 HTML을 만드는" 템플릿 표현을 익힌다. 자동 재시작과 브라우저 개발자도구로 결과를 확인한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s01_hello.rs](lab/examples/s01_hello.rs) (실행: `cd lab` → `cargo run --example s01_hello`, 포트 3001) |
| 검증 | 아래 `curl` 결과는 모두 실제로 실행해 얻은 출력이다 |

---

## ① 목표

- 핸들러가 "**평범한 `async fn`**"이라는 사실을 코드로 확인한다.
- `&str` / `String` / `Json<T>` / `Html<String>` / `(StatusCode, Json<T>)`가 전부 응답이 되는 이유를 이해한다.
- 경로 파라미터(`{name}`), 쿼리(`?name=`)를 타입으로 받는다.
- 사용자 입력을 HTML에 넣을 때 **이스케이프**가 왜 필요한지 눈으로 본다.

---

## ② 📖 이론

### 핸들러 = 인자는 extractor, 반환은 IntoResponse

```rust
async fn greet(Query(q): Query<GreetQuery>) -> Json<Greeting> { ... }
//             └ extractor (요청에서 꺼냄)         └ IntoResponse (응답으로 바뀜)
```

애노테이션도, 부모 클래스도 없다. **함수 시그니처가 곧 계약**이다.

- 인자: `Path<T>`, `Query<T>`, `Json<T>`, `State<T>`, `HeaderMap` … 요청에서 값을 꺼내 준다. 꺼내지 못하면 **핸들러는 호출되지 않고** 400/415/422 같은 응답이 먼저 나간다.
- 반환: `IntoResponse`를 구현한 타입이면 무엇이든 된다.

| 반환 타입 | 결과 |
|---|---|
| `&'static str`, `String` | `200`, `text/plain; charset=utf-8` |
| `Json<T>` (T: Serialize) | `200`, `application/json` |
| `Html<String>` | `200`, `text/html; charset=utf-8` |
| `StatusCode` | 상태 코드만, 본문 없음 |
| `(StatusCode, Json<T>)` | 상태 코드 + 본문 |
| `(StatusCode, [(헤더, 값)], Json<T>)` | 상태 코드 + 헤더 + 본문 |
| `Result<T, E>` (둘 다 `IntoResponse`) | `Ok`면 T, `Err`면 E가 응답 — **오류 처리의 기본형** (study5) |

### 비유 — 자동판매기

버튼(경로)을 누르면 정해진 칸(핸들러)에서 물건(응답)이 나온다. 돈이 모자라면(extractor 실패) **칸을 열어 보지도 않고** "투입 금액 부족"(400)이 나온다.

### 템플릿 표현이란

서버가 **데이터를 끼워 넣은 HTML**을 만들어 주는 것을 말한다. 가장 원시적인 형태는 `format!`이고, 실무에서는 `askama`(컴파일 타임 검사 템플릿)·`maud`(Rust 매크로로 HTML 작성) 같은 템플릿 엔진을 쓴다. 이 저장소는 템플릿 엔진 없이 **JSON API + 정적 HTML/JS** 구조를 택했다(study3, studyFinal).

> ⚠️ 템플릿 표현에서 가장 중요한 한 가지: **사용자가 준 값은 반드시 이스케이프한다.** 안 하면 XSS다(study7). 템플릿 엔진을 쓰는 가장 큰 이유가 "기본이 이스케이프"이기 때문이다.

---

## ③ 🛠️ 실기

### 3-1. 전체 코드 — [lab/examples/s01_hello.rs](lab/examples/s01_hello.rs)

```rust
use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(root))
        .route("/hello/{name}", get(hello))
        .route("/greet", get(greet))
        .route("/page/{name}", get(page))
        .route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> &'static str {
    "Hello, Axum!"
}

async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

#[derive(Deserialize)]
struct GreetQuery {
    name: String,          // 필수. 없으면 400
    times: Option<u8>,     // Option이면 선택
}

#[derive(Serialize)]
struct Greeting {
    message: String,
    times: u8,
}

async fn greet(Query(q): Query<GreetQuery>) -> Json<Greeting> {
    let times = q.times.unwrap_or(1).min(10);
    Json(Greeting { message: format!("안녕하세요, {}님!", q.name), times })
}

async fn page(Path(name): Path<String>) -> Html<String> {
    let safe = escape_html(&name);
    Html(format!(r#"<!doctype html>
<html lang="ko">
  <head><meta charset="utf-8"><title>Hello {safe}</title></head>
  <body>
    <h1>안녕하세요, {safe}님</h1>
  </body>
</html>"#))
}

fn escape_html(input: &str) -> String {
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
         .replace('"', "&quot;").replace('\'', "&#39;")
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}
```

### 3-2. 줄별 설명

| 코드 | 설명 |
|---|---|
| `.route("/hello/{name}", get(hello))` | `{name}`이 **경로 파라미터**. **axum 0.8부터 `:name`이 아니라 `{name}`** 이다 |
| `async fn root() -> &'static str` | 인자가 없는 가장 단순한 핸들러. 문자열 리터럴은 `'static`이라 그대로 반환 가능 |
| `Path(name): Path<String>` | **패턴 매칭으로 바로 값을 꺼내는** 문법. `Path<String>`을 받아 안쪽 `String`을 `name`에 묶는다. `Path<u32>`로 쓰면 숫자가 아닐 때 400이 난다 |
| `format!("Hello, {name}!")` | 변수 이름을 중괄호에 바로 쓰는 인라인 포맷 (Rust 2021+) |
| `#[derive(Deserialize)] struct GreetQuery` | 쿼리스트링을 이 구조체로 **자동 변환**. `serde`가 해 준다 |
| `name: String` vs `times: Option<u8>` | 필수/선택의 차이를 **타입으로** 표현한다. 검사 코드를 쓰지 않는다 |
| `q.times.unwrap_or(1).min(10)` | 없으면 1, 있어도 상한 10. **클라이언트가 보낸 숫자는 믿지 않고 서버가 상한을 건다** |
| `#[derive(Serialize)] struct Greeting` | 구조체 → JSON 변환. `Json(...)`으로 감싸 반환 |
| `Html(format!(r#"..."#))` | `r#"..."#`는 **raw 문자열**(따옴표·줄바꿈을 이스케이프 없이). `Html`이 `Content-Type: text/html`을 붙인다 |
| `{safe}` | 템플릿 "변수 자리". 여기에 들어가는 값은 반드시 `escape_html`을 거친 것 |
| `(StatusCode::OK, Json(...))` | 튜플로 상태 코드를 지정. `impl IntoResponse`는 "구체 타입은 숨기고 응답이 되는 무언가"라는 뜻 |

### 3-3. 실행과 결과

```powershell
cd lab
cargo run --example s01_hello
```

```
$ curl http://127.0.0.1:3001/
Hello, Axum!

$ curl http://127.0.0.1:3001/hello/mina
Hello, mina!

$ curl "http://127.0.0.1:3001/greet?name=mina&times=3"
{"message":"안녕하세요, mina님!","times":3}

$ curl -o /dev/null -w "%{http_code}" "http://127.0.0.1:3001/greet"      # name을 안 보냄
400
```

**XSS 이스케이프 확인** — 이름 자리에 `<script>alert(1)</script>`를 넣어 보았다.

```
$ curl "http://127.0.0.1:3001/page/%3Cscript%3Ealert(1)%3C%2Fscript%3E" | grep h1
    <h1>안녕하세요, &lt;script&gt;alert(1)&lt;/script&gt;님</h1>
```

`<`가 `&lt;`로 바뀌어 **글자 그대로 화면에 보인다.** 이스케이프를 빼면 브라우저가 이 스크립트를 실행한다.

---

## ④ 🔗 연동 포인트

### 개발 서버 자동 재시작

```powershell
cargo watch -q -x "run --example s01_hello"      # 프로젝트라면 cargo watch -q -x run
```

저장 → 재빌드 → 재시작. 실제로 응답 문자열을 바꿔 저장했더니 `Hello, Axum!`이 `Hello, Axum! (reloaded)`로 바뀌었다. **브라우저 자동 새로고침(HMR)은 없다**: 서버가 다시 뜬 뒤 직접 새로고침한다. (프론트 개발 서버가 따로 있는 구조는 study12)

### 브라우저 개발자도구로 결과 확인 (F12)

| 탭 | 확인할 것 |
|---|---|
| **Network** | `/greet?name=mina` 요청을 클릭 → **Status**(200/400), **Response Headers**의 `content-type`, **Response/Preview**의 JSON 본문, **Timing** |
| **Network → Headers** | `/greet`(성공)과 `/greet`(name 없음, 400)을 비교. 400의 본문이 JSON이 아니라 **평문**이라는 점을 기억해 둔다 (study5에서 통일) |
| **Elements** | `/page/...`로 만든 HTML에서 `<h1>`의 텍스트가 이스케이프됐는지 |
| **Console** | `fetch("/greet?name=mina").then(r => r.json()).then(console.log)` 로 JS에서 같은 요청을 재현 |

`curl -i`(헤더 포함 출력)는 개발자도구 Network 탭의 터미널 버전이다.

### 다음 단계와 이어지는 것

`greet`의 `Json<Greeting>` 반환은 그대로 프론트가 `fetch`로 받는 **API의 모양**이다. 이후 모든 단계가 이 패턴(extractor → 로직 → `IntoResponse`)의 확장이다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 실행하자마자 패닉: `Path segments must not start with ':'` | **axum 0.8에서 `:id` → `{id}`로 변경.** 옛 블로그 예제의 `/hello/:name` | 경로를 `/hello/{name}`으로. **컴파일은 통과하고 실행할 때** 패닉으로 알려 준다는 점이 함정 |
| 컴파일 에러: `the trait bound ... Handler<_, _> is not satisfied` | 핸들러 조건 불충족. 가장 자주 보는 에러. 원인은 보통 ① **본문 extractor(`Json`)가 마지막 인자가 아님** ② 인자 타입에 `#[derive(Deserialize)]` 누락 ③ 반환 타입이 `IntoResponse`가 아님 ④ `State`를 쓰는데 `.with_state()`를 빠뜨림(study2) | `#[axum::debug_handler]`를 핸들러에 붙이면 **원인을 정확히 짚어 준다** (`axum`의 `macros` feature 필요) |
| `/greet`에서 400, 본문이 JSON이 아니라 평문 | `Query` extractor 실패(필수 `name` 없음). 기본 거절 응답은 평문 | study5에서 오류 형식을 통일 |
| 한글이 `???`로 보임 (PowerShell 출력) | 터미널 코드페이지 문제 (서버 문제가 아님) | 브라우저나 UTF-8 설정으로 확인. 한글 JSON을 **보낼** 때는 `--data-binary @파일.json` |
| `found an item that was configured out` (예: `tokio::net`) | tokio의 **feature가 꺼져 있음** | `cargo add tokio --features net,macros,rt-multi-thread` — 컴파일러가 원인을 정확히 알려 주는 좋은 예 |
| 재시작이 안 되고 `Access is denied` | 이전에 띄운 서버가 실행 중인 채 재빌드 (Windows) | 서버 종료 후 빌드, 또는 `cargo watch` 사용 |

---

## ⑥ 직접 해보기

1. **`/add/{a}/{b}`**: 두 정수를 더한 결과를 JSON으로 돌려주는 핸들러를 만든다. `Path<(i32, i32)>`를 쓴다. `/add/1/abc`는 어떤 상태 코드가 나오는가?
2. **`/page/{name}`에서 `escape_html`을 일부러 빼 보기**: `<b>굵게</b>`를 넣어 렌더링이 어떻게 달라지는지 개발자도구 Elements 탭으로 확인하고, 다시 복구한다.
3. **상한 확인**: `/greet?name=mina&times=999`를 호출하고 `times`가 몇으로 돌아오는지 본다. 상한을 서버에서 거는 것과 클라이언트에서 거는 것의 차이를 한 줄로 적어 본다.

---

## ⑦ ☐ 체크리스트

- ☐ `&str`, `String`, `Json`, `Html`, `(StatusCode, Json)`이 각각 어떤 응답이 되는지 표 없이 말할 수 있다
- ☐ 경로 파라미터는 `{name}`(0.8)이고 `:name`은 패닉이라는 것을 기억한다
- ☐ `Query<T>`에서 필수와 선택을 `T`와 `Option<T>`로 구분한다
- ☐ 본문 extractor는 **마지막 인자**여야 한다는 규칙을 안다
- ☐ `format!`로 만든 HTML에 사용자 입력을 넣을 때 이스케이프하는 이유를 설명할 수 있다
- ☐ `cargo watch`로 저장 시 재시작을 확인했다
- ☐ 개발자도구 Network 탭에서 상태 코드·헤더·본문을 읽을 수 있다
- ☐ `Handler is not satisfied` 에러의 원인 후보 네 가지를 댈 수 있다

---

## ⑧ 다음 단계 연결

지금까지는 핸들러 몇 개가 `main.rs`에 한 덩어리로 있었다. [study2](study2.md)에서는 이것을 **모듈로 나누고**, 모듈끼리 서로를 부르고, `State`로 데이터를 공유하는 Axum의 기본 구조를 만든다.
