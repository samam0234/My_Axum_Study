# 설치와 첫 Axum 서버 (Study 1)

[study0.md](study0.md)에서 개념을 봤으니 이제 실제로 손을 움직입니다. 이 문서에서는 Rust 툴체인 설치 → Cargo 프로젝트 생성 → Axum 서버 실행 → 기본 extractor(`Path`, `Query`) 실습까지 진행합니다.

**이 문서에서 만드는 것**: `demo/` 프로젝트 + 4개의 엔드포인트(`/`, `/health`, `/hello/{name}`, `/echo`)

---

## 1. Rust 설치

### Windows

Rust는 `rustup`이라는 **툴체인 관리자**를 통해 설치합니다. (Node의 nvm, Python의 pyenv에 해당)

```powershell
# 방법 1: winget
winget install Rustlang.Rustup

# 방법 2: https://rustup.rs 에서 rustup-init.exe 다운로드 후 실행
```

> ⚠️ **Windows에서는 Rust만 설치해서는 컴파일이 안 됩니다.** Rust의 기본 타깃인 `x86_64-pc-windows-msvc`는 **MSVC 링커(`link.exe`)와 Windows SDK**를 사용합니다. Visual Studio Installer에서 **"C++를 사용한 데스크톱 개발"** 워크로드를 설치해야 합니다. (Visual Studio Community 또는 Build Tools 둘 다 가능)

### 설치 확인

```powershell
rustc --version
cargo --version
```

실제 출력:

```
rustc 1.98.1 (48a229cea 2026-09-01)
cargo 1.98.1 (797e8a9bc 2026-08-05)
```

- `rustup`은 `%USERPROFILE%\.cargo\bin`에 실행 파일을 설치하고 PATH에 추가합니다. 설치 직후 현재 터미널에서 `cargo`를 못 찾으면 **터미널을 새로 열면** 됩니다.

---

## 2. Cargo 명령어 정리

Cargo는 **빌드 도구 + 패키지 매니저 + 테스트 러너 + 포매터 실행기**를 겸합니다. npm + webpack + jest를 한 덩어리로 묶었다고 보면 됩니다.

| 명령어 | 하는 일 | npm 대응 |
|---|---|---|
| `cargo new <name>` | 새 프로젝트 생성 (바이너리) | `npm init` |
| `cargo new <name> --lib` | 라이브러리 크레이트로 생성 | — |
| `cargo add <crate>` | 의존성 추가 (Cargo.toml 자동 수정) | `npm install <pkg>` |
| `cargo add <crate> --features a,b` | 특정 기능 플래그와 함께 추가 | — |
| `cargo remove <crate>` | 의존성 제거 | `npm uninstall` |
| **`cargo check`** | **타입/빌림 검사만 (코드 생성 X → 빠름)** | `tsc --noEmit` |
| `cargo build` | 디버그 빌드 → `target/debug/` | `npm run build` |
| `cargo build --release` | 최적화 빌드 → `target/release/` | `npm run build -- --prod` |
| `cargo run` | 빌드 + 실행 | `npm start` |
| `cargo test` | 테스트 실행 | `npm test` |
| `cargo fmt` | 코드 포매팅 | `prettier --write` |
| `cargo clippy` | 린트 (관용구 위반, 잠재 버그 지적) | `eslint` |
| `cargo tree` | 의존성 트리 출력 | `npm ls` |
| `cargo update` | 의존성 잠금 파일 갱신 | `npm update` |
| `cargo doc --open` | 내 프로젝트 + 의존성 문서 생성/열기 | — |

> 💡 **개발 중에는 `cargo build`보다 `cargo check`를 쓰세요.** 실행 파일을 만들지 않고 타입 검사만 하기 때문에 몇 배 빠릅니다. 실제로 돌려볼 때만 `cargo run`을 씁니다.
>
> 💡 `cargo doc --open`은 생각보다 강력합니다. **내가 쓰는 버전 그대로의** 의존성 문서가 로컬에 생성되기 때문에, axum 버전이 달라서 생기는 문서 불일치 문제가 없습니다.

---

## 3. 프로젝트 생성

```powershell
cd Auxm
cargo new demo --vcs none   # 상위 폴더가 이미 git 저장소라 --vcs none
cd demo
```

`--vcs none`을 준 이유: `cargo new`는 기본적으로 그 폴더에 `git init`을 해버립니다. 여기서는 상위 `Auxm/`이 이미 저장소이므로 중첩 저장소가 생기지 않게 막습니다.

생성 결과:

```
demo/
├─ Cargo.toml
└─ src/
   └─ main.rs
```

