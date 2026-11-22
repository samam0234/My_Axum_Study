# study16 — 운영 · CI/CD

> 🎯 **목표**: 서비스를 **계속 안전하게 굴리는** 체크리스트를 갖춘다: Docker/compose 운영 구성, GitHub Actions로 테스트·이미지 빌드 자동화, 로그·모니터링, 백업과 **복원 검증**. 설명보다 **체크리스트와 직접 돌려 본 결과** 중심이다.

| 항목 | 내용 |
|---|---|
| 관련 파일 | 단계 예제 [lab/examples/s16_ops.rs](lab/examples/s16_ops.rs)(포트 3016), 실제 프로젝트: [.github/workflows/ci.yml](.github/workflows/ci.yml), [.github/workflows/docker-publish.yml](.github/workflows/docker-publish.yml), [demo/Dockerfile](demo/Dockerfile), [demo/compose.yaml](demo/compose.yaml), [demo/compose.postgres.yaml](demo/compose.postgres.yaml) |
| 로컬 검증 | 워크플로 YAML 문법 검증 + **워크플로가 실행하는 것과 같은 명령**(`cargo test`, `cargo check --no-default-features --features postgres`, `cargo test --workspace`, `cargo test --lib --examples`)을 로컬에서 실행해 통과 확인 · 백업/복원 실측(PostgreSQL, SQLite) |
| GitHub에서의 실제 실행 | `ci.yml` 첫 실행 **3 job 모두 성공** (아래 ③-4). `docker-publish.yml`은 태그 푸시 때만 돌아서 아직 미실행 |

---

## ① 목표

1. 컨테이너 운영 설정(재시작·헬스체크·볼륨·비밀)을 체크리스트로 점검한다.
2. push/PR마다 **자동으로 테스트**하고, 태그를 푸시하면 **이미지를 빌드해 레지스트리에 올리는** 파이프라인을 만든다.
3. 로그와 최소한의 모니터링(헬스·알림)을 구성한다.
4. 백업을 **받는 것**이 아니라 **복원해 보는 것**까지 한다.

---

## ② 📖 이론

### CI와 CD

| | 하는 일 | 이 저장소 |
|---|---|---|
| **CI** (지속적 통합) | 변경이 올라올 때마다 **빌드·테스트를 자동 실행** → 깨지면 바로 알림 | `ci.yml` |
| **CD** (지속적 배포) | 통과한 것을 **이미지로 만들어 배포 가능한 상태로** (또는 서버에 자동 배포) | `docker-publish.yml` (이미지 게시까지. 서버 배포 job은 주석으로 준비) |

원칙: **main에 들어간 것은 항상 테스트를 통과한 것**이어야 한다. 테스트가 없으면 CI는 "컴파일되는지"만 본다 — 그래서 study9의 시험이 먼저다.

### 운영에서 계속 지켜야 할 것 — 네 축

```
① 올리기   이미지 빌드 → 레지스트리 → 서버      (CI/CD, 롤백용 태그)
② 지키기   헬스체크, 재시작, 비root, 시크릿       (compose, Dockerfile)
③ 보기     로그, 요청 ID, 메트릭, 알림            (tracing, 모니터링)
④ 되살리기 백업, 복원 연습                        (DB 백업 + 복원 검증)
```

---

## ③ 🛠️ 실기

### 3-0. 단계 예제 — [lab/examples/s16_ops.rs](lab/examples/s16_ops.rs) (포트 3016)

운영에서 **서버 코드로 할 수 있는 장치**를 한 파일에 담았다. JSON(구조화) 로그, liveness와 readiness의 구분, `/metrics`, 그리고 백업은 **복원해 보는 것**이라는 것(SQLite WAL에서 `.db`만 복사하면 빈 DB). GitHub Actions 워크플로는 아래 3-2·3-3이다.

```powershell
cd lab
cargo run --example s16_ops                                  # 사람이 읽는 로그
$env:LOG_FORMAT = "json"; cargo run --example s16_ops        # JSON 로그
cargo run --example s16_ops -- backup backup.db              # 안전한 백업(VACUUM INTO) 후 복원 검증
cargo test --example s16_ops                                 # 시험 3개
```

