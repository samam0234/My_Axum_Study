//! Axum 학습용 게시판 API.
//!
//! `main.rs`(실행 파일)와 `tests/`(통합 테스트)가 **같은 라우터를 공유**하도록
//! 앱 조립 로직을 라이브러리 크레이트에 둔다. 이게 Rust 프로젝트의 표준 구조다.

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod extract;
pub mod middleware;
pub mod openapi;
pub mod pagination;
pub mod posts;
pub mod routes;
pub mod state;
pub mod upload;

use std::time::Duration;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, StatusCode, header},
    routing::get,
};
use tower::ServiceBuilder;
use tower_http::{
    catch_panic::CatchPanicLayer, compression::CompressionLayer, cors::CorsLayer,
    request_id::{PropagateRequestIdLayer, SetRequestIdLayer}, services::ServeDir,
    timeout::TimeoutLayer, trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::error::{handle_404, handle_405};
use crate::state::AppState;

/// `RUST_LOG` 환경변수로 레벨을 조절한다. 없으면 기본값을 쓴다.
///
/// 테스트에서 여러 번 불려도 안전하도록 `try_init`을 쓴다.
/// (`init`은 두 번째 호출에서 패닉한다)
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("demo=debug,tower_http=debug,axum::rejection=trace"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .try_init();
}

/// 라우터 조립. `main`에서도, 통합 테스트에서도 **이 함수 하나**를 쓴다.
pub fn app(state: AppState) -> Router {
    // Arc<String> → String (ServeDir은 AsRef<Path>를 요구한다)
    let upload_dir = state.upload_dir.as_str().to_string();

    Router::new()
        // Swagger UI: http://127.0.0.1:3000/docs
        // OpenAPI JSON: http://127.0.0.1:3000/api-docs/openapi.json
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", openapi::ApiDoc::openapi()))
        .route("/", get(routes::root))
        .route("/health", get(routes::health))
        .route("/hello/{name}", get(routes::hello))
        .route("/echo", get(routes::echo))
        .route("/boom", get(routes::boom))
        .route("/panic", get(routes::panic_handler))
        .route("/slow", get(routes::slow))
        // 게시판 라우터를 /posts 아래에 장착한다.
        .nest("/posts", posts::router())
        // 인증 라우터
        .nest("/auth", auth::router())
        // 업로드된 파일을 정적으로 서빙한다. (study8)
        // ServeDir은 Router가 아니라 tower Service라서 nest_service를 쓴다.
        .nest_service("/uploads", ServeDir::new(upload_dir))
        // 매칭되는 경로가 없을 때 (404)
        .fallback(handle_404)
        // 경로는 있는데 메서드가 다를 때 (405)
        .method_not_allowed_fallback(handle_405)
        // 여기서 상태를 주입하면 Router<AppState> → Router<()> 가 된다.
        .with_state(state)
        // ── 미들웨어 계층 ──────────────────────────────────────────────
        // ServiceBuilder는 "위에 적은 것이 바깥(먼저 실행)"으로 읽힌다.
        .layer(
            ServiceBuilder::new()
                // 1. 요청 ID 부여 — 가장 바깥. 이후 모든 로그가 이 ID를 달 수 있다.
                .layer(SetRequestIdLayer::new(
                    middleware::REQUEST_ID_HEADER,
                    middleware::request_id_maker(),
                ))
                // 2. 요청 ID를 응답 헤더로 되돌려 보낸다.
                //    ⚠️ 반드시 CatchPanic/Timeout보다 **바깥**이어야 한다.
                .layer(PropagateRequestIdLayer::new(middleware::REQUEST_ID_HEADER))
                // 3. 요청/응답 로깅
                .layer(TraceLayer::new_for_http())
                // 4. 처리 시간 측정 헤더
                .layer(axum::middleware::from_fn(middleware::response_time))
                // 5. 패닉을 500 JSON으로 (연결이 끊기지 않게)
                .layer(CatchPanicLayer::custom(middleware::on_panic))
                // 6. 타임아웃
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    middleware::REQUEST_TIMEOUT,
                ))
                // 7. 응답 압축
                .layer(CompressionLayer::new())
                // 8. CORS
                .layer(cors_layer())
                // 9. 요청 본문 크기 제한 — 가장 안쪽
                .layer(DefaultBodyLimit::max(middleware::BODY_LIMIT_BYTES)),
        )
}

/// CORS 설정.
///
/// `CorsLayer::permissive()`는 편하지만 **운영에서 쓰면 안 된다.**
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