### 의존성 추가

```powershell
cargo add axum
cargo add tokio --features rt-multi-thread,macros,net
cargo add serde --features derive
cargo add serde_json
```

결과 `Cargo.toml`:

```toml
[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8.9"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "net"] }
```

### `Cargo.toml` 읽는 법

| 항목 | 의미 |
|---|---|
| `name` | 크레이트 이름. 빌드 결과 실행 파일 이름이 된다 (`target/debug/demo.exe`) |
| `edition` | Rust 에디션(언어 방언). `2015`/`2018`/`2021`/`2024`가 있고, 새 에디션일수록 문법이 개선됨. **버전 업이 아니라 옵트인 방식이라 하위 호환이 깨지지 않는다** |
| `[dependencies]` | 의존 크레이트 목록 |
| `"0.8.9"` | **캐럿(`^`) 요구사항의 축약.** `>=0.8.9, <0.9.0` 을 의미. 0.x에서는 마이너 버전이 breaking change 경계 |

### **features**: Rust 생태계 특유의 개념

```toml
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "net"] }
```

- 많은 크레이트가 **기능을 옵션으로 쪼개 놓고, 필요한 것만 켜게** 합니다. 안 켠 기능은 **아예 컴파일되지 않으므로** 빌드 시간과 바이너리 크기가 줄어듭니다.
- tokio는 특히 심합니다. 위에서 켠 것들의 의미:

| feature | 의미 |
|---|---|
| `rt-multi-thread` | 멀티스레드 런타임 (이게 없으면 단일 스레드만) |
| `macros` | `#[tokio::main]`, `#[tokio::test]` 매크로 |
| `net` | TCP/UDP 소켓 (`TcpListener`) |

- 귀찮으면 `features = ["full"]`로 전부 켤 수도 있지만, 학습용이라도 **뭐가 왜 필요한지 알고 켜는 편**이 낫습니다.
- serde의 `derive`도 같은 맥락입니다. `#[derive(Serialize)]`를 쓰려면 이 기능이 켜져 있어야 합니다.

---

## 4. 첫 서버 — `src/main.rs`

```rust
use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};

#[tokio::main]
async fn main() {
    let app = app();

    // 0.7부터 hyper Server가 아니라 tokio TcpListener + axum::serve 조합을 쓴다.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("3000 포트를 열 수 없습니다");

    println!("listening on http://{}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

/// 라우터 조립만 따로 함수로 빼두면 나중에 통합 테스트에서 그대로 재사용할 수 있다. (study9)
fn app() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/hello/{name}", get(hello))
        .route("/echo", get(echo))
}
```

### 한 줄씩 뜯어보기

**`#[tokio::main]`**

```rust
#[tokio::main]
async fn main() { ... }
```

이 매크로는 아래 코드로 펼쳐집니다.

```rust
fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { ... })   // 여기서 비로소 async 코드가 실행된다
}
```

- `main`은 원래 async일 수 없습니다. 이 매크로가 **동기 `main` 안에서 런타임을 만들고 async 블록을 실행**해 줍니다.
- FastAPI의 `uvicorn.run(app)`, Node의 이벤트 루프 부팅에 해당하는 자리입니다.

**`TcpListener` + `axum::serve`**

- axum 0.6까지는 `axum::Server::bind(...)`가 있었지만 **0.7에서 제거**되었습니다(hyper 1.0 대응). 지금은 tokio 리스너를 직접 만들어 `axum::serve`에 넘깁니다.
- 인터넷의 오래된 예제가 `axum::Server`를 쓰고 있다면 **0.6 시절 코드**입니다.

**`.expect(...)` vs `.unwrap()`**

- 둘 다 실패 시 패닉하지만 `expect`는 메시지를 남깁니다. **서버 부팅 단계는 실패하면 어차피 못 뜨는 구간**이라 패닉이 적절합니다. 반면 요청 처리 핸들러 안에서는 절대 쓰지 않습니다. (study4에서 제대로 된 오류 처리로 대체)

**`fn app() -> Router`를 따로 뺀 이유**

- 라우터 조립을 함수로 분리해 두면 `main`에서도, 테스트에서도 같은 라우터를 씁니다. study9의 통합 테스트에서 이 함수를 그대로 재사용합니다. **처음부터 이렇게 짜두는 게 좋습니다.**

---

## 5. 핸들러 4종 — extractor 실습

### 5-1. 문자열 그대로 반환

```rust
async fn root() -> &'static str {
    "Hello, Axum!"
}
```

