# study0 — 환경 설정

> 🎯 **목표**: Rust 툴체인과 에디터를 갖추고, `cargo run` 한 번으로 Axum 서버가 응답하는 것을 확인한다. Git 저장소와 `.env`까지 준비해 다음 단계부터 바로 코드를 쓸 수 있는 상태로 만든다.

| 항목 | 내용 |
|---|---|
| 기준 버전 | rustc / cargo **1.98.1**, rustup 1.29.1, **axum 0.8.9**, tokio 1.5x, edition **2024** |
| 관련 코드 | 이 단계는 임시 프로젝트(`hello-axum`)로 진행한다. 완성본은 [lab/examples/s01_hello.rs](lab/examples/s01_hello.rs) |
| 선행 지식 | Rust 문법을 아주 조금(소유권, `Result`, `async`) 본 적이 있으면 충분하다 |

---

## ① 목표

- 설치 → 버전 확인 → 프로젝트 생성 → 서버 실행까지 **막힘 없이** 한 바퀴 돈다.
- 폴더 구조와 `Cargo.toml`의 의미를 설명할 수 있다.
- 파일을 고치면 서버가 알아서 다시 뜨는 개발 루프를 만든다.
- `.gitignore`와 `.env`로 "커밋하면 안 되는 것"을 처음부터 분리한다.

---

## ② 📖 이론

### 왜 Axum인가

**Axum**은 Rust의 비동기 런타임 `tokio` 팀이 만든 웹 프레임워크다. 직접 HTTP를 구현하지 않고, 검증된 부품을 얹어 만든다.

```
[ 내 코드: 핸들러 · 라우터 ]   ← axum   (요청을 함수 인자로 풀어 주고, 반환값을 응답으로 바꿔 준다)
[ 미들웨어: 로깅 · 압축 · 타임아웃 ] ← tower / tower-http
[ HTTP 프로토콜 ]              ← hyper
[ 비동기 실행 · 소켓 ]          ← tokio
```

- **빠르다**: 컴파일된 네이티브 바이너리이고 GC가 없다.
- **안전하다**: 컴파일러가 "데이터 경합", "해제된 메모리 접근", "처리 안 한 오류"를 빌드 단계에서 막는다.
- **대가**: 컴파일이 느리고(첫 빌드는 수 분), 문법과 소유권을 배워야 한다. 이 저장소의 학습 기록 절반이 "컴파일러가 알려 준 것"이다.

### 비유 — 식당으로 보는 구조

| 식당 | Axum |
|---|---|
| 손님이 주문서를 낸다 | HTTP 요청 |
| 홀 직원이 주문서를 분류해 주방 담당에게 전달한다 | **Router** (경로 + 메서드 → 핸들러) |
| 담당 요리사 | **핸들러** (`async fn`) |
| 주문서에서 "메뉴, 수량, 알레르기"를 뽑아 요리사에게 건넨다 | **Extractor** (`Path`, `Query`, `Json` …) |
| 접시에 담아 내보낸다 | **IntoResponse** (반환값 → 응답) |
| 입구에서 손님 확인, 영수증 기록, 포장 | **미들웨어** (tower Layer) |
| 주방 공용 냉장고 | **State** (모든 핸들러가 공유하는 데이터) |

### 핵심 용어 사전

| 용어 | 뜻 |
|---|---|
| **toolchain** | 컴파일러(`rustc`) + 패키지 관리자(`cargo`) + 도구 묶음. `rustup`이 설치·갱신한다 |
| **crate** | Rust의 패키지 단위. 라이브러리 크레이트와 실행 파일 크레이트가 있다 |
| **cargo** | 새 프로젝트 생성, 의존성 추가, 빌드, 실행, 테스트를 모두 하는 명령 |
| **edition** | 언어 문법 세대(2018/2021/2024). 프로젝트마다 `Cargo.toml`에 적는다 |
| **feature** | 크레이트 안의 선택 기능 스위치. 켜지 않으면 **코드 자체가 컴파일에서 빠진다** |
| **Cargo.lock** | 실제로 해결된 의존성 버전 목록. 실행 파일 프로젝트는 커밋한다 |
| **tokio runtime** | `async` 함수를 실제로 돌리는 실행기. `#[tokio::main]`이 시작해 준다 |
| **handler** | extractor를 인자로 받고 `IntoResponse`를 돌려주는 평범한 `async fn` |
| **Router** | 경로와 핸들러의 표. `Router::new().route("/", get(핸들러))`로 **값을 조립**한다 |
| **extractor** | 요청의 일부(경로, 쿼리, 본문, 헤더)를 꺼내 타입으로 바꿔 주는 인자 타입 |
| **tower `Service` / `Layer`** | 요청→응답 변환기 / 그것을 감싸는 데코레이터. 미들웨어의 실체 |