```rust
//! study16 — 운영 · CI/CD 중 **서버 코드로 할 수 있는 운영 장치**. 포트 3016.
//!
//!   ① 구조화(JSON) 로그        LOG_FORMAT=json — 로그 수집기가 필드로 검색한다
//!   ② 헬스 두 종류             `/health`(살아 있나, liveness) vs `/ready`(DB까지 쓸 수 있나, readiness)
//!   ③ 메트릭                   `/metrics` — 요청 수(상태 계열별)·처리 중 요청·누적 처리 시간 (Prometheus 텍스트)
//!   ④ 백업은 **복원해 보는 것**   SQLite WAL에서 `.db`만 복사하면 빈 DB → `VACUUM INTO`가 안전한 방법
//! (GitHub Actions 워크플로는 .github/workflows/ci.yml, docker-publish.yml)
//!
//! 실행:  cargo run --example s16_ops
//!        LOG_FORMAT=json cargo run --example s16_ops
//!        cargo run --example s16_ops -- backup backup.db      # 안전한 백업(VACUUM INTO) 후 복원 검증

use std::{
    path::Path,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

// ── ① 로그: 사람이 읽는 형식과 기계가 읽는 형식 ──────────────────────────────
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "s16_ops=info".into());
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        builder.json().init(); // {"timestamp":..,"level":"INFO","fields":{"method":"GET","status":200,..}} — 필드로 검색·집계
    } else {
        builder.compact().init();
    }
}

// ── ③ 메트릭 ───────────────────────────────────────────────────────────────
#[derive(Default)]
struct Metrics {
    requests_2xx: AtomicU64,
    requests_4xx: AtomicU64,
    requests_5xx: AtomicU64,
    in_flight: AtomicU64,
    duration_micros_total: AtomicU64,
}

impl Metrics {
    fn render(&self) -> String {
        // Prometheus 텍스트 형식: 줄마다 `이름{라벨} 값`. 수집기가 주기적으로 긁어 간다
        format!(
            "# TYPE http_requests_total counter\n\
             http_requests_total{{class=\"2xx\"}} {}\n\
             http_requests_total{{class=\"4xx\"}} {}\n\
             http_requests_total{{class=\"5xx\"}} {}\n\
             # TYPE http_requests_in_flight gauge\n\
             http_requests_in_flight {}\n\
             # TYPE http_request_duration_seconds_sum counter\n\
             http_request_duration_seconds_sum {:.6}\n",
            self.requests_2xx.load(Ordering::Relaxed),
            self.requests_4xx.load(Ordering::Relaxed),
            self.requests_5xx.load(Ordering::Relaxed),
            self.in_flight.load(Ordering::Relaxed),
            self.duration_micros_total.load(Ordering::Relaxed) as f64 / 1_000_000.0,
        )
    }
}

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    metrics: Arc<Metrics>,
}

/// 모든 요청을 세고 재는 미들웨어. 로그도 여기서 **구조화된 필드**로 남긴다.
async fn track(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let (method, path) = (request.method().clone(), request.uri().path().to_string());
    state.metrics.in_flight.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();

    let response = next.run(request).await;

    let elapsed = started.elapsed();
    state.metrics.in_flight.fetch_sub(1, Ordering::Relaxed);
    state.metrics.duration_micros_total.fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
    let status = response.status();
    let counter = match status.as_u16() {
        200..=299 => &state.metrics.requests_2xx,
        400..=499 => &state.metrics.requests_4xx,
        500..=599 => &state.metrics.requests_5xx,
        _ => return response,
    };
    counter.fetch_add(1, Ordering::Relaxed);

    // 문자열을 이어 붙이지 않고 필드로 남긴다 → JSON 로그에서 status > 499 같은 조건으로 검색
    tracing::info!(%method, %path, status = status.as_u16(), elapsed_ms = elapsed.as_millis() as u64, "request");
    response
}

// ── ② 헬스 두 종류 ─────────────────────────────────────────────────────────
/// liveness: 프로세스가 살아 있나. 여기가 실패하면 **재시작**이 답이다. DB를 보지 않는다
/// (DB가 죽었다고 앱을 재시작해 봐야 소용없고, 재시작 폭풍만 만든다)
async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

/// readiness: 지금 요청을 받을 수 있나. 실패하면 **트래픽에서 빼는** 것이 답이다. DB까지 확인한다
async fn ready(State(state): State<AppState>) -> Response {
    match sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => Json(json!({ "status": "ready" })).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "readiness 실패");
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "status": "unavailable" }))).into_response()
        }
    }
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")], state.metrics.render())
}

#[derive(Deserialize)]
struct NewNote {
    text: String,
}

async fn add_note(State(state): State<AppState>, Json(body): Json<NewNote>) -> Result<StatusCode, StatusCode> {
    sqlx::query("INSERT INTO notes (text) VALUES ($1)").bind(body.text).execute(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

async fn count_notes(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes").fetch_one(&state.db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "data": { "notes": n } })))
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/metrics", get(metrics))
        .route("/api/notes", get(count_notes).post(add_note))
        .layer(axum::middleware::from_fn_with_state(state.clone(), track))
        .with_state(state)
}

// ── DB 연결과 ④ 백업 ───────────────────────────────────────────────────────
async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    let in_memory = url.contains(":memory:");
    let mut options = SqliteConnectOptions::from_str(url)?.create_if_missing(true).busy_timeout(Duration::from_secs(5));
    if !in_memory {
        options = options.journal_mode(SqliteJournalMode::Wal); // 읽기·쓰기가 서로 막지 않는다. 대신 최근 쓰기는 -wal 파일에 있다
    }
    let pool = SqlitePoolOptions::new().max_connections(if in_memory { 1 } else { 5 }).connect_with(options).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL)").execute(&pool).await?;
    Ok(pool)
}

/// 일관된 스냅샷을 **새 파일 하나**로 만든다. 서버를 멈추지 않아도 된다.
/// (`.db` 파일만 복사하면 -wal에 있는 최근 쓰기가 빠진다. 대상 파일은 미리 있으면 안 된다)
async fn backup_to(db: &SqlitePool, dest: &Path) -> anyhow::Result<()> {
    sqlx::query("VACUUM INTO $1").bind(dest.to_string_lossy().to_string()).execute(db).await?;
    Ok(())
}

/// 복원 리허설: 백업 파일을 **새로 열어** 보고 안에 데이터가 있는지 센다. 백업은 복원해 봐야 백업이다.
async fn verify_restore(path: &Path) -> anyhow::Result<i64> {
    let restored = connect(&format!("sqlite://{}?mode=rw", path.display())).await?;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes").fetch_one(&restored).await?;
    restored.close().await;
    Ok(n)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://s16.db?mode=rwc".into());
    let db = connect(&url).await?;

    // `cargo run --example s16_ops -- backup out.db`
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [cmd, dest] = args.as_slice() && cmd == "backup" {
        let started = Instant::now();
        backup_to(&db, Path::new(dest)).await?;
        let rows = verify_restore(Path::new(dest)).await?;
        println!("백업 {dest} 완료, 복원 검증: notes {rows}건 ({}ms)", started.elapsed().as_millis());
        return Ok(());
    }

    let state = AppState { db, metrics: Arc::default() };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3016").await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");
    axum::serve(listener, app(state)).await?;
    Ok(())
}
```