`&'static str`은 `IntoResponse`를 구현하고 있어서 그대로 반환하면 `200 OK` + `text/plain; charset=utf-8`이 됩니다.

### 5-2. 상태 코드 + JSON

```rust
async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}
```

- **튜플 `(StatusCode, T)`가 `IntoResponse`** 입니다. 이게 axum에서 상태 코드를 지정하는 기본 방법입니다.
- 확장하면 `(StatusCode, HeaderMap, T)`까지 가능합니다.
- `serde_json::json!` 매크로는 임시 JSON을 만들 때 편리합니다. 실제 API에서는 구조체 + `Serialize`를 씁니다.
- 반환 타입을 `impl IntoResponse`로 적으면 "IntoResponse를 구현한 뭔가"라는 뜻이라 구체 타입을 안 써도 됩니다. 편하지만, **반환 지점이 여러 개이고 타입이 서로 다르면 컴파일 에러**가 납니다(같은 타입이어야 함). 그럴 땐 `Response`로 통일하거나 `Result<T, E>`를 씁니다.

### 5-3. 경로 파라미터 — `Path<T>`

```rust
// .route("/hello/{name}", get(hello))
async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}
```

- `Path(name): Path<String>`은 **구조 분해 패턴**입니다. `Path`는 튜플 구조체라서 인자 위치에서 바로 내용물을 꺼낼 수 있습니다. 이렇게 안 쓰면 함수 안에서 `path.0`으로 접근해야 합니다.
- 파라미터가 여러 개면 튜플로 받습니다.

```rust
// .route("/users/{user_id}/posts/{post_id}", get(h))
async fn h(Path((user_id, post_id)): Path<(i64, i64)>) { }

// 또는 구조체로 (필드 이름이 경로 이름과 일치해야 함)
#[derive(Deserialize)]
struct Params { user_id: i64, post_id: i64 }
async fn h2(Path(p): Path<Params>) { }
```

- **타입 변환도 해줍니다.** `Path<i64>`로 선언하면 `/hello/abc` 요청은 자동으로 `400 Bad Request`가 됩니다. FastAPI의 `id: int`와 같은 동작입니다.

### 5-4. 쿼리스트링 — `Query<T>`

```rust
#[derive(Debug, Deserialize)]
struct EchoParams {
    message: String,        // 필수
    times: Option<u8>,      // 선택
}

#[derive(Debug, Serialize)]
struct EchoResponse {
    original: String,
    repeated: Vec<String>,
}

async fn echo(Query(params): Query<EchoParams>) -> Json<EchoResponse> {
    let times = params.times.unwrap_or(1);
    let repeated = (0..times).map(|_| params.message.clone()).collect();

    Json(EchoResponse { original: params.message, repeated })
}
```

- `Query<T>`는 쿼리스트링을 `T`로 역직렬화합니다. `T`는 `Deserialize`를 구현해야 하므로 `#[derive(Deserialize)]`가 필요합니다.
- **`Option<T>` 필드 = 선택 파라미터**, 아니면 필수입니다. 없으면 400.
- `.clone()`이 필요한 이유: `params.message`를 `repeated` 벡터에 `times`번 넣어야 하는데, **소유권이 하나뿐**이라 그대로 옮기면 두 번째부터 쓸 수 없습니다. 그래서 복제합니다. 마지막에 `EchoResponse { original: params.message, ... }`에서는 더 이상 안 쓰므로 복제 없이 이동시킵니다. — **이게 Rust를 쓰면서 계속 하게 될 사고방식**입니다.

---

## 6. 실행 및 검증

```powershell
cargo run
```

```
   Compiling demo v0.1.0 (D:\vscode\2026_my_WebSite_study\backend_study\Auxm\demo)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.17s
     Running `target\debug\demo.exe`
listening on http://127.0.0.1:3000
```

> 첫 빌드는 의존성 52개를 전부 컴파일하느라 13초 정도 걸렸습니다. 이후 내 코드만 고치면 1~2초면 됩니다.

### 실제 요청/응답 결과

| 요청 | 응답 |
|---|---|
| `GET /` | `200` `Hello, Axum!` |
| `GET /health` | `200` `{"status":"ok"}` |
| `GET /hello/axum` | `200` `Hello, axum!` |
| `GET /echo?message=hi&times=3` | `200` `{"original":"hi","repeated":["hi","hi","hi"]}` |
| `GET /echo` (파라미터 누락) | `400` `Failed to deserialize query string: missing field 'message'` |
| `GET /nope` | `404` (본문 없음) |
| `POST /` | `405` Method Not Allowed |

**여기서 확인할 것 3가지:**

