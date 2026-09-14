mod auth;
mod db;
mod error;
mod extract;
mod middleware;
mod pagination;
mod posts;
mod state;
mod upload;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::{
    catch_panic::CatchPanicLayer,
    services::ServeDir,
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::error::{AppResult, handle_404, handle_405};
use crate::extract::{Path, Query};
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    // DATABASE_URL이 없으면 프로젝트 폴더의 data.db를 쓴다.
    // `mode=rwc` = read/write/create. (study9에서 .env 기반 설정으로 정리한다)
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data.db?mode=rwc".to_string());

    let pool = db::init(&database_url).await?;
    tracing::info!(%database_url, "database ready");

    // 업로드 폴더 준비 (study8)
    upload::ensure_dir().await?;

    // 학습용 기본값. 운영에서는 반드시 환경변수로 주입한다. (study9에서 설정 관리로 정리)
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "dev-only-insecure-secret-change-me".to_string());

    let app = app(AppState::new(pool, jwt_secret));

    // 0.7부터 hyper Server가 아니라 tokio TcpListener + axum::serve 조합을 쓴다.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app).await?;
    Ok(())
}

/// `RUST_LOG` 환경변수로 레벨을 조절한다. 없으면 기본값을 쓴다.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("demo=debug,tower_http=debug,axum::rejection=trace"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .init();
}

/// 라우터 조립만 따로 함수로 빼두면 나중에 통합 테스트에서 그대로 재사용할 수 있다. (study9)
fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/hello/{name}", get(hello))
        .route("/echo", get(echo))
        .route("/boom", get(boom))
        .route("/panic", get(panic_handler))
        .route("/slow", get(slow))
        // 게시판 라우터를 /posts 아래에 장착한다.
        .nest("/posts", posts::router())
        // 인증 라우터
        .nest("/auth", auth::router())
        // 업로드된 파일을 정적으로 서빙한다. (study8)
        // ServeDir은 Router가 아니라 tower Service라서 nest_service를 쓴다.
        .nest_service("/uploads", ServeDir::new(upload::UPLOAD_DIR))
        // 매칭되는 경로가 없을 때 (404)
        .fallback(handle_404)
        // 경로는 있는데 메서드가 다를 때 (405)
        .method_not_allowed_fallback(handle_405)
        // 여기서 상태를 주입하면 Router<AppState> → Router<()> 가 된다.
        .with_state(state)
        // ── 미들웨어 계층 ──────────────────────────────────────────────
        // ServiceBuilder는 "위에 적은 것이 바깥(먼저 실행)"으로 읽힌다.
        // (Router::layer를 직접 여러 번 호출하면 순서가 반대라 헷갈린다)
        .layer(
            ServiceBuilder::new()
                // 1. 요청 ID 부여 — 가장 바깥. 이후 모든 로그가 이 ID를 달 수 있다.
                .layer(SetRequestIdLayer::new(
                    middleware::REQUEST_ID_HEADER,
                    middleware::request_id_maker(),
                ))
                // 2. 요청 ID를 응답 헤더로 되돌려 보낸다.
                //    ⚠️ 반드시 CatchPanic/Timeout보다 **바깥**이어야 한다.
                //    안쪽에 두면 패닉·타임아웃으로 만들어진 응답에는 ID가 안 붙는다.
                .layer(PropagateRequestIdLayer::new(middleware::REQUEST_ID_HEADER))
                // 3. 요청/응답 로깅
                .layer(TraceLayer::new_for_http())
                // 4. 처리 시간 측정 헤더 (타임아웃으로 잘린 응답도 측정하려고 바깥에 둔다)
                .layer(axum::middleware::from_fn(middleware::response_time))
                // 5. 패닉을 500 JSON으로 (연결이 끊기지 않게)
                .layer(CatchPanicLayer::custom(middleware::on_panic))
                // 6. 타임아웃 — 초과 시 돌려줄 상태 코드를 명시한다.
                //    (tower-http 0.7의 `TimeoutLayer::new`는 deprecated)
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    middleware::REQUEST_TIMEOUT,
                ))
                // 7. 응답 압축 (Accept-Encoding을 보고 알아서 판단)
                .layer(CompressionLayer::new())
                // 8. CORS
                .layer(cors_layer())
                // 9. 요청 본문 크기 제한 — 가장 안쪽. extractor 바로 앞.
                .layer(DefaultBodyLimit::max(middleware::BODY_LIMIT_BYTES)),
        )
}

/// CORS 설정.
///
/// `CorsLayer::permissive()`는 편하지만 **운영에서 쓰면 안 된다.**
/// 허용할 출처/메서드/헤더를 명시적으로 좁힌다.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin("http://localhost:5173".parse::<HeaderValue>().unwrap())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .max_age(Duration::from_secs(3600))
}

/// 가장 단순한 핸들러: &'static str도 IntoResponse를 구현하므로 그대로 반환할 수 있다.
/// → 200 OK, Content-Type: text/plain; charset=utf-8
async fn root() -> &'static str {
    "Hello, Axum!"
}

/// (StatusCode, Json<T>) 튜플로 상태 코드와 본문을 함께 지정한다.
async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

/// 경로 파라미터: axum 0.8부터 `:name`이 아니라 `{name}` 문법을 쓴다.
async fn hello(Path(name): Path<String>) -> String {
    format!("Hello, {name}!")
}

#[derive(Debug, Deserialize)]
struct EchoParams {
    /// 필수 파라미터. 없으면 400.
    message: String,
    /// Option<T>로 선언하면 선택 파라미터가 된다.
    times: Option<u8>,
}

#[derive(Debug, Serialize)]
struct EchoResponse {
    original: String,
    repeated: Vec<String>,
}

/// 쿼리스트링: GET /echo?message=hi&times=3
async fn echo(Query(params): Query<EchoParams>) -> Json<EchoResponse> {
    let times = params.times.unwrap_or(1);
    let repeated = (0..times).map(|_| params.message.clone()).collect();

    Json(EchoResponse {
        original: params.message,
        repeated,
    })
}

/// 500 응답 확인용 엔드포인트.
/// `anyhow::Error`가 `?`로 `AppError::Internal`로 자동 변환되는 것을 보여준다.
async fn boom() -> AppResult<&'static str> {
    let raw = "not-a-number";
    // ParseIntError → anyhow::Error → AppError::Internal
    let _n: i32 = raw
        .parse()
        .map_err(|e: std::num::ParseIntError| anyhow::anyhow!("설정 값 파싱 실패: {e}"))?;
    Ok("unreachable")
}

/// CatchPanicLayer 확인용. 패닉해도 연결이 끊기지 않고 500 JSON이 나가야 한다.
async fn panic_handler() -> &'static str {
    panic!("의도적으로 터뜨린 패닉");
}

/// TimeoutLayer 확인용. REQUEST_TIMEOUT보다 오래 걸린다.
async fn slow() -> &'static str {
    tokio::time::sleep(Duration::from_secs(10)).await;
    "이 응답은 도달하지 못한다"
}