시험 코드는 같은 파일 아래쪽의 `#[cfg(test)] mod tests`에 있다: [s16_ops.rs](lab/examples/s16_ops.rs).

| 코드 | 설명 |
|---|---|
| `LOG_FORMAT=json` → `builder.json()` | 로그를 문장이 아니라 **필드**로 남긴다. 수집기에서 `status > 499` 같은 조건으로 검색·집계할 수 있다 |
| `tracing::info!(%method, %path, status = …, elapsed_ms = …, "request")` | 문자열을 이어 붙이지 않고 필드로 넘긴다. **비밀번호·토큰은 필드에 넣지 않는다** |
| `Metrics` + `AtomicU64` | 락 없이 카운터를 올린다. `Relaxed` 순서면 충분하다 (정확한 순서가 아니라 합계만 필요) |
| `track` 미들웨어 (`from_fn_with_state`) | 모든 요청을 세고(상태 계열별) 시간을 재고 `in_flight`를 올렸다 내린다. 로그도 여기서 한 번에 |
| `/metrics`의 Prometheus 텍스트 | `이름{라벨} 값` 줄. 수집기가 주기적으로 긁어 간다. 요청 수는 **누적 counter**, 처리 중 요청은 **gauge** |
| `/health` (liveness) | 프로세스가 살아 있나. 실패하면 **재시작**이 답이다. **DB를 보지 않는다** — DB가 죽었다고 앱을 재시작해 봐야 소용없고 재시작 폭풍만 만든다 |
| `/ready` (readiness) | 지금 요청을 받을 수 있나(DB까지). 실패하면 **트래픽에서 빼는** 것이 답이다. 실패는 503 |
| `journal_mode(Wal)` | 읽기·쓰기가 서로 막지 않는다. 대신 **최근 쓰기는 `-wal` 파일에 있다** — 이것이 백업 함정의 원인 |
| `backup_to` = `VACUUM INTO $1` | 서버를 멈추지 않고 **일관된 스냅샷을 새 파일 하나로** 만든다. 대상 파일은 미리 있으면 안 된다 |
| `verify_restore` | 백업 파일을 **새로 열어** 데이터를 센다. 받는 것이 아니라 **복원해 보는 것**이 백업이다. 걸린 시간이 RTO의 첫 근사치 |
| `cargo run --example s16_ops -- backup out.db` | 같은 코드를 CLI로. `let [cmd, dest] = args.as_slice() && cmd == "backup"` (let-chain, edition 2024) |

**실행 결과** (직접 실행한 결과):

