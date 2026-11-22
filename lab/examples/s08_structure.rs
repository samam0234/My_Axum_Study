//! study8 — 구조화 · 재사용. 포트 3008.
//!
//! 한 단계에서 배우는 것만 담았다: 폴더 대신 **인라인 모듈**로 보는 경계(config / error / middleware / routes),
//! `Config`를 한 곳에서 읽고 **시작할 때 검증**, 미들웨어 **순서**(요청 ID가 패닉·타임아웃 응답에서 사라지는 버그를
//! 환경변수 하나로 재현·복구), `lib + bin`처럼 `app()`을 따로 두기.
//!
//! 실행:  cargo run --example s08_structure                          # 올바른 순서
//!        LAYER_ORDER=wrong cargo run --example s08_structure         # 요청 ID 계층을 맨 안쪽에 둔 잘못된 순서
//!        JWT_SECRET=short cargo run --example s08_structure          # 짧은 비밀키 → 부팅 거부

use axum::{Router, extract::DefaultBodyLimit, routing::get};
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
};

// ───────────────────────────────────────────────────────────────────────────
// config — 설정은 한 곳에서, 시작할 때 한 번에 읽고 검증한다
// ───────────────────────────────────────────────────────────────────────────
mod config {
    const DEV_JWT_SECRET: &str = "dev-only-insecure-secret-change-me";

    #[derive(Debug, Clone)]
    pub struct Config {
        pub bind_addr: String,
        pub jwt_secret: String,
        pub request_timeout_secs: u64,
        pub wrong_order: bool,
    }

    impl Config {
        /// 우선순위: 실제 환경변수 > 기본값. (`.env`는 dotenvy가 먼저 환경변수로 올려 준다 — demo/)
        pub fn from_env() -> anyhow::Result<Self> {
            let config = Self {
                bind_addr: env_or("BIND_ADDR", "127.0.0.1:3008"),
                jwt_secret: env_or("JWT_SECRET", DEV_JWT_SECRET),
                request_timeout_secs: env_or("REQUEST_TIMEOUT_SECS", "2").parse()?,
                wrong_order: env_or("LAYER_ORDER", "right") == "wrong",
            };
            config.validate()?;
            Ok(config)
        }

        /// 시작 시점에 잘못된 설정을 잡는다. 서버가 뜬 뒤에 발견하는 것보다 낫다.
        pub fn validate(&self) -> anyhow::Result<()> {
            if self.jwt_secret.len() < 16 {
                anyhow::bail!("JWT_SECRET이 너무 짧습니다 (최소 16자)");
            }
            if self.jwt_secret == DEV_JWT_SECRET {
                eprintln!("WARN 기본 JWT_SECRET을 쓰고 있습니다. 운영에서는 반드시 설정하세요.");
            }
            Ok(())
        }

        /// 시험 전용 설정 — 일반 빌드에는 포함되지 않는다.
        #[cfg(test)]
        pub fn for_test() -> Self {
            Self { bind_addr: "127.0.0.1:0".into(), jwt_secret: "test-secret-test-secret".into(), request_timeout_secs: 1, wrong_order: false }
        }
    }

    fn env_or(key: &str, default: &str) -> String {
        std::env::var(key).unwrap_or_else(|_| default.to_string())
    }
}

// ───────────────────────────────────────────────────────────────────────────
// error — 모든 오류 응답이 같은 모양. 핸들러는 `?`만 쓴다
// ───────────────────────────────────────────────────────────────────────────
mod error {
    use axum::{Json, http::StatusCode, response::{IntoResponse, Response}};
    use serde_json::json;

    #[derive(Debug, thiserror::Error)]
    pub enum AppError {
        #[error(transparent)]
        Internal(#[from] anyhow::Error),
    }
    pub type AppResult<T> = Result<T, AppError>;