---

## ③ 🛠️ 실기

### 3-1. 설치

**Windows**

```powershell
winget install Rustlang.Rustup        # 또는 https://rustup.rs 의 설치 파일
```

Windows에서는 링커가 필요하다. **Visual Studio Build Tools**의 "**C++를 사용한 데스크톱 개발**" 워크로드(MSVC + Windows SDK)를 함께 설치한다. rustup 설치 중에 안내가 나온다.

**macOS / Linux**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

설치하면 `~/.cargo/bin`(Windows는 `%USERPROFILE%\.cargo\bin`)에 `cargo`, `rustc`, `rustup`이 생긴다. **새 터미널을 열어야** PATH에 반영된다.

### 3-2. 버전 확인

```powershell
rustc --version          # rustc 1.98.1 (48a229cea 2026-09-01)
cargo --version          # cargo 1.98.1 (797e8a9bc 2026-08-05)
rustup --version         # rustup 1.29.1
rustup show active-toolchain   # stable-x86_64-pc-windows-msvc (default)
rustfmt --version        # rustfmt 1.9.0-stable   ← 코드 포매터
cargo clippy --version   # clippy 0.1.98          ← 린터 (실수하기 쉬운 코드를 알려 준다)
```

갱신: `rustup update`. edition 2024는 비교적 새 도구가 필요하므로 오래된 설치라면 먼저 갱신한다.

### 3-3. 프로젝트 생성

```powershell
cargo new hello-axum
cd hello-axum
```

```
hello-axum/
├─ .git/            ← cargo가 git 저장소까지 만들어 준다
├─ .gitignore       ← 내용은 딱 한 줄: /target
├─ Cargo.toml       ← 프로젝트 설명서 (이름, 버전, 의존성)
└─ src/
   └─ main.rs       ← fn main() { println!("Hello, world!"); }
```

의존성은 손으로 적지 않고 `cargo add`로 넣는다.

```powershell
cargo add axum
cargo add tokio --features macros,rt-multi-thread,net
```

```toml
[package]
name = "hello-axum"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8.9"
tokio = { version = "1.54.1", features = ["macros", "rt-multi-thread", "net"] }
```

> ⚠️ `axum = "0.8.9"`은 "0.8.9 이상, 0.9 미만"이라는 뜻이다. **Axum은 마이너 버전마다 API가 바뀐다**(0.7에서 `Server` 제거, 0.8에서 경로 문법 변경). 인터넷 예제를 붙여넣기 전에 `Cargo.toml`의 버전부터 확인한다.

### 3-4. 첫 서버 — `src/main.rs`

```rust
use axum::{Router, routing::get};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/", get(|| async { "Hello, Axum!" }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

| 줄 | 설명 |
|---|---|
| `use axum::{Router, routing::get}` | 라우터 타입과 "GET 요청용" 핸들러 어댑터를 가져온다 |
| `#[tokio::main]` | `async fn main`을 tokio 런타임 위에서 실행되게 바꿔 주는 매크로. 이게 없으면 `async main`은 컴파일되지 않는다 |
| `Router::new().route("/", get(...))` | "`GET /`이 오면 이 핸들러를 실행"하는 표를 **값으로** 만든다. 애노테이션이 없다 |
| `\|\| async { "Hello, Axum!" }` | 인자 없는 async 클로저. `&'static str`은 그대로 응답이 된다(`IntoResponse` 구현) |
| `TcpListener::bind("127.0.0.1:3000")` | 포트를 연다. `127.0.0.1`은 **내 컴퓨터에서만** 접속된다 |
| `axum::serve(listener, app)` | 연결을 받아 `app`에 넘겨 준다. 0.7부터 `axum::Server`가 아니라 이 함수다 |
| `.unwrap()` | 실패하면 즉시 종료. 학습용 예제라서 허용하고, 실제 코드에서는 `?`와 `anyhow`를 쓴다 |

### 3-5. 개발 서버 실행

```powershell
cargo run
```

```
   Compiling hello-axum v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s)
     Running `target\debug\hello-axum.exe`
```

다른 터미널에서 확인:

```powershell
curl http://127.0.0.1:3000/        # Hello, Axum!
```

자주 쓰는 cargo 명령:

| 명령 | 용도 |
|---|---|
| `cargo check` | **실행 파일은 만들지 않고** 타입 검사만. 가장 빠른 피드백 |
| `cargo build` / `cargo build --release` | 디버그 / 최적화 빌드 (`target/debug`, `target/release`) |
| `cargo run` | 빌드 후 실행 |
| `cargo test` | 테스트 실행 (study9) |
| `cargo fmt` | 포맷 정리 |
| `cargo clippy` | 린트 |
| `cargo add <크레이트>` | 의존성 추가 |
| `cargo tree` | 의존성 트리 |
| `cargo doc --open` | **내 버전의** 문서를 브라우저로 열기 |

### 3-6. 파일을 고치면 자동으로 다시 시작 — `cargo-watch`

Rust는 "저장하면 바로 반영"이 기본이 아니다. 도구를 하나 설치한다.

```powershell
cargo install cargo-watch        # 한 번만 (몇 분 걸린다)
cargo watch -q -x run            # 소스가 바뀌면 cargo run을 다시 실행
```

직접 확인한 결과: `s01_hello` 예제를 `cargo watch -q -x "run --example s01_hello"`로 띄운 채 응답 문자열을 바꿨더니, 저장 후 다시 빌드·재시작되어 **`Hello, Axum!` → `Hello, Axum! (reloaded)`** 로 바뀌었다. (재시작이므로 메모리에 있던 상태는 사라진다.)

### 3-7. 에디터 확장 프로그램 (VS Code)

이 저장소는 [.vscode/extensions.json](.vscode/extensions.json)에 추천 목록을 넣어 두었다. 폴더를 열면 설치 안내가 뜬다.

| 확장 | 하는 일 |
|---|---|
| **rust-analyzer** (`rust-lang.rust-analyzer`) | 자동완성, 타입 힌트, 정의로 이동, 저장 시 오류 표시. **필수** |
| **Even Better TOML** (`tamasfe.even-better-toml`) | `Cargo.toml` 문법 강조·검사 |
| **CodeLLDB** (`vadimcn.vscode-lldb`) | 중단점 디버깅 (study9) |

rust-analyzer의 힌트(변수 옆에 회색으로 보이는 추론 타입)는 학습에 큰 도움이 된다. 타입이 헷갈릴 때 가장 먼저 볼 곳이다.

---

## ④ 🔗 연동 포인트

### Git 저장소 초기화와 `.gitignore`

`cargo new`가 이미 `git init`까지 해 준다. 기존 폴더에 직접 만들 때는:

```powershell
git init
```

Rust 프로젝트에서 **절대 커밋하면 안 되는 것**을 `.gitignore`에 처음부터 넣는다. 이 저장소의 [.gitignore](.gitignore):

```gitignore
# Rust 빌드 산출물 — 수 GB가 될 수 있다
**/target/

# 환경변수 파일 (비밀번호·키가 들어간다). 예시 파일만 커밋한다
**/.env
**/.env.*
!**/.env.example

# SQLite 파일 (study6~)
**/*.db
**/*.db-shm
**/*.db-wal

# 업로드된 파일 (폴더 구조만 유지)
**/uploads/*
!**/uploads/.gitkeep
```

확인:

```powershell
git status          # target/ 이 목록에 나오지 않아야 한다
git check-ignore -v target/debug/foo   # 어느 규칙이 무시하는지 보여 준다
```

### `.env` — 환경변수 파일

설정(포트, DB 주소, 비밀키)은 **코드에 쓰지 않고** 환경변수로 뺀다. 개발할 때는 `.env` 파일에 적어 두고 `dotenvy` 크레이트로 읽는다.

1. 예시 파일 [demo/.env.example](demo/.env.example)을 **커밋**한다 (값은 가짜, 키 이름 목록 역할).
2. 각자 `.env`로 복사해 값을 채운다 (**커밋하지 않는다**).

```powershell
copy .env.example .env
```

```dotenv
DATABASE_URL=sqlite://data.db?mode=rwc
JWT_SECRET=dev-only-insecure-secret-change-me
BIND_ADDR=127.0.0.1:3000
RUST_LOG=demo=debug,tower_http=debug
```

읽는 코드(이 저장소 [demo/src/config.rs](demo/src/config.rs)의 핵심):

```rust
pub fn load_dotenv() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => {}
        Err(e) if e.not_found() => {}          // .env가 없어도 에러가 아니다 (컨테이너에서는 보통 없다)
        Err(e) => return Err(e).context(".env 파일을 읽지 못했습니다"),
    }
    Ok(())
}
```