```
$ LOG_FORMAT=json cargo run --example s16_ops   (글 3개 작성 후)
$ curl .../health        {"status":"ok"}               $ curl .../ready   {"status":"ready"}
$ curl .../api/notes     {"data":{"notes":3}}          $ curl .../nope    → 404
$ curl .../metrics
# TYPE http_requests_total counter
http_requests_total{class="2xx"} 6
http_requests_total{class="4xx"} 1
http_requests_total{class="5xx"} 0
# TYPE http_requests_in_flight gauge
http_requests_in_flight 1                                    ← 이 /metrics 요청 자신이 처리 중
# TYPE http_request_duration_seconds_sum counter
http_request_duration_seconds_sum 0.006553

JSON 로그 (마지막 2줄):
{"timestamp":"2026-11-22T18:58:13.544650Z","level":"INFO","fields":{"message":"request","method":"GET","path":"/nope","status":404,"elapsed_ms":0},"target":"s16_ops"}
{"timestamp":"2026-11-22T18:58:13.778160Z","level":"INFO","fields":{"message":"request","method":"GET","path":"/metrics","status":200,"elapsed_ms":0},"target":"s16_ops"}

서버가 떠 있는 동안의 파일:  s16.db  s16.db-shm  s16.db-wal               ← 최근 쓰기는 -wal에 있다
$ s16.db만 복사해서 열기  →  no such table: notes                         ← ❌ 테이블이 없는 DB (WAL의 쓰기가 빠졌다)
$ cargo run --example s16_ops -- backup backup.db    (같은 DB, 서버는 계속 실행 중)
백업 …/backup.db 완료, 복원 검증: notes 3건 (23ms)                           ← ✅ VACUUM INTO. 서버를 멈추지 않고, 복원해서 3건 확인
```

시험 (`cargo test --example s16_ops`):

```
test tests::메트릭은_상태_계열별로_요청을_센다 ... ok
test tests::liveness는_db와_무관하고_readiness는_db가_죽으면_503이다 ... ok     ← 풀을 닫아도 /health는 200, /ready는 503
test tests::db_파일만_복사하면_빈_db고_vacuum_into는_복원된다 ... ok
test result: ok. 3 passed
```

아래 3-1부터는 이 개념이 실제 프로젝트(`demo/`)에서 어떤 모양이 되는지 이어서 본다.

### 3-1. Docker · compose 운영 체크리스트

[demo/compose.yaml](demo/compose.yaml):

```yaml
services:
  api:
    build: .
    image: axum-demo:latest
    ports: ["3000:3000"]
    environment:
      JWT_SECRET: ${JWT_SECRET:?JWT_SECRET 환경변수를 설정하세요}    # 없으면 컨테이너 시작을 거부
      RUST_LOG: demo=info,tower_http=info
    volumes:
      - api-data:/app/data          # DB
      - api-uploads:/app/uploads    # 업로드 파일
    restart: unless-stopped
volumes: { api-data: , api-uploads: }
```

```
이미지
□ 멀티 스테이지 (빌더 버리고 바이너리만)                          실측 160MB
□ 비root 사용자 (USER appuser)                                    실측 uid=1000
□ 의존성 레이어 캐싱 (Cargo.toml/lock 먼저)                       소스만 바뀌면 내 코드 컴파일 ~23초
□ .dockerignore에 target/ .env *.db                              
□ 비밀을 이미지에 굽지 않았다 (JWT_SECRET은 실행 시 주입)
□ 이미지 태그를 버전(또는 커밋 해시)으로 — latest만 쓰지 않는다 (롤백용)

실행
□ restart: unless-stopped  (프로세스가 죽거나 서버가 재부팅돼도 다시 뜬다)
□ HEALTHCHECK 있음 (curl /health)  → docker ps 에서 (healthy)
□ 볼륨: DB·업로드가 컨테이너 밖에 있다 (컨테이너를 지워도 데이터가 산다)
□ BIND_ADDR=0.0.0.0:3000, 포트는 필요한 것만 노출
□ SIGTERM으로 1초 안에 정상 종료 (docker stop 0.4초 실측)  ← 안 되면 배포마다 요청이 끊긴다
□ 리소스 제한(메모리/CPU)을 둔다 (한 컨테이너가 서버 전체를 먹지 않게)
□ 로그 드라이버/크기 제한 (json-file max-size) — 안 하면 로그가 디스크를 채운다
```

compose에서 DB 컨테이너와 앱을 함께 쓸 때(PostgreSQL): DB에 `healthcheck: pg_isready`, 앱에 `depends_on: { db: { condition: service_healthy } }`. 앱 이미지는 `--build-arg CARGO_FLAGS="--no-default-features --features postgres"`로 빌드한다 (study10·11).

### 3-2. GitHub Actions — CI: [.github/workflows/ci.yml](.github/workflows/ci.yml)