    impl IntoResponse for AppError {
        fn into_response(self) -> Response {
            let Self::Internal(e) = &self;
            eprintln!("internal error: {e:?}"); // 원인은 로그에만, 응답은 고정 문구
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": { "code": "INTERNAL", "message": "서버 내부 오류가 발생했습니다." } }))).into_response()
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// middleware — 오류 본문에 requestId를 채우고, 우리 코드를 거치지 않은 4xx·5xx도 같은 모양으로
// ───────────────────────────────────────────────────────────────────────────
mod middleware {
    use axum::{
        body::{Body, to_bytes},
        extract::Request,
        http::{HeaderName, HeaderValue, StatusCode, header},
        middleware::Next,
        response::{IntoResponse, Response},
    };
    use serde_json::{Value, json};

    pub const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

    /// 핸들러는 요청 헤더를 모르므로 응답이 나가는 길목에서 requestId를 채운다.
    pub async fn error_envelope(request: Request, next: Next) -> Response {
        let request_id = request.headers().get(&REQUEST_ID).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let response = next.run(request).await;
        let status = response.status();
        if !(status.is_client_error() || status.is_server_error()) {
            return response; // 성공 응답은 건드리지 않는다
        }
        let (mut parts, body) = response.into_parts();
        let bytes = to_bytes(body, 64 * 1024).await.unwrap_or_default();
        let mut value: Value = serde_json::from_slice(&bytes)
            .ok()
            .filter(|v: &Value| v.get("error").is_some_and(Value::is_object)) // 이미 { error: {..} } 이면 그대로
            .unwrap_or_else(|| json!({ "error": { "code": code_for(status), "message": status.canonical_reason().unwrap_or("error") } })); // 빈 본문(408 등)은 새로 감싼다
        if let (Some(id), Some(error)) = (request_id, value.get_mut("error")) {
            error["requestId"] = Value::String(id);
        }
        parts.headers.remove(header::CONTENT_LENGTH); // 본문이 바뀌었으니 길이를 다시 계산하게
        parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Response::from_parts(parts, Body::from(value.to_string()))
    }

    fn code_for(status: StatusCode) -> &'static str {
        match status {
            StatusCode::NOT_FOUND => "NOT_FOUND",
            StatusCode::REQUEST_TIMEOUT => "REQUEST_TIMEOUT",
            StatusCode::PAYLOAD_TOO_LARGE => "PAYLOAD_TOO_LARGE",
            s if s.is_client_error() => "BAD_REQUEST",
            _ => "INTERNAL",
        }
    }

    /// 패닉을 연결 끊김이 아니라 500 JSON으로.
    pub fn on_panic(_: Box<dyn std::any::Any + Send + 'static>) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(json!({ "error": { "code": "INTERNAL", "message": "서버 내부 오류가 발생했습니다." } }))).into_response()
    }
}

// ───────────────────────────────────────────────────────────────────────────
// routes — 핸들러는 얇게
// ───────────────────────────────────────────────────────────────────────────
mod routes {
    use super::error::{AppError, AppResult};
    use std::time::Duration;

    pub async fn health() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({ "status": "ok" }))
    }
    pub async fn boom() -> AppResult<&'static str> {
        let _n: i32 = "not-a-number".parse::<i32>().map_err(|e| AppError::Internal(anyhow::anyhow!("설정 값 파싱 실패: {e}")))?;
        Ok("unreachable")
    }
    pub async fn panic_handler() -> &'static str {
        panic!("의도적으로 터뜨린 패닉");
    }
    pub async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_secs(10)).await; // TimeoutLayer보다 오래 걸린다
        "도달하지 못한다"
    }
}