- 우선순위: **실제 환경변수 > `.env` > 코드 기본값**. `dotenvy`는 이미 설정된 값을 덮어쓰지 않는다.
- ⚠️ **`.env`는 `tracing` 초기화보다 먼저 읽어야 한다.** 순서가 반대면 `.env`의 `RUST_LOG`가 조용히 무시된다 (아래 오류 노트).

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| `'cargo' is not recognized` / `command not found` | 설치 직후 터미널에 PATH가 반영되지 않음 | 새 터미널을 연다. 그래도 안 되면 PATH에 `%USERPROFILE%\.cargo\bin` 추가 (PowerShell: `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"`) |
| `linker 'link.exe' not found` (Windows) | MSVC 빌드 도구·Windows SDK 없음 | Visual Studio Build Tools에서 "C++를 사용한 데스크톱 개발" 설치 |
| 첫 `cargo build`가 몇 분째 안 끝남 | 의존성을 전부 소스에서 컴파일하는 중 (정상) | 기다린다. 이후에는 바뀐 부분만 다시 컴파일한다. 터미널을 막고 싶지 않으면 백그라운드로 실행 |
| `error: Access is denied. (os error 5)` — `target\debug\*.exe` 링크 단계 (Windows) | 서버가 실행 중인 채로 다시 빌드. Windows는 실행 중인 파일을 덮어쓰지 못함 | 서버 프로세스를 먼저 종료하고 빌드. `cargo watch`는 이걸 알아서 처리한다 |
| `Address already in use` (os error 10048) | 같은 포트를 쓰는 프로세스가 이미 있음 (이전에 띄운 서버 등) | `netstat -ano \| findstr :3000`으로 PID 확인 후 종료, 또는 `BIND_ADDR`의 포트 변경 |
| `.env`에 `RUST_LOG`를 적었는데 로그 레벨이 안 바뀜 | `EnvFilter`는 초기화 시점의 환경변수를 **한 번만** 읽는데, `.env`를 그 뒤에 읽음. **에러가 안 나고 조용히 동작만 달라진다** | `config::load_dotenv()`를 `init_tracing()`보다 **먼저** 호출 |
| `.env`의 값을 바꿨는데 반영 안 됨 | 실제 환경변수가 `.env`보다 우선 | 셸에 남아 있는 환경변수를 확인(`$env:이름`)하고 지운다 |
| `target/`이 `git status`에 잔뜩 뜸 | `.gitignore`에 `target`이 없거나 이미 커밋됨 | `.gitignore`에 추가하고 `git rm -r --cached target` |

---

## ⑥ 직접 해보기

1. **`hello-axum`을 처음부터 만들어 보기**: `cargo new` → `cargo add` → `main.rs` 작성 → `curl`로 확인. `cargo run`, `cargo check`, `cargo build --release`의 걸린 시간을 각각 적어 비교한다.
2. **응답 바꾸고 자동 재시작 확인**: `cargo install cargo-watch` 후 `cargo watch -q -x run`을 켜 둔 채 문자열을 바꿔 저장한다. 브라우저를 새로고침했을 때 바뀌는지 본다.
3. **`.env` 흐름 만들기**: `.env.example`을 만들고 `BIND_ADDR=127.0.0.1:3100`을 적은 `.env`를 만든 뒤, `dotenvy`로 읽어 그 포트에서 서버가 뜨게 한다. `git status`에 `.env`가 보이지 않는지 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ `rustc --version`, `cargo --version`이 출력된다
- ☐ `cargo new` → `cargo add axum` → `cargo run`으로 `curl`에 `Hello, Axum!`이 돌아온다
- ☐ `Cargo.toml`의 `edition`, `[dependencies]`, `features`가 각각 무엇인지 설명할 수 있다
- ☐ `cargo check`와 `cargo build`의 차이를 안다
- ☐ `cargo watch -q -x run`으로 저장 시 자동 재시작을 확인했다
- ☐ rust-analyzer가 동작한다 (변수에 마우스를 올리면 타입이 보인다)
- ☐ `.gitignore`에 `target/`, `.env`, `*.db`가 들어 있고 `git status`가 깨끗하다
- ☐ `.env.example`은 커밋하고 `.env`는 커밋하지 않는 이유를 설명할 수 있다
- ☐ `.env`를 `tracing` 초기화보다 먼저 읽어야 하는 이유를 설명할 수 있다

---

## ⑧ 다음 단계 연결

서버가 뜬다는 것을 확인했으니, [study1](study1.md)에서는 `"Hello, Axum!"` 한 줄에서 출발해 경로 파라미터·쿼리·JSON·HTML까지 **첫 응답을 다양한 모양으로** 만들어 본다.