```yaml
name: ci

on:
  push:
    branches: [master]
  pull_request:

concurrency:                                  # 같은 브랜치에 새 푸시가 오면 이전 실행은 취소
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  demo:
    name: demo (sqlite 테스트 + postgres 빌드)
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: demo } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2          # target/과 ~/.cargo 캐시 — Rust CI에서 가장 효과가 큰 한 줄
        with: { workspaces: demo }
      - name: 테스트 (in-memory SQLite)
        run: cargo test
      - name: PostgreSQL feature로도 컴파일되는가
        run: cargo check --no-default-features --features postgres

  project:
    runs-on: ubuntu-latest      # 워크스페이스 전체 테스트 (도메인 서비스 시험)
    defaults: { run: { working-directory: project } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: project }
      - run: cargo test --workspace

  lab:
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: lab } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: lab }
      - run: cargo test --lib --examples
```

| 항목 | 설명 |
|---|---|
| `on: push (master) / pull_request` | 메인에 올라갈 때와 PR 때 실행 |
| 프로젝트 3개 → job 3개 | 서로 의존하지 않아 **병렬**. 하나가 깨져도 나머지 결과를 본다 |
| `working-directory` | 저장소 루트가 아니라 각 크레이트 폴더에서 실행 |
| `Swatinem/rust-cache` | 의존성 컴파일 결과를 캐시. 없으면 매번 수 분 |
| `concurrency` | 연속 푸시 시 옛 실행을 취소해 러너 시간을 아낀다 |
| 테스트는 DB 서버 불필요 | in-memory SQLite + `oneshot`(study9) 덕분. PostgreSQL은 **컴파일만** 확인 (서비스 컨테이너로 실제 시험하려면 `services: postgres:` 추가) |
| 비밀 | 이 파이프라인에는 비밀이 **없다.** 키가 필요한 시험은 가짜로 (study13) |

CI에서 `cargo test`가 통과하려면 **서버·네트워크·외부 서비스 없이** 돌아야 한다. study9의 가짜들이 그 조건을 만든다.

### 3-3. GitHub Actions — CD: [.github/workflows/docker-publish.yml](.github/workflows/docker-publish.yml)

```yaml
name: docker-publish
on:
  push:
    tags: ["v*"]                  # git tag v0.1.0 && git push origin v0.1.0
  workflow_dispatch:              # 수동 실행 버튼도
permissions:
  contents: read
  packages: write                 # GHCR에 올리려면 필요

jobs:
  publish:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false            # 한 이미지가 실패해도 다른 이미지는 계속
      matrix:
        include:
          - { name: demo,    context: demo }        # 게시판 API
          - { name: project, context: project }     # 최종 서비스 (웹사이트 + API)
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with: { registry: ghcr.io, username: "${{ github.actor }}", password: "${{ secrets.GITHUB_TOKEN }}" }
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/${{ github.repository }}/${{ matrix.name }}
          tags: |
            type=semver,pattern={{version}}      # v0.1.0 → 0.1.0
            type=raw,value=latest
      - uses: docker/build-push-action@v6
        with:
          context: ${{ matrix.context }}
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          cache-from: type=gha,scope=${{ matrix.name }}      # 레이어 캐시를 Actions 캐시에 (이미지별 범위)
          cache-to: type=gha,mode=max,scope=${{ matrix.name }}
```

| 항목 | 설명 |
|---|---|
| 트리거가 **태그** | 매 푸시마다 이미지를 만들지 않는다. **릴리스 의도가 있을 때만**. 이미지 태그가 버전과 1:1 |
| `GITHUB_TOKEN` | 워크플로가 자동으로 받는 토큰. **별도 비밀 등록 없이** GHCR에 올린다. `packages: write` 권한만 열어 준다 (최소 권한) |
| `metadata-action` | 태그 이름에서 이미지 태그를 만든다 → 롤백할 때 "어떤 버전이었나"가 남는다 |
| `cache-from/to: gha` | Dockerfile의 의존성 레이어 캐싱 트릭과 함께 쓰면 소스만 바뀐 빌드가 크게 줄어든다 |
| 서버 배포 job | 주석으로 준비: `appleboy/ssh-action`으로 서버에서 `docker compose pull && up -d`, 마지막에 **`curl /health`**(배포 직후 헬스체크, 실패하면 job 실패). 필요한 시크릿 3개(`DEPLOY_HOST/USER/SSH_KEY`)를 등록해야 켤 수 있다 |

> **자동 배포가 안전해지는 조건**: ① CI가 통과해야만 태그를 붙인다 ② 배포 직후 헬스체크가 실패하면 **자동으로 이전 태그로 되돌린다** (또는 사람이 보게 알린다) ③ 마이그레이션이 **앞뒤 버전 모두에서 동작**하도록 (컬럼 삭제는 두 번에 나눠서).

### 3-4. 로컬에서 같은 명령을 먼저 돌려 본 결과