/// 라우터 조립. main과 테스트가 **같은 함수**를 쓴다 (lib + bin 구조의 이유).
///
/// `.layer()`를 연달아 부르면 **나중에 붙인 것이 바깥**이다. (ServiceBuilder는 위가 바깥)
fn app(config: &config::Config) -> Router {
    let routes = Router::new()
        .route("/health", get(routes::health))
        .route("/boom", get(routes::boom))
        .route("/panic", get(routes::panic_handler))
        .route("/slow", get(routes::slow));

    let timeout = TimeoutLayer::with_status_code(axum::http::StatusCode::REQUEST_TIMEOUT, std::time::Duration::from_secs(config.request_timeout_secs));
    let propagate = PropagateRequestIdLayer::new(middleware::REQUEST_ID);
    let set_id = SetRequestIdLayer::new(middleware::REQUEST_ID, MakeRequestUuid);

    if config.wrong_order {
        // ❌ Propagate가 맨 안쪽. CatchPanic·Timeout은 아래 계층의 응답을 버리고 **자기가 새 응답을 만들기** 때문에
        //    그 새 응답은 Propagate를 거치지 않는다 → 패닉·타임아웃 응답에만 x-request-id가 빠진다
        routes
            .layer(propagate)
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
            .layer(timeout)
            .layer(CatchPanicLayer::custom(middleware::on_panic))
            .layer(axum::middleware::from_fn(middleware::error_envelope))
            .layer(CompressionLayer::new())
            .layer(set_id)
    } else {
        // ✅ 안쪽 → 바깥 순서로 적는다.
        routes
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024)) // 10. 가장 안쪽: 본문 크기 제한
            .layer(timeout) //                                8. 5초 넘는 요청을 408로
            .layer(CatchPanicLayer::custom(middleware::on_panic)) // 7. 패닉 → 500 JSON
            .layer(axum::middleware::from_fn(middleware::error_envelope)) // 6. 오류 본문에 requestId. CatchPanic·Timeout보다 바깥이어야 그 응답도 본다
            .layer(CompressionLayer::new()) // 5. error_envelope보다 바깥: 압축된 바이트는 JSON으로 읽을 수 없다
            .layer(propagate) //                              2. 요청 ID를 응답 헤더로. CatchPanic·Timeout보다 바깥이어야 한다
            .layer(set_id) //                                 1. 가장 바깥: 이후 모든 계층이 이 ID를 쓴다
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::from_env()?; // 검증 실패면 여기서 종료 (서버가 뜨기 전)
    println!("s08_structure  http://{}  (LAYER_ORDER={})", config.bind_addr, if config.wrong_order { "wrong" } else { "right" });
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    axum::serve(listener, app(&config)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_with(config: &config::Config, uri: &str) -> (u16, Option<String>, serde_json::Value) {
        let req = Request::builder().uri(uri).header("x-request-id", "rid-1").body(Body::empty()).unwrap();
        let res = app(config).oneshot(req).await.unwrap();
        let status = res.status().as_u16();
        let header = res.headers().get("x-request-id").map(|v| v.to_str().unwrap().to_string());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, header, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn 올바른_순서면_패닉과_타임아웃_응답에도_요청_id가_붙는다() {
        let config = config::Config::for_test();
        for (uri, want) in [("/health", 200), ("/panic", 500), ("/slow", 408), ("/nope", 404)] {
            let (status, header, body) = get_with(&config, uri).await;
            assert_eq!((status, header.as_deref()), (want, Some("rid-1")), "{uri}");
            if want >= 400 {
                assert_eq!(body["error"]["requestId"], "rid-1", "{uri}: 오류 본문의 requestId는 헤더와 같은 값");
            }
        }
    }

    #[tokio::test]
    async fn 잘못된_순서는_패닉과_타임아웃_응답에서만_헤더가_사라진다() {
        let config = config::Config { wrong_order: true, ..config::Config::for_test() };
        assert_eq!(get_with(&config, "/health").await.1.as_deref(), Some("rid-1")); // 평소엔 멀쩡해서 발견하기 어렵다
        assert_eq!(get_with(&config, "/panic").await.1, None);
        assert_eq!(get_with(&config, "/slow").await.1, None);
    }

    #[test]
    fn 짧은_비밀키는_시작을_거부한다() {
        let config = config::Config { jwt_secret: "short".into(), ..config::Config::for_test() };
        assert!(config.validate().is_err());
        assert!(config::Config::for_test().validate().is_ok());
    }
}
