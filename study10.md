# study10 — 배포 · 기초 구간 정리

> 🎯 **목표**: 릴리스 빌드와 **멀티 스테이지 Docker 이미지**로 서버를 패키징하고, 도메인·HTTPS의 개념을 이해하며, 배포 환경에서 환경변수·DB를 잇는 방법과 **배포 후 점검표**를 갖춘다. 마지막에 기초 구간(study0~10) 전체를 정리한다.

| 항목 | 내용 |
|---|---|
| 관련 파일 | 단계 예제 [lab/examples/s10_deploy.rs](lab/examples/s10_deploy.rs)(포트 3010), 실제 프로젝트: [demo/Dockerfile](demo/Dockerfile), [demo/.dockerignore](demo/.dockerignore), [demo/compose.yaml](demo/compose.yaml), [demo/compose.postgres.yaml](demo/compose.postgres.yaml), [demo/Cargo.toml](demo/Cargo.toml) `[profile.release]`, [demo/src/main.rs](demo/src/main.rs) (`shutdown_signal`) |
| **검증** | Docker 28.3 — 이미지를 **실제로 빌드하고 컨테이너로 실행**해 헬스체크·영속성·종료 동작을 확인했다 (아래 측정값은 모두 실측) |
| 이 단계에서 고친 것 | ① `utoipa-swagger-ui`에 `vendored` feature (Docker 빌드가 **빌드 중 네트워크·curl에 의존**해서 실패) ② `docker stop`이 SIGTERM을 못 받아 **10초 뒤 강제 종료**되던 것 (`shutdown_signal`) ③ 이미지 빌드 인자 `CARGO_FLAGS` 추가 ④ 시작 로그에 **DB 비밀번호가 평문으로** 찍히던 것 (`redacted_database_url`) |

---

## ① 목표

1. `cargo build --release`의 의미와 `[profile.release]` 설정을 이해한다.
2. 멀티 스테이지 Dockerfile(의존성 레이어 캐싱 포함)로 이미지를 만든다.
3. 컨테이너에서 설정(환경변수)·데이터(볼륨)·비밀(주입)을 다루는 방법을 안다.
4. 도메인·DNS·HTTPS가 서버 앞에서 어떻게 이어지는지 설명할 수 있다.
5. 배포 직후 확인할 점검표를 갖는다.

---

## ② 📖 이론

### 배포 = "내 컴퓨터에서 돌던 것"을 "다른 컴퓨터에서 똑같이" 돌리기

Rust는 결과물이 **바이너리 하나**라서 배포가 단순한 편이다. 그래도 컨테이너를 쓰는 이유는 **실행 환경 전체**(OS 라이브러리, 비root 사용자, 환경변수, 헬스체크, 볼륨)를 한 단위로 묶기 위해서다.

### 멀티 스테이지 빌드

```
[빌더 스테이지]  rust:1.98-slim  ← 컴파일러·의존성 소스·중간 산출물 (수 GB)
        │ 바이너리 하나(19MB)만 복사
        ▼
[런타임 스테이지] debian:bookworm-slim + ca-certificates + curl  ← 컴파일러 없음. 최종 이미지 160MB
```

컴파일에 필요한 것과 실행에 필요한 것은 다르다. 빌더는 버리고 **바이너리만** 런타임에 옮긴다.

### 의존성 레이어 캐싱 트릭 (Rust에서 특히 중요)

Docker는 줄(레이어)마다 결과를 캐시하고, **그 줄의 입력이 바뀌지 않으면 재사용**한다. Rust는 컴파일이 느려서 소스 한 줄을 고칠 때마다 의존성 수백 개를 다시 컴파일하면 곤란하다.

```dockerfile
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release && rm -rf src     # ① 가짜 소스로 의존성만 컴파일 → 레이어로 캐시
COPY src ./src                                 # ② 진짜 소스 (자주 바뀐다)
RUN touch src/main.rs src/lib.rs && cargo build --release   # ③ 내 코드만 컴파일
```

실측: 첫 빌드에서 의존성 컴파일(①) **58초** + 내 코드 컴파일(③) **23초**. 이후 소스만 고쳐 다시 빌드하면 ①이 `CACHED`로 건너뛰어져 ③만 다시 컴파일한다.

### 도메인 · DNS · HTTPS

```
브라우저 ── https://board.example.com ──▶ [DNS] 도메인 → 서버 IP 를 알려 준다 (A/AAAA 레코드)
                                       ▼
                               [리버스 프록시: Caddy / nginx / 클라우드 로드밸런서]
                                  · HTTPS(TLS) 종료: 인증서로 암호화를 풀어 준다
                                  · 80 → 443 리다이렉트
                                       │  http (내부망)
                                       ▼
                               [Axum 컨테이너 :3000]
```