```
demo     $ cargo test                                                      → 단위 12 + 통합 31 통과
demo     $ cargo check --no-default-features --features postgres           → Finished
project  $ cargo test --workspace                                          → domain 서비스 8 + 유틸 1, web 단위 6 + HTTP 20 통과 (SQLite·PostgreSQL·MySQL 드라이버 포함)
lab      $ cargo test --lib --examples                                     → 31개 통과 (예제별 시험 + mock 3, s16의 WAL 백업 시험 포함)
워크플로 YAML                                                               → 문법 검증 통과 (jobs: demo, project, lab / publish)
```

**GitHub Actions 실제 실행 결과** — 첫 실행(커밋 `50acec1` 푸시 직후)은 세 job 모두 **success**였고, 이후 스펙 v1 정렬(`1349aed` demo, `b511e9d` project)과 단계 예제 추가(`s06~s10`, `s16`)를 푸시했을 때도 `ci`가 모두 **success**였다. 마지막 실행(`a03e8aa`)의 job별 결과:

```
ci  completed success   (커밋 a03e8aa)
  demo     (sqlite 테스트 + postgres 빌드)   success  30초   ← Ubuntu에서 cargo test(단위 12 + 통합 31) + PostgreSQL feature 컴파일
  project  (워크스페이스 테스트)              success  30초   ← domain 서비스 8 + web HTTP 20 + 단위 7
  lab      (예제 컴파일 + 테스트)             success  99초   ← sqlx·mongodb·reqwest·ws 포함 전 예제 컴파일 + 시험 31개 (s10의 종료 시험, s16의 WAL 백업 시험 포함)
```

lab이 99초로 가장 오래 걸리는 것은 새로 들어온 `sqlx`(SQLite 번들)·`argon2` 등을 처음 컴파일했기 때문이다. `Swatinem/rust-cache`가 `target/`과 레지스트리를 캐시하므로 의존성이 그대로면 다음 실행부터 수십 초로 줄어든다. 첫 실행보다 demo·project가 빨라진 것도 같은 이유다.

로컬(Windows)에서 통과한 것이 CI(Ubuntu)에서도 통과했다 — 경로 구분자·줄바꿈·설치된 도구에 의존하는 시험이 없다는 확인이기도 하다.

`docker-publish.yml`은 **`v*` 태그를 푸시할 때만** 실행되므로 아직 실행하지 않았다. 실행하면 이미지가 GHCR에 게시(공개 범위는 저장소 설정을 따른다)되므로, 첫 릴리스는 의도를 가지고 태그를 붙여 확인한다. (YAML 문법과 matrix 구성은 로컬에서 검증했다.)

### 3-5. 로그 · 모니터링 체크리스트

이 저장소가 **이미 갖고 있는 것**:

```
✔ 구조적 로그          tracing — 레벨(ERROR/WARN/INFO/DEBUG), 모듈 경로, key=value 필드
✔ 요청 ID              x-request-id 응답 헤더 = 오류 본문의 requestId = (TraceLayer의) 서버 로그
✔ 응답 시간            x-response-time-ms 헤더
✔ 헬스 엔드포인트      GET /health + Docker HEALTHCHECK
✔ 패닉 → 500 + 로그    CatchPanicLayer (연결이 끊기지 않고 로그에 패닉 메시지)
✔ 로그에 비밀 없음     DB 비밀번호 마스킹 (redacted_database_url)
```

**운영에서 추가할 것** (이 저장소에서 구현하지 않았다 — 도입 체크리스트):

```
□ 로그를 JSON 한 줄로 (tracing_subscriber의 json feature) → 로그 수집기(Loki, CloudWatch 등)가 필드로 검색
□ 로그 레벨: 운영 info, 문제 추적 때만 일시적으로 debug (RUST_LOG만 바꿔 재시작)
□ 로그 보관 기간·크기 제한 (컨테이너 로그가 디스크를 채우지 않게)
□ 메트릭: 요청 수, 오류율(5xx 비율), 지연 p50/p95/p99, DB 풀 사용량 → /metrics (Prometheus 형식)
□ 외부 헬스 감시: 서비스 바깥에서 1분마다 /health 호출 (업타임 서비스) — 서버가 통째로 죽은 것은 서버 안에서는 모른다
□ 알림: 5xx 비율 급증, 헬스 실패, 디스크 80%, 인증서 만료 14일 전 → 채팅/메일
□ 장애 대응: requestId로 로그 추적하는 절차를 문서화
□ 에러 추적(Sentry 류): 패닉/500을 스택과 함께 모은다
```

로그 읽는 법은 [study9 ③-5](study9.md).

### 3-6. ⭐ 백업 — 받는 것이 아니라 **복원해 보는 것**

#### SQLite (WAL 모드)에서 파일 복사는 안전하지 않다

실측: WAL 모드의 DB(`live.db` + `live.db-wal` + `live.db-shm`)에서 **`.db` 파일만 복사**하고 열어 봤더니:

```
files while connection open: ['live.db', 'live.db-shm', 'live.db-wal'] | live rows: 1000
naive copy of .db only  -> ERROR no such table: posts          ← 복사본에는 테이블조차 없다
VACUUM INTO snapshot    -> 1000 rows
backup API              -> 1000 rows
```

최근 쓰기는 아직 `-wal` 파일에 있고 본 파일에 반영되기 전이다. **백업을 받았다고 믿었는데 비어 있는** 가장 흔한 사고다. 안전한 방법:

```sql
VACUUM INTO '/backup/board-2026-11-22.db';      -- 일관된 스냅샷을 새 파일로 (실행 중에도 안전)
```

또는 SQLite의 온라인 백업 API(`sqlite3 live.db ".backup backup.db"`), 또는 앱을 잠시 멈추고 `-wal`/`-shm`까지 함께 복사.

#### PostgreSQL — `pg_dump` → 복원

```powershell
docker exec axumstudy-db-1 pg_dump -U postgres -Fc board > board.dump          # 커스텀 포맷 (압축, 선택 복원 가능)
docker exec axumstudy-db-1 psql -U postgres -c "create database board_restore"
docker exec -i axumstudy-db-1 pg_restore -U postgres -d board_restore < board.dump
```

실측: 원본 `posts 1, users 2` → 덤프 7,168 bytes → **새 DB로 복원 후 `posts 1, users 2, _sqlx_migrations 3`** 로 일치. 마이그레이션 기록까지 복원되어 앱을 새 DB에 붙여도 다시 마이그레이션하지 않는다.

#### 업로드 파일 (`uploads/`)

DB와 **한 세트**다. DB에는 파일명만 있고 파일이 없으면 깨진 이미지가 된다. 같은 시점에 함께 백업한다(볼륨 스냅샷, `tar`).

```
백업 체크리스트
□ 대상: DB + 업로드 파일 + (설정 값의 키 이름 목록; 비밀 값은 비밀 저장소에)
□ 방법: SQLite → VACUUM INTO / 백업 API (파일 복사 금지)   PostgreSQL → pg_dump 또는 PITR(WAL 아카이브)
□ 주기: 잃어도 되는 데이터 양(RPO)으로 정한다 (하루 1회? 1시간?)
□ 보관: 서버와 **다른 곳**(다른 디스크/리전/버킷), 보관 기간·개수, 암호화
□ ⭐ 복원 연습: 새 환경에서 복원 → 앱을 붙여 가입/조회가 되는지 (분기마다)
□ 복원 시간(RTO)을 쟀다: 장애 시 몇 분 안에 되살릴 수 있나
□ 백업 실패 알림 (조용히 실패하는 백업이 가장 위험하다)
```

---

## ④ 🔗 연동 포인트

| 이어지는 곳 | 어떻게 |
|---|---|
| **study9 테스트 → CI** | CI가 `cargo test`를 돌린다. 서버·네트워크 없이 도는 시험이 CI를 가능하게 한다 |
| **study10 Docker → CD** | `docker-publish.yml`이 같은 `Dockerfile`로 이미지를 만들어 GHCR에 올린다. 배포 후 점검표(study10)를 자동 스모크로 |
| **study11 DB** | CI에서 PostgreSQL feature 컴파일 확인. 시험까지 하려면 `services: postgres` + `TEST_DATABASE_URL` |
| **study13 시크릿** | 외부 서비스 키는 GitHub Secrets → 서버 환경변수. 이미지·저장소·로그에 없다 |
| **study14 버전** | 이미지 태그 = 릴리스 버전. API `Sunset` 날짜와 배포 일정을 맞춘다 |
| **study15 측정** | 모니터링의 지연/오류율 지표가 성능 작업의 **전후 비교 기준** |
| **README** | 상태 배지(CI 통과 여부)를 붙이면 저장소 첫 화면에서 보인다 |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| CI에서만 실패 (로컬은 통과) | CI에는 `.env`, 로컬 파일(`data.db`), 설치된 도구가 없다 / 운영체제가 Linux | 시험이 환경에 의존하지 않게. CI와 같은 명령을 **깨끗한 체크아웃**에서 돌려 본다 |
| 모든 실행이 매번 수 분 | 캐시 없음 | `Swatinem/rust-cache`, Docker는 `cache-from: gha` |
| Docker 이미지 빌드가 CI에서 `curl`/네트워크 오류 | 빌드 중 네트워크로 파일을 받는 크레이트 (예: `utoipa-swagger-ui`) | `vendored` feature (study10) |
| `denied: permission_denied: write_package` | `packages: write` 권한이 없거나 저장소 설정이 패키지 쓰기를 막음 | 워크플로 `permissions`, 저장소 Settings → Actions → Workflow permissions |
| 태그를 푸시했는데 워크플로가 안 돎 | 태그 패턴이 안 맞음(`v*`) / 태그를 만들었지만 푸시 안 함 | `git push origin v0.1.0` |
| 배포 후 새 컨테이너가 계속 재시작 | 부팅 시 설정 검증 실패(짧은 시크릿), DB 연결 실패 | `docker logs`. 시작 시 검증이 이유를 바로 보여 준다 |
| `docker compose up -d`가 옛 이미지를 씀 | 이미지 태그가 `latest` 고정이고 pull을 안 함 | `docker compose pull`, 버전 태그 사용 |
| 백업 파일을 열었더니 **테이블이 없음 / 데이터가 적음** | SQLite WAL에서 `.db`만 복사 (실측) | `VACUUM INTO` / 백업 API |
| 복원은 됐는데 앱이 마이그레이션을 또 실행/실패 | 마이그레이션 기록 테이블이 빠진 덤프 | 전체 덤프(`_sqlx_migrations` 포함)로 |
| 디스크가 갑자기 가득 참 | 컨테이너 로그·업로드·SQLite WAL이 무한 증가 | 로그 크기 제한, 업로드 정리 정책, 디스크 알림 |
| 배포 중 요청이 끊김 | `docker stop`이 SIGTERM을 못 받아 SIGKILL (10초) | `shutdown_signal`에 SIGTERM (study10, 실측 0.4초) |
| 시크릿이 Actions 로그에 찍힘 | `echo ${{ secrets.X }}` 또는 비밀을 URL에 포함 | 시크릿은 환경변수로만 넘기고 출력하지 않는다 (GitHub가 마스킹하지만 의존하지 않는다) |