1. **`400`이 자동으로 나온다.** 내가 검증 코드를 한 줄도 안 썼는데, `Query<EchoParams>` extractor가 실패하면서 400을 돌려줬습니다. **extractor의 실패가 곧 오류 응답**입니다.
2. **`404`와 `405`를 axum이 구분한다.** 경로 자체가 없으면 404, 경로는 있는데 메서드가 없으면 405입니다. 직접 짜지 않아도 라우터가 처리해 줍니다.
3. **기본 오류 응답은 평문이다.** `400`의 본문이 JSON이 아니라 그냥 텍스트입니다. API 전체의 오류 형식을 통일하려면 직접 손봐야 하고, 그게 **study4의 주제**입니다.

---

## 7. 처음 만난 오류

### 7-1. `Path segments must not start with ':'` (런타임 패닉)

인터넷 예제를 그대로 따라 `:name`을 쓰면:

```rust
.route("/hello/:name", get(hello))
```

```
thread 'main' (26436) panicked at src\main.rs:29:10:
Path segments must not start with `:`. For capture groups, use `{capture}`.
If you meant to literally match a segment starting with a colon,
call `without_v07_checks` on the router.
```

- **axum 0.8에서 경로 문법이 `:id` → `{id}`로 바뀌었습니다.** 내부 라우팅 엔진(`matchit`)이 0.8로 올라가면서 생긴 변경입니다.
- 다행히 axum이 **0.7 문법을 감지해서 친절한 패닉 메시지**를 띄워줍니다. 컴파일은 통과하고 **실행할 때** 터진다는 점만 기억하면 됩니다.
- 교훈: **Axum은 마이너 버전마다 API가 꽤 바뀝니다.** 블로그 예제를 붙여넣기 전에 `Cargo.toml`의 버전을 먼저 확인하고, 의심스러우면 `cargo doc --open`으로 **내 버전의** 문서를 보세요.

### 7-2. `the trait bound ... Handler<_, _> is not satisfied` (컴파일 에러)

가장 자주 만나고, 가장 읽기 힘든 에러입니다. `get(handler)`에 넘긴 함수가 핸들러 조건을 못 맞췄다는 뜻인데, 실제 원인은 보통 이 넷 중 하나입니다.

| 원인 | 해결 |
|---|---|
| 반환 타입이 `IntoResponse`가 아님 | 반환 타입 확인. 커스텀 타입이면 `IntoResponse` 구현 |
| 본문 extractor(`Json`, `Form`, `String`)가 마지막 인자가 아님 | **마지막으로 옮긴다** |
| 인자 타입이 extractor가 아님 | `#[derive(Deserialize)]` 빠졌는지 확인 |
| `State<T>`를 썼는데 `.with_state()`를 안 붙임 | 라우터에 상태 주입 (study2) |

에러 메시지 아래쪽의 **`note:` 줄**에 "Consider using `#[axum::debug_handler]`"라는 힌트가 나올 때가 있습니다. 이 매크로(`axum`의 `macros` feature 필요)를 핸들러에 붙이면 **훨씬 구체적인 에러**를 보여줍니다.

```rust
#[axum::debug_handler]   // 원인을 정확히 짚어준다
async fn my_handler(...) -> impl IntoResponse { ... }
```

### 7-3. `target\debug\demo.exe`: Access is denied (os error 5)

서버를 백그라운드로 띄운 채 `cargo build`를 다시 돌리면 링크 단계에서 실패합니다. **Windows는 실행 중인 파일을 덮어쓰지 못합니다.** 재빌드 전에 서버 프로세스를 먼저 종료해야 합니다.

---

## 8. 이번 study 정리

- `cargo new` → `cargo add` → `cargo run`이 기본 사이클이고, 개발 중 검사는 `cargo check`가 빠르다.
- `#[tokio::main]`이 런타임을 띄우고, `TcpListener` + `axum::serve`가 서버를 연다. (`axum::Server`는 0.7에서 사라졌다)
- 라우터는 `Router::new().route(경로, 메서드(핸들러))`로 **값을 조립**한다. 애노테이션이 없다.
- 경로 파라미터는 **`{name}`** (0.8 문법).
- `Path<T>`, `Query<T>`가 파싱과 타입 변환, 그리고 **실패 시 400 응답까지** 처리한다.
- 반환 타입이 `IntoResponse`이면 뭐든 응답이 된다. 상태 코드는 튜플로 얹는다.

다음: [study2.md](study2.md) — 공유 상태(`Arc<RwLock>`)를 써서 in-memory 게시판 CRUD 만들기.
