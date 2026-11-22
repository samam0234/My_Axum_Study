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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_text(app: &Router, uri: &str) -> (StatusCode, String) {
        let res = app.clone().oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        (status, String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap())
    }

    fn test_state(db: SqlitePool) -> AppState {
        AppState { db, metrics: Arc::default() }
    }

    #[tokio::test]
    async fn 메트릭은_상태_계열별로_요청을_센다() {
        let app = app(test_state(connect("sqlite::memory:").await.unwrap()));
        get_text(&app, "/health").await; // 2xx
        get_text(&app, "/api/notes").await; // 2xx
        get_text(&app, "/nope").await; // 4xx
        let (_, text) = get_text(&app, "/metrics").await;
        assert!(text.contains("http_requests_total{class=\"2xx\"} 2"), "{text}");
        assert!(text.contains("http_requests_total{class=\"4xx\"} 1"), "{text}");
        assert!(text.contains("http_requests_total{class=\"5xx\"} 0"), "{text}");
        assert!(text.contains("http_requests_in_flight 1"), "측정 중인 /metrics 요청 자신이 처리 중이다: {text}");
    }

    #[tokio::test]
    async fn liveness는_db와_무관하고_readiness는_db가_죽으면_503이다() {
        let db = connect("sqlite::memory:").await.unwrap();
        let app = app(test_state(db.clone()));
        assert_eq!(get_text(&app, "/ready").await.0, StatusCode::OK);

        db.close().await; // DB 연결이 끊긴 상황
        assert_eq!(get_text(&app, "/health").await.0, StatusCode::OK, "프로세스는 살아 있다 → 재시작할 이유가 없다");
        assert_eq!(get_text(&app, "/ready").await.0, StatusCode::SERVICE_UNAVAILABLE, "요청을 받을 수 없다 → 트래픽에서 뺀다");
    }

    #[tokio::test]
    async fn db_파일만_복사하면_빈_db고_vacuum_into는_복원된다() {
        let dir = std::env::temp_dir().join(format!("s16_ops_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.db");

        // WAL 모드 DB에 3건을 쓴다. 풀이 열려 있는 동안 최근 쓰기는 src.db-wal에 있다
        let db = connect(&format!("sqlite://{}?mode=rwc", src.display())).await.unwrap();
        for text in ["a", "b", "c"] {
            sqlx::query("INSERT INTO notes (text) VALUES ($1)").bind(text).execute(&db).await.unwrap();
        }

        // ❌ 흔한 실수: .db 파일만 복사
        let copy = dir.join("copy.db");
        std::fs::copy(&src, &copy).unwrap();
        let broken = SqlitePoolOptions::new().connect_with(SqliteConnectOptions::new().filename(&copy)).await.unwrap();
        let result = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notes").fetch_one(&broken).await;
        assert!(result.is_err(), "테이블이 없는 DB가 된다 (최근 쓰기는 -wal에 있었다): {result:?}");
        broken.close().await;

        // ✅ VACUUM INTO: 서버를 멈추지 않고 일관된 스냅샷 한 파일. 복원해서 3건을 확인한다
        let backup = dir.join("backup.db");
        backup_to(&db, &backup).await.unwrap();
        assert_eq!(verify_restore(&backup).await.unwrap(), 3);

        db.close().await;
        std::fs::remove_dir_all(&dir).ok();
    }
}