| 개념 | 설명 |
|---|---|
| **도메인** | 사람이 읽는 이름. 구매 후 DNS에서 서버 IP로 연결 |
| **DNS A 레코드** | `board.example.com → 203.0.113.10` |
| **HTTPS / TLS** | 통신 암호화 + 서버 신원 증명. 비밀번호·토큰·쿠키는 HTTPS 없이 보내면 안 된다 |
| **인증서** | Let's Encrypt가 **무료**로 발급·자동 갱신 (Caddy는 도메인만 적으면 알아서 발급) |
| **리버스 프록시** | 앞에서 요청을 받아 뒤의 앱으로 전달. HTTPS 종료, 압축, 정적 파일, 속도 제한을 맡길 수 있다 |
| **`Secure` 쿠키** | HTTPS에서만 쿠키 전송. 프록시 뒤에서는 앱이 `http`로 보이므로 **환경변수로 켠다**(`SECURE_COOKIE=true`) |

Axum 앱이 직접 TLS를 처리할 수도 있지만(`axum-server` + rustls), **프록시에서 종료하는 구성이 운영·갱신이 쉬워서** 일반적이다. 앱 앞 프록시 설정 예(Caddyfile):

```
board.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

한 줄이다. 인증서 발급·갱신·HTTP→HTTPS 리다이렉트를 Caddy가 한다.

---

## ③ 🛠️ 실기

### 3-0. 단계 예제 — [lab/examples/s10_deploy.rs](lab/examples/s10_deploy.rs) (포트 3010)

컨테이너·서버에서 **실제로 문제가 되는 것만** 담았다. `BIND_ADDR`, 시작 시 설정 검증, 로그에 DB 비밀번호를 찍지 않기, 그리고 **정상 종료**(Ctrl+C와 SIGTERM을 모두 받고 진행 중인 요청은 마친 뒤 종료). Dockerfile·compose는 아래 3-1부터의 `demo/`다.

```powershell
cd lab
cargo run --example s10_deploy
$env:BIND_ADDR = "0.0.0.0:3010"; $env:JWT_SECRET = "길고-무작위인-비밀키-32자-이상"
$env:DATABASE_URL = "postgres://board:secret@db:5432/board"; cargo run --example s10_deploy
$env:SHUTDOWN_AFTER_MS = "1500"; cargo run --example s10_deploy     # 1.5초 뒤 스스로 종료 신호 (시험용)
cargo test --example s10_deploy                                     # 시험 3개
```

```rust
//! study10 — 배포 · 정리. 포트 3010.
//!
//! 컨테이너·서버에서 실제로 문제가 되는 것만 담았다:
//!   ① `BIND_ADDR` — 컨테이너 안에서는 0.0.0.0이어야 밖에서 접속된다
//!   ② 시작 시 설정 검증 — 짧은/기본 비밀키는 부팅 거부·경고
//!   ③ 시작 로그에 **DB 비밀번호를 찍지 않는다** (`redact_url_password`)
//!   ④ **정상 종료** — Ctrl+C(SIGINT)와 **SIGTERM**(docker stop)을 모두 받고, 진행 중인 요청은 마친 뒤 종료
//! (Dockerfile·compose는 demo/Dockerfile, demo/compose.yaml)
//!
//! 실행:  cargo run --example s10_deploy
//!        BIND_ADDR=0.0.0.0:3010 JWT_SECRET="길고-무작위인-비밀키-32자-이상" DATABASE_URL="postgres://u:secret@db:5432/board" cargo run --example s10_deploy
//!        SHUTDOWN_AFTER_MS=500 cargo run --example s10_deploy        # 0.5초 뒤 스스로 종료 신호를 보낸다 (시험용)

use std::{future::Future, time::Duration};

use axum::{Json, Router, extract::Query, routing::get};
use serde::Deserialize;
use serde_json::json;

const DEV_JWT_SECRET: &str = "dev-only-insecure-secret-change-me";

#[derive(Debug, Clone)]
struct Config {
    bind_addr: String,
    jwt_secret: String,
    database_url: String,
}

impl Config {
    fn from_env() -> anyhow::Result<Self> {
        let config = Self {
            bind_addr: env_or("BIND_ADDR", "127.0.0.1:3010"), // 로컬 기본은 내 컴퓨터만. 컨테이너는 0.0.0.0:포트
            jwt_secret: env_or("JWT_SECRET", DEV_JWT_SECRET),
            database_url: env_or("DATABASE_URL", "sqlite://data.db?mode=rwc"),
        };
        if config.jwt_secret.len() < 16 {
            anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)"); // 안전한 쪽으로 실패한다
        }
        if config.jwt_secret == DEV_JWT_SECRET {
            eprintln!("WARN 기본 JWT_SECRET을 쓰고 있습니다. 운영 환경에서는 반드시 JWT_SECRET을 설정하세요.");
        }
        Ok(config)
    }

    /// 로그에 찍어도 되는 DB 주소. 비밀번호를 가린다.
    fn redacted_database_url(&self) -> String {
        redact_url_password(&self.database_url)
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// `scheme://user:password@host/...` 의 password를 `***`로 바꾼다. 비밀번호가 없으면 그대로 둔다.
fn redact_url_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else { return url.to_string() };
    let (scheme, rest) = url.split_at(scheme_end + 3);
    // 사용자 정보는 첫 '/' 앞에 있고 마지막 '@'로 끝난다 (비밀번호에 '@'가 들어 있을 수 있다)
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let Some(at) = rest[..authority_end].rfind('@') else { return url.to_string() };
    match rest[..at].split_once(':') {
        Some((user, _password)) => format!("{scheme}{user}:***{}", &rest[at..]),
        None => url.to_string(),
    }
}

