//! study9 — 테스트 · 디버깅. 포트 3009.
//!
//! 한 단계에서 배우는 것만 담았다: 같은 파일의 **단위 테스트**, 서버를 띄우지 않는 **`oneshot` 요청 테스트**,
//! 시험마다 새 상태(격리), 상태를 조금 바꿔 만드는 `test_app_with`, 실패를 읽는 법(`--nocapture`, 한글 시험 이름).
//! 이 파일의 대상은 일부러 단순하게 만든 "메모(memo)" API다. (DB·외부 API를 가짜로 바꾸는 trait 경계는 src/mock_demo.rs)
//!
//! 실행:   cargo run --example s09_testing
//! 시험:   cargo test --example s09_testing                 # 이 파일의 시험만
//!         cargo test --example s09_testing -- --nocapture  # println!/로그를 그대로 본다

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

// ── 순수 함수: 가장 싸게 시험할 수 있는 부분은 따로 뺀다 ───────────────────────────
/// 전체 건수와 한 페이지 크기로 페이지 수를 구한다. 올림 나눗셈이다.
fn total_pages(total: u64, limit: u64) -> u64 {
    total.div_ceil(limit.max(1)) // limit가 0이어도 0으로 나누지 않는다 — 경계값은 시험에서 꼭 확인한다
}

// ── 앱 ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Default)]
struct AppState {
    memos: Arc<Mutex<Vec<Memo>>>,
    max_len: usize, // 시험에서 바꿔 끼울 수 있는 설정
}

#[derive(Clone, Serialize)]
struct Memo {
    id: usize,
    text: String,
}

#[derive(Deserialize)]
struct NewMemo {
    text: String,
}

enum AppError {
    TooLong(usize),
    NotFound(usize),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::TooLong(max) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", format!("메모는 {max}자 이하여야 합니다")),
            Self::NotFound(id) => (StatusCode::NOT_FOUND, "NOT_FOUND", format!("메모 {id}을(를) 찾을 수 없습니다")),
        };
        (status, Json(json!({ "error": { "code": code, "message": message } }))).into_response()
    }
}

async fn create(State(state): State<AppState>, Json(body): Json<NewMemo>) -> Result<impl IntoResponse, AppError> {
    if body.text.chars().count() > state.max_len {
        return Err(AppError::TooLong(state.max_len));
    }
    let mut memos = state.memos.lock().unwrap();
    let memo = Memo { id: memos.len() + 1, text: body.text };
    memos.push(memo.clone());
    Ok((StatusCode::CREATED, Json(json!({ "data": memo }))))
}

async fn get_one(State(state): State<AppState>, Path(id): Path<usize>) -> Result<Json<serde_json::Value>, AppError> {
    let memos = state.memos.lock().unwrap();
    let memo = memos.iter().find(|m| m.id == id).ok_or(AppError::NotFound(id))?;
    Ok(Json(json!({ "data": memo })))
}

async fn list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let memos = state.memos.lock().unwrap();
    Json(json!({ "data": *memos, "meta": { "total": memos.len(), "totalPages": total_pages(memos.len() as u64, 10) } }))
}

/// main과 테스트가 **같은 `app()`** 을 쓴다. (lib + bin 구조를 단일 파일에서 흉내 낸 것)
fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/memos", get(list).post(create))
        .route("/api/memos/{id}", get(get_one))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3009").await?;
    println!("s09_testing  http://127.0.0.1:3009/api/memos");
    axum::serve(listener, app(AppState { max_len: 20, ..Default::default() })).await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────────
// 시험은 같은 파일 안에 둔다. #[cfg(test)]라서 일반 빌드에는 포함되지 않고, 비공개 항목에도 접근한다.
// ───────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, header},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt; // oneshot

    // ── 단위 시험: 함수 이름을 한글 문장으로 쓰면 실패 로그가 곧 명세서가 된다 ──
    #[test]
    fn 페이지_수는_올림_계산이다() {
        assert_eq!(total_pages(25, 10), 3); // 2.5가 아니라 3
        assert_eq!(total_pages(30, 10), 3);
        assert_eq!(total_pages(0, 10), 0); // 경계값: 결과 없음
        assert_eq!(total_pages(5, 0), 5); // 경계값: limit 0이어도 패닉하지 않는다
    }

    // ── 요청 시험 헬퍼: (상태 코드, JSON)만 돌려주면 대부분의 시험이 한 줄로 끝난다 ──
    fn test_app() -> Router {
        // 시험마다 새 상태를 만든다 → 서로 간섭하지 않고 병렬로 돌아간다
        app(AppState { max_len: 20, ..Default::default() })
    }

    /// 상태를 조금 바꿔서 앱을 만든다. (예: 길이 제한을 줄인다)
    fn test_app_with(tweak: impl FnOnce(AppState) -> AppState) -> Router {
        app(tweak(AppState { max_len: 20, ..Default::default() }))
    }

    async fn request(app: &Router, method: &str, uri: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(json) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        // oneshot은 서비스를 소비한다. Router의 Clone은 싸니 복제해서 넘긴다. 서버도 포트도 필요 없다.
        let response = app.clone().oneshot(builder.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes(); // 본문은 스트림이라 끝까지 모은다
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    // ── 요청 시험: 한 줄이 한 가지 약속이다 ──
    #[tokio::test]
    async fn 메모를_만들면_201과_data로_돌아온다() {
        let app = test_app();
        let (status, body) = request(&app, "POST", "/api/memos", Some(json!({ "text": "첫 메모" }))).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["data"]["id"], 1);
        assert_eq!(body["data"]["text"], "첫 메모");
    }

    #[tokio::test]
    async fn 없는_메모는_404_json이다() {
        let (status, body) = request(&test_app(), "GET", "/api/memos/999", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn 길이_제한은_상태를_바꿔서_시험한다() {
        let app = test_app_with(|s| AppState { max_len: 3, ..s });
        let (status, body) = request(&app, "POST", "/api/memos", Some(json!({ "text": "네글자!" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");
        // 경계값: 정확히 3자는 통과한다
        let (status, _) = request(&app, "POST", "/api/memos", Some(json!({ "text": "세글자" }))).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn 시험마다_상태가_격리된다() {
        let a = test_app();
        request(&a, "POST", "/api/memos", Some(json!({ "text": "a" }))).await;
        // 다른 앱(다른 시험)에는 보이지 않는다
        let (_, body) = request(&test_app(), "GET", "/api/memos", None).await;
        assert_eq!(body["meta"]["total"], 0);
        let (_, body) = request(&a, "GET", "/api/memos", None).await;
        assert_eq!((body["meta"]["total"].as_u64(), body["meta"]["totalPages"].as_u64()), (Some(1), Some(1)));
    }

    #[tokio::test]
    async fn 깨진_json은_서버가_아니라_extractor가_400으로_막는다() {
        let req = Request::builder()
            .method("POST")
            .uri("/api/memos")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{bad"))
            .unwrap();
        let response = test_app().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST); // 핸들러는 호출되지도 않았다
    }
}