---

## ⑥ 직접 해보기

1. **CI를 일부러 깨 보기**: 브랜치에서 통합 테스트 하나의 기대값을 틀리게 바꿔 PR을 올리고, Actions에서 어느 job·어느 시험이 실패하는지 읽는다. 고치면 초록으로 돌아오는지 본다.
2. **복원 리허설**: PostgreSQL `pg_dump` → 완전히 새 컨테이너/DB로 `pg_restore` → `axum-demo:pg` 이미지를 그 DB에 붙여 가입·조회가 되는지 확인한다. 걸린 시간을 적는다 (RTO).
3. **JSON 로그**: `tracing-subscriber`의 `json` feature를 켜고 `.json()` 포매터로 바꿔, 로그 한 줄을 `python -m json.tool`로 파싱해 본다. `requestId`로 필터하는 `grep`/`jq` 한 줄을 만든다.
4. **단계 예제로 실험**: `s16_ops`의 `backup_to`를 `std::fs::copy`로 바꾸고 `cargo run --example s16_ops -- backup out.db`를 실행한다. `verify_restore`가 어떻게 실패하는지 보고, 왜 "백업은 복원해 봐야 백업"인지 설명해 본다.

---

## ⑦ ☐ 체크리스트

- ☐ CI(통합·테스트 자동화)와 CD(배포 가능한 산출물 자동화)의 차이를 설명할 수 있다
- ☐ `ci.yml`의 트리거·job 분리·캐시·`concurrency`가 각각 왜 있는지 설명할 수 있다
- ☐ 워크플로가 실행하는 명령을 **로컬에서 먼저** 돌려 통과를 확인했다
- ☐ 이미지 태그를 버전으로 남기는 이유(롤백)를 안다
- ☐ `GITHUB_TOKEN` + 최소 권한(`packages: write`)으로 GHCR에 올리는 방식을 안다
- ☐ 운영 Docker 체크리스트(비root, 헬스체크, 볼륨, 재시작, 정상 종료, 리소스 제한, 로그 제한)를 점검할 수 있다
- ☐ 로그·요청 ID·헬스로 장애를 추적하는 절차를 설명할 수 있다
- ☐ **SQLite WAL에서 `.db` 파일만 복사하면 안 되는 이유**와 안전한 방법 두 가지를 안다
- ☐ 백업을 받은 뒤 **새 환경에 복원해 앱을 붙여 보았다**
- ☐ `cargo run --example s16_ops`로 `/metrics`·`/ready`·JSON 로그를 확인하고, `.db`만 복사하면 `no such table`이 나오는 것과 `VACUUM INTO` 백업이 복원되는 것을 직접 확인했다

---

## ⑧ 다음 단계 연결

운영 기반이 갖춰졌으니 [study17](study17.md)에서는 서비스에 **실시간 기능**을 붙인다: WebSocket 채팅, SSE 알림, 대용량 파일 스트리밍 업로드 — 그리고 연결이 오래 유지되는 기능이 서버 운영(종료·타임아웃·프록시)에 미치는 영향을 다룬다.