// ── 핸들러 ─────────────────────────────────────────────────────────────────
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" })) // HEALTHCHECK가 찌르는 곳
}

#[derive(Deserialize)]
struct SlowQuery {
    secs: Option<u64>,
}

/// 오래 걸리는 요청. 종료 신호가 와도 **이 요청은 끝까지 응답을 받아야** 한다.
async fn slow(Query(q): Query<SlowQuery>) -> Json<serde_json::Value> {
    let secs = q.secs.unwrap_or(3).min(30);
    tokio::time::sleep(Duration::from_secs(secs)).await;
    Json(json!({ "finished": true, "tookSecs": secs }))
}

fn app() -> Router {
    Router::new().route("/health", get(health)).route("/slow", get(slow))
}

// ── 정상 종료 ──────────────────────────────────────────────────────────────
/// 종료 신호를 기다린다. Ctrl+C(SIGINT)와 SIGTERM을 **둘 다** 받는다.
///
/// SIGTERM을 처리하지 않으면 컨테이너의 PID 1은 신호를 무시해서 `docker stop`이 10초를 기다린 뒤
/// SIGKILL로 죽인다. 진행 중이던 요청이 그대로 끊긴다.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("Ctrl+C 신호를 기다리지 못했습니다: {e}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                eprintln!("SIGTERM 핸들러를 등록하지 못했습니다: {e}");
                std::future::pending::<()>().await;
            }
        }
    };
    // Windows에는 SIGTERM이 없다. 영원히 끝나지 않는 future로 채워 둔다.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // 시험용: 환경변수가 있으면 그 시간 뒤에 스스로 종료 신호를 낸다. 실제 신호 없이도 종료 경로를 시험할 수 있다.
    let self_trigger = async {
        match std::env::var("SHUTDOWN_AFTER_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
            Some(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
            None => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = ctrl_c => println!("종료 신호(Ctrl+C)를 받았습니다"),
        _ = terminate => println!("종료 신호(SIGTERM)를 받았습니다"),
        _ = self_trigger => println!("종료 신호(SHUTDOWN_AFTER_MS)를 받았습니다"),
    }
}

/// main과 시험이 함께 쓰는 서버 실행기. 신호 future가 끝나면 **새 연결을 받지 않고 진행 중인 요청을 마친 뒤** 돌아온다.
async fn serve(listener: tokio::net::TcpListener, shutdown: impl Future<Output = ()> + Send + 'static) -> std::io::Result<()> {
    axum::serve(listener, app()).with_graceful_shutdown(shutdown).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    println!("s10_deploy  database_url={}  (비밀번호는 가려서 로그에)", config.redacted_database_url());
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    println!("listening on {}", listener.local_addr()?);
    serve(listener, shutdown_signal()).await?;
    println!("서버를 종료했습니다");
    Ok(())
}
```

시험 코드는 같은 파일 아래쪽의 `#[cfg(test)] mod tests`에 있다: [s10_deploy.rs](lab/examples/s10_deploy.rs).

| 코드 | 설명 |
|---|---|
| `BIND_ADDR` 기본 `127.0.0.1:3010` | 로컬 기본은 내 컴퓨터만. **컨테이너 안에서 127.0.0.1이면 밖에서 접속할 수 없다** → 컨테이너는 `0.0.0.0:포트` |
| `JWT_SECRET.len() < 16` → `bail!` | 짧은 비밀키는 **부팅 거부**. 기본 비밀키는 경고만. 운영용 기본값을 코드에 두지 않는다 |
| `redact_url_password` | `postgres://u:비밀번호@host`의 비밀번호를 `***`로. 시작 로그에 `DATABASE_URL`을 그대로 찍으면 비밀번호가 로그 수집 시스템에 남는다. 비밀번호에 `@`가 들어 있어도 **마지막 `@`** 를 기준으로 가린다 |
| `/health` | `HEALTHCHECK`가 찌르는 곳 |
| `/slow?secs=3` | 일부러 오래 걸리는 요청 — 종료 신호가 와도 **이 요청은 끝까지 응답을 받아야** 한다 |
| `ctrl_c` + `terminate`(SIGTERM) | **SIGTERM을 처리하지 않으면** 컨테이너의 PID 1은 신호를 무시해서 `docker stop`이 10초를 기다린 뒤 SIGKILL로 죽인다. 진행 중이던 요청이 그대로 끊긴다 |
| `#[cfg(unix)]` / `#[cfg(not(unix))]` | Windows에는 SIGTERM이 없다. 영원히 끝나지 않는 future(`pending`)로 채워 어느 OS에서나 같은 `select!`가 컴파일된다 |
| `SHUTDOWN_AFTER_MS` | 시험용: 그 시간 뒤에 스스로 종료 신호를 낸다. 실제 신호를 보내기 어려운 환경에서도 종료 경로를 시험할 수 있다 |
| `with_graceful_shutdown(future)` | future가 끝나면 **새 연결을 받지 않고 진행 중인 요청을 마친 뒤** `serve`가 돌아온다 |
| `serve(listener, shutdown)` | main과 시험이 함께 쓰는 실행기. 신호 future를 **인자로** 받으니 시험에서 채널로 대체할 수 있다 |

**실행 결과** (직접 실행한 결과):

```
$ JWT_SECRET=short cargo run --example s10_deploy
Error: JWT_SECRET이 너무 짧습니다 (최소 16자)

$ BIND_ADDR=127.0.0.1:3010 JWT_SECRET=<긴 값> DATABASE_URL=postgres://board:s3cret@db:5432/board SHUTDOWN_AFTER_MS=1500 cargo run --example s10_deploy
s10_deploy  database_url=postgres://board:***@db:5432/board  (비밀번호는 가려서 로그에)       ← 비밀번호 마스킹
listening on 127.0.0.1:3010
   (그 사이에 curl ".../slow?secs=3"  → {"finished":true,"tookSecs":3}  [200]  3.3초 만에 응답)   ← 종료 신호(1.5초)가 왔는데도 끊기지 않았다
종료 신호(SHUTDOWN_AFTER_MS)를 받았습니다
서버를 종료했습니다
$ curl localhost:3010/health   → 연결 거부 (종료된 서버는 새 연결을 받지 않는다)
```

시험 (`cargo test --example s10_deploy`, 1초짜리 요청을 보내 두고 처리 도중에 종료 신호를 보내는 시험이라 3초 걸린다):

```
test tests::비밀번호만_가린다 ... ok
test tests::비밀번호가_없으면_그대로_둔다 ... ok
test tests::종료_신호가_와도_진행_중인_요청은_끝까지_응답한다 ... ok     ← 응답 수신 → 서버 종료 → 새 연결 거절까지 확인
test result: ok. 3 passed; finished in 3.05s
```

> Windows에서는 SIGTERM을 보낼 수 없어서 이 예제의 **SIGTERM 경로 자체**는 위 시험(채널)과 `SHUTDOWN_AFTER_MS`로 종료 절차를 확인했다. 실제 SIGTERM은 같은 코드를 쓰는 `demo`·`project` 컨테이너에서 `docker stop`이 **0.4초**(수정 전 10.4초)임을 확인한 것이 근거다 (아래 3-4).

아래 3-1부터는 이 개념이 실제 프로젝트(`demo/`)에서 어떤 모양이 되는지 이어서 본다.

### 3-1. 릴리스 빌드 — [demo/Cargo.toml](demo/Cargo.toml)

```toml
[profile.release]
opt-level = 3
lto = "thin"        # 크레이트 경계를 넘는 인라이닝
codegen-units = 1   # 병렬 컴파일을 포기하고 최적화 기회를 늘린다
strip = true        # 심볼 제거 (백트레이스는 잃는다)
# panic = "abort"   # ⚠️ 절대 켜지 말 것: CatchPanicLayer가 동작하지 않게 된다
```

| 빌드 (이전 측정) | 시간 | 바이너리 |
|---|---|---|
| `debug` (증분) | 수 초 | 16.3 MB |
| `release` 기본 | 57.9s | 19.6 MB |
| `release` 위 설정 | **89.4s** | **18.5 MB** |

시간은 1.5배 늘고 크기는 1.1MB만 줄었다. **크게 안 줄어든 이유**는 Swagger UI 정적 자산이 바이너리에 박혀 있기 때문이다(`utoipa-swagger-ui`를 빼면 훨씬 작아진다). 이 설정의 이득은 크기보다 **실행 속도**다.

### 3-2. Dockerfile — [demo/Dockerfile](demo/Dockerfile)

```dockerfile
FROM rust:1.98-slim-bookworm AS builder
WORKDIR /app

ARG CARGO_FLAGS=""        # 예: --no-default-features --features postgres

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release ${CARGO_FLAGS} \
    && rm -rf src

COPY src ./src
COPY migrations ./migrations
COPY migrations_pg ./migrations_pg
RUN touch src/main.rs src/lib.rs \
    && cargo build --release ${CARGO_FLAGS}

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /usr/sbin/nologin appuser
WORKDIR /app

COPY --from=builder /app/target/release/demo /usr/local/bin/demo

RUN mkdir -p /app/data /app/uploads && chown -R appuser:appuser /app
VOLUME ["/app/data", "/app/uploads"]
USER appuser

ENV DATABASE_URL="sqlite:///app/data/data.db?mode=rwc" \
    UPLOAD_DIR="/app/uploads" \
    BIND_ADDR="0.0.0.0:3000" \
    RUST_LOG="demo=info,tower_http=info"
# ⚠️ JWT_SECRET은 이미지에 굽지 않는다. 실행할 때 -e 로 주입한다.

EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:3000/health || exit 1

CMD ["demo"]
```

| 코드 | 설명 |
|---|---|
| `FROM ... AS builder` / `AS runtime` | 두 스테이지. 최종 이미지는 마지막 스테이지만 남는다 |
| `ARG CARGO_FLAGS` | 빌드 시 인자로 feature를 고른다. **기본은 SQLite**, PostgreSQL 이미지는 `--build-arg CARGO_FLAGS="--no-default-features --features postgres"` |
| 가짜 `main.rs`/`lib.rs`로 먼저 빌드 | 의존성 컴파일을 캐시 레이어에 가둔다 (위 ② 참고) |
| `touch src/main.rs src/lib.rs` | 가짜 빌드의 산출물보다 **소스가 새 것**으로 보이게 해서 다시 컴파일되게 한다. 빠뜨리면 가짜 바이너리가 그대로 나가는 사고가 난다 |
| `migrations`는 빌더에만 | `sqlx::migrate!`가 SQL을 **컴파일 시점에 바이너리에 박아 넣어서** 런타임 이미지에는 `migrations/`가 필요 없다 |
| `ca-certificates`, `curl` | 외부 HTTPS 호출용 / `HEALTHCHECK`에서 `/health`를 찌르기 위해 |
| `useradd appuser` + `USER appuser` | **root로 실행하지 않는다.** 컨테이너가 뚫려도 피해를 줄인다 (실측: `uid=1000(appuser)`) |
| `VOLUME [...]` | DB·업로드 파일이 사는 곳. 볼륨으로 마운트해야 컨테이너를 지워도 데이터가 산다 |
| `ENV BIND_ADDR="0.0.0.0:3000"` | **컨테이너 안에서는 `0.0.0.0`**. `127.0.0.1`이면 컨테이너 밖에서 접속할 수 없다 |
| `JWT_SECRET` 없음 | 이미지에 구운 비밀은 이미지를 가진 모두가 알게 된다. 실행 시 주입 |
| `HEALTHCHECK` | 오케스트레이터가 "살아 있는가"를 판단하는 기준 |

### 3-3. `.dockerignore` — [demo/.dockerignore](demo/.dockerignore)

```
target/          ← 이 한 줄이 가장 중요하다. 수 GB의 빌드 산출물을 빌드 컨텍스트로 보내지 않는다
*.db  *.db-shm  *.db-wal   uploads/   .env   .git/   *.md
```

`target/`을 빼먹으면 `docker build`가 시작도 못 하고 컨텍스트 전송에만 몇 분을 쓴다. `.env`가 이미지에 들어가면 **비밀이 이미지에 구워진다.**

### 3-4. ⭐ 종료 신호 — [demo/src/main.rs](demo/src/main.rs)

처음 Docker로 돌려 보니 **`docker stop`이 10.4초**나 걸렸고 종료 로그도 없었다. 앱이 `Ctrl+C`(SIGINT)만 기다리고, Docker가 보내는 **SIGTERM**은 처리하지 않았기 때문이다. 컨테이너의 PID 1은 핸들러가 없는 신호를 무시하므로 Docker가 10초 기다린 뒤 SIGKILL로 죽인다. 진행 중이던 요청이 그대로 끊긴다.

```rust
async fn shutdown_signal() {
    let ctrl_c = async { /* tokio::signal::ctrl_c() */ };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => { sigterm.recv().await; }
            Err(e) => { tracing::error!(%e, "SIGTERM 핸들러를 등록하지 못했습니다"); std::future::pending::<()>().await; }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();          // Windows에는 SIGTERM이 없다

    tokio::select! {
        _ = ctrl_c => tracing::info!("종료 신호(Ctrl+C)를 받았습니다"),
        _ = terminate => tracing::info!("종료 신호(SIGTERM)를 받았습니다"),
    }
}
```

`axum::serve(...).with_graceful_shutdown(shutdown_signal())`가 이 future가 끝나면 **새 연결을 받지 않고 진행 중인 요청을 마친 뒤** 종료한다. 수정 후 실측: `docker stop` **0.4초**, 로그에 `종료 신호(SIGTERM)를 받았습니다` → `서버를 종료했습니다`.

### 3-5. 빌드 · 실행 · 확인 (실측)

```powershell
cd demo
docker build -t axum-demo:study .
docker images axum-demo:study                 # axum-demo:study   160MB  (바이너리 19MB + debian-slim + curl)
```

```powershell
# 비밀은 실행 시 주입, 데이터는 볼륨
docker run -d --name axum-demo -p 3200:3000 `
  -e JWT_SECRET="$(python -c 'import secrets;print(secrets.token_urlsafe(48))')" `
  -v axumstudy-data:/app/data axum-demo:study
```

```
$ curl http://127.0.0.1:3200/health
{"status":"ok"}

$ docker inspect -f '{{.State.Health.Status}}' axum-demo
starting  →  (약 30초 뒤)  healthy

$ docker exec axum-demo id
uid=1000(appuser) gid=1000(appuser) groups=1000(appuser)

# 글을 하나 만들고 → 컨테이너를 지우고 → 같은 볼륨으로 새 컨테이너를 띄운다
$ curl "http://127.0.0.1:3200/api/posts"
posts after recreate: 1 ['in container']                  ← 데이터가 볼륨에 남아 있다 (data.meta.total = 1)

# 비밀키가 짧으면 부팅을 거부한다
$ docker run --rm -e JWT_SECRET=short axum-demo:study
Error: JWT_SECRET이 너무 짧습니다 (최소 16자)
```

> `docker compose up -d --build`(`compose.yaml`)는 위 설정을 파일로 적어 둔 것이다. `JWT_SECRET: ${JWT_SECRET:?JWT_SECRET 환경변수를 설정하세요}` — **설정이 없으면 컨테이너 시작 자체를 거부**한다. `api-data`, `api-uploads` 볼륨이 데이터를 지킨다.

---

## ④ 🔗 연동 포인트 — 배포 환경에서 환경변수·DB를 잇는 방법

### 설정이 흘러가는 길

```
[CI/서버의 시크릿 저장소] ──주입──▶ docker run -e / compose environment / 오케스트레이터 Secret
                                           │
                       컨테이너 환경변수 (DATABASE_URL, JWT_SECRET, BIND_ADDR, UPLOAD_DIR, RUST_LOG)
                                           │ Config::from_env()  — 시작 시 검증
                                           ▼
                                      AppState (DB 풀, 비밀키)
```

같은 이미지가 개발·스테이징·운영에 그대로 가고, **달라지는 것은 주입하는 값뿐**이다.

### 배포 환경에서 DB를 잇는 세 가지

| 방식 | 구성 | 언제 |
|---|---|---|
| **① 볼륨 위 SQLite** | `-v data:/app/data`, `DATABASE_URL=sqlite:///app/data/data.db?mode=rwc` | 작은 서비스, 서버 1대. **인스턴스를 둘 이상 띄우면 안 된다** |
| **② 같은 compose의 PostgreSQL 컨테이너** | 앱 이미지를 `--build-arg CARGO_FLAGS="--no-default-features --features postgres"`로 빌드, `DATABASE_URL=postgres://postgres:…@db:5432/board` (서비스 이름 `db`가 호스트명) | 한 서버에서 앱+DB |
| **③ 관리형 DB** (클라우드 PostgreSQL) | `DATABASE_URL`만 교체. 보통 `?sslmode=require` | 여러 인스턴스, 백업·장애 복구를 맡기고 싶을 때 |

컨테이너끼리는 **서비스 이름으로** 서로를 찾는다(`db`). 앱에서 `localhost`는 **자기 자신 컨테이너**다.

실측(②): PostgreSQL 이미지(`axum-demo:pg`, 158MB)를 `compose.postgres.yaml`의 DB와 **같은 Docker 네트워크**에 띄워 `DATABASE_URL=postgres://postgres:postgres@db:5432/board`로 연결했다. 가입 → 로그인 → 글 작성(201) → 검색까지 동작했고, 마이그레이션(`migrations_pg/`)은 앱이 시작할 때 자동 적용됐으며 `docker stop`은 0.4초였다.

### 마이그레이션은 앱이 시작할 때 자동 실행

`db::init()`이 `sqlx::migrate!`를 실행한다. 이미 적용된 것은 건너뛴다. **여러 인스턴스가 동시에 뜨면** 경합할 수 있어서(sqlx는 락으로 보호하지만) 큰 서비스는 마이그레이션을 **별도 단계**(배포 전 1회)로 분리한다.

### 배포 후 점검표

배포했다면 위에서 아래로 확인한다.

```
□ 1. 프로세스      docker ps 에서 Up, 재시작 반복(restarting) 아님
□ 2. 헬스체크      curl -fsS https://도메인/health → {"status":"ok"} / docker inspect → healthy
□ 3. 로그          docker logs 에 ERROR·panic 없음. "database ready", "listening" 확인. 비밀번호·토큰이 로그에 없다
□ 4. 설정          기본 JWT_SECRET 경고(WARN)가 없다. BIND_ADDR=0.0.0.0, RUST_LOG=info
□ 5. HTTPS         http:// 접속 시 https://로 이동. 인증서 유효기간 확인
□ 6. 핵심 시나리오  가입 → 로그인 → 글 작성 → 목록 → 수정 → 삭제 (curl 또는 Swagger UI)
□ 7. 인증/인가     토큰 없이 글쓰기 401, 남의 글 수정 403
□ 8. 오류 모양     없는 경로 404, 잘못된 입력 400이 { error: { code, message, requestId } } 인지
□ 9. 데이터 유지   컨테이너 재생성 후에도 글이 남는다 (볼륨/DB)
□ 10. 정상 종료    docker stop 이 1초 안팎, 로그에 "종료 신호(SIGTERM)"
□ 11. 비root       docker exec <컨테이너> id → appuser
□ 12. CORS         허용 출처가 운영 프론트 주소로 설정되어 있다 (개발용 localhost 아님)
□ 13. 백업         DB 파일/볼륨 백업이 실제로 되고 복원해 봤다 (study16)
□ 14. 롤백 경로    직전 이미지 태그를 알고 있다 (latest만 쓰지 않는다)
```

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| `docker build` 중 `failed to run custom build command for utoipa-swagger-ui` — `trying to download using curl system package` | 그 크레이트의 `build.rs`가 **빌드 중에 GitHub에서 Swagger UI zip을 curl로 내려받는다.** slim 빌더 이미지에는 curl이 없고, 오프라인 CI에서는 네트워크도 없다 | `utoipa-swagger-ui = { features = ["axum", "vendored"] }` — 정적 파일을 크레이트에 포함해 **빌드가 네트워크에 의존하지 않게** 한다. (로컬에서는 curl·네트워크가 있어서 문제를 못 봤다) |
| `docker stop`이 10초 걸리고 강제 종료됨 | SIGTERM을 처리하지 않음 (PID 1은 핸들러 없는 신호를 무시) | `shutdown_signal`에서 SIGTERM 처리 |
| 컨테이너에서 실행했는데 밖에서 접속이 안 됨 | `BIND_ADDR=127.0.0.1` | `0.0.0.0:3000` |
| 컨테이너를 지웠더니 DB가 사라짐 | 볼륨을 마운트하지 않음 | `-v 이름:/app/data` |
| 컨테이너가 계속 재시작 (`Restarting`) | 부팅 중 오류 (짧은 `JWT_SECRET`, DB 연결 실패 등) | `docker logs`로 이유 확인. 시작 시 검증이 이 경우 **즉시, 명확하게** 실패하게 해 준다 |
| `permission denied` (볼륨 쓰기) | `appuser`가 볼륨 소유자가 아님 | Dockerfile에서 `chown`, 호스트 디렉터리를 마운트할 땐 권한 확인 |
| 소스를 한 줄 고쳤는데 의존성 전체를 다시 컴파일 | 의존성 캐싱 트릭이 깨짐 (`COPY . .`를 먼저 했거나 `Cargo.lock` 미포함) | `Cargo.toml`/`Cargo.lock`만 먼저 복사 → 가짜 빌드 → 소스 복사 |
| 이미지에 가짜 `fn main() {}`이 들어감 (실행하면 아무 일도 안 함) | `touch`를 빠뜨려 두 번째 빌드가 캐시된 가짜 결과를 씀 | `touch src/main.rs src/lib.rs` |
| `docker build`가 시작도 안 하고 몇 분 걸림 | `.dockerignore`에 `target/`이 없어 수 GB를 전송 | `.dockerignore`의 `target/` |
| 브라우저에서 HTTPS인데 로그인 쿠키가 안 붙음 | 프록시 뒤라 앱이 `http`로 인식해 `Secure` 판단이 어긋나거나, 반대로 `Secure` 쿠키를 http로 접속 | `SECURE_COOKIE` 환경변수로 명시 (study7) |
| 시작 로그에 **DB 비밀번호가 그대로** 찍힘 (`database_url=postgres://postgres:postgres@db…`) | `DATABASE_URL`을 그대로 `tracing`에 출력 | `Config::redacted_database_url()`로 비밀번호를 `***`로 가려 출력 (단위 테스트 포함). 비밀 값은 **로그에도 남기지 않는다** |
| 이미지가 예상(~100MB)보다 큼 (실측 160MB) | `debian:bookworm-slim`(~75MB) + `curl` + 바이너리(19MB) | 더 줄이려면 헬스체크를 앱 서브커맨드로 대체해 `curl` 제거, 또는 `distroless`/`alpine`(musl 타깃 필요) |

---

## ⑥ 직접 해보기

1. **이미지 빌드 시간 비교**: `src/routes.rs`의 응답 문자열 한 줄을 바꾸고 `docker build`를 다시 돌려 **의존성 레이어가 캐시되는지**(`CACHED`) 확인한다. 이어서 `Cargo.toml`에 의존성을 하나 추가했을 때 어떻게 달라지는지 본다.
2. **PostgreSQL 이미지**: `docker build --build-arg CARGO_FLAGS="--no-default-features --features postgres" -t axum-demo:pg .` 후 `compose.postgres.yaml`의 DB 컨테이너와 **같은 네트워크**에서 `DATABASE_URL=postgres://postgres:postgres@db:5432/board`로 실행하고 점검표 6번 시나리오를 돌린다 (study11).
3. **점검표 스크립트화**: 위 점검표 2·6·7·8번을 `curl`과 `jq`/`python`으로 자동 실행하는 `smoke.sh`를 만든다. 실패하면 종료 코드가 0이 아니게 한다 — study16의 CI/CD에서 그대로 쓴다.
4. **단계 예제로 실험**: `s10_deploy`에서 `SHUTDOWN_AFTER_MS=1000`으로 띄우고 `/slow?secs=5`를 호출한다. 응답이 끊기지 않는지 확인한 뒤, `serve()`에서 `.with_graceful_shutdown(shutdown)` 줄을 지우고 같은 실험을 해 어떻게 달라지는지 본다.

---

## ⑦ ☐ 체크리스트

- ☐ `debug`/`release` 빌드의 차이와 `[profile.release]`의 각 항목, `panic = "abort"`를 켜면 안 되는 이유를 설명할 수 있다
- ☐ 멀티 스테이지 빌드가 왜 필요한지(빌드 수 GB → 실행 이미지)를 설명할 수 있다
- ☐ 의존성 레이어 캐싱 트릭(가짜 소스 → 의존성 컴파일 → 진짜 소스)과 `touch`가 필요한 이유를 설명할 수 있다
- ☐ `.dockerignore`의 `target/`, `.env`가 중요한 이유를 안다
- ☐ 컨테이너에서 `BIND_ADDR=0.0.0.0`, 비root 실행, 비밀 주입, 볼륨 마운트가 각각 왜 필요한지 설명할 수 있다
- ☐ `docker stop`이 SIGTERM을 받아 **빠르게 정상 종료**하는 것을 직접 확인했다
- ☐ 도메인 → DNS → 리버스 프록시(HTTPS 종료) → 앱의 흐름을 그릴 수 있다
- ☐ 컨테이너를 지웠다 다시 띄워도 데이터가 남는 것을 직접 확인했다
- ☐ 배포 후 점검표 14개를 순서대로 말할 수 있다

---

## 기초 구간(study0~10) 총정리

| 단계 | 핵심 | 대표 산출물 |
|---|---|---|
| study0 환경 | rustup · cargo · rust-analyzer · `.env`/`.gitignore` | `hello-axum` |
| study1 Hello World | 핸들러 = `async fn`, `IntoResponse`, `{name}` 경로, 이스케이프 | `s01_hello` |
| study2 구조 | 핸들러 → Router → 모듈, `State`, `with_state` | `s02_modules` |
| study3 데이터 | `Arc<RwLock<…>>`, 파일 저장(임시 파일 + rename), 바인딩 | `s03_store` |
| study4 라우팅 | 경로/쿼리, 404/405, 미들웨어 흐름, CORS | `s04_routing` |
| study5 폼·검증 | `validator`, `ValidatedJson`, 상태 코드별 화면, `Idempotency-Key` | `s05_form` |
| study6 DB·API | sqlx · 마이그레이션 · CRUD · 스펙 v1 오류 본문/camelCase · 환경변수 | `demo/` |
| study7 인증·보안 | Argon2 · JWT · `AuthUser` · 401/403 · CORS/XSS/CSRF | `demo/auth` |
| study8 구조화 | lib+bin · 공통 모듈 · `Config` · 미들웨어 순서 | `demo/lib.rs` |
| study9 테스트 | 단위 · `oneshot` · mock · 디버깅 · 오류 TOP 10 | `demo/tests`, `lab/mock_demo` |
| study10 배포 | 릴리스 · 멀티 스테이지 Docker · HTTPS · 점검표 | `Dockerfile` |

**기초 구간에서 얻은 원칙 일곱 가지**

1. **핸들러는 함수, 라우터는 값.** 애노테이션도 컨테이너도 없다. 시그니처가 곧 계약이다.
2. **Extractor가 전부다.** 파싱·검증·인증이 인자 타입으로 표현되고, 실패하면 핸들러는 호출되지 않는다.
3. **오류는 값이다.** `Result<T, AppError>` + `IntoResponse`로 한 곳에서 HTTP로 번역하고, 새 갈래는 컴파일러가 누락을 알려 준다.
4. **인증(401)과 인가(403)는 다른 문제.** 인가는 DB를 읽어야 해서 extractor가 아니라 핸들러/서비스의 몫이다.
5. **미들웨어는 순서가 전부다.** 전체에 적용할 것은 바깥, 조건부는 안쪽.
6. **버전부터 확인한다.** 오류의 절반은 버전 차이였다 (axum 0.8 `{id}`, jsonwebtoken 11 `rust_crypto`, sqlx 0.9 `SqlSafeStr`, argon2 0.6).
7. **직접 돌려 본 것만 믿는다.** Docker 빌드를 돌려서야 `curl` 의존, SIGTERM 미처리가 드러났다.
- ☐ `cargo run --example s10_deploy`로 짧은 비밀키 거부, DB 비밀번호 마스킹, 종료 신호 뒤에도 진행 중 요청이 끝까지 응답받는 것을 직접 확인했다

---

## ⑧ 다음 단계 연결

기초 구간이 끝났다. [studyMidpoint](studyMidpoint.md)에서는 study0~10에서 배운 것**만으로** 게시판 + 로그인 + CRUD 서비스를 처음부터 끝까지 조립하는 순서를 문서로 정리하고, 1차 배포 체크리스트와 회고로 중간 점검을 한다.
