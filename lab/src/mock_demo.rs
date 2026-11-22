//! study9 — 테스트에서 DB·외부 API를 가짜(mock/fake)로 바꾸기.
//!
//! 핵심 아이디어: **핸들러가 구체 타입(sqlx, reqwest)이 아니라 trait에 의존**하게 만든다.
//! - 운영:   `SqlRepo`, `HttpNotifier`   (진짜 DB, 진짜 HTTP)
//! - 테스트: `MemRepo`, `FakeNotifier`   (메모리, 호출 기록)
//!
//! "무엇으로 갈아 끼울지"를 **타입 파라미터**로 정한다.
//! DI 컨테이너가 없으므로 "무엇을 주입할지"는 `router::<R, N>(...)`을 부르는 쪽이 정한다.

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

// ── 경계(trait) ───────────────────────────────────────────────────────────────

/// DB 접근 경계. 운영 구현은 sqlx 풀을 들고 있고, 테스트 구현은 `Vec`를 들고 있다.
pub trait OrderRepo: Clone + Send + Sync + 'static {
    fn insert(&self, item: String, qty: u32) -> impl Future<Output = Result<u32, String>> + Send;
}

/// 외부 API 경계(메일·슬랙·결제 알림 등).
pub trait Notifier: Clone + Send + Sync + 'static {
    fn notify(&self, message: String) -> impl Future<Output = Result<(), String>> + Send;
}

// ── 앱 ──────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState<R, N> {
    pub repo: R,
    pub notifier: N,
}

#[derive(Deserialize)]
pub struct NewOrder {
    pub item: String,
    pub qty: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderCreated {
    pub id: u32,
    /// 알림 전송 성공 여부. 알림이 실패해도 주문 자체는 성공으로 본다.
    pub notified: bool,
}

pub fn router<R: OrderRepo, N: Notifier>(state: AppState<R, N>) -> Router {
    Router::new().route("/orders", post(create_order::<R, N>)).with_state(state)
}

async fn create_order<R: OrderRepo, N: Notifier>(
    State(state): State<AppState<R, N>>,
    Json(body): Json<NewOrder>,
) -> Response {
    if body.item.trim().is_empty() || body.qty == 0 {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let id = match state.repo.insert(body.item.clone(), body.qty).await {
        Ok(id) => id,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // 외부 서비스 장애가 핵심 기능(주문 저장)을 막으면 안 된다. 실패는 기록만 하고 넘어간다.
    let notified = state
        .notifier
        .notify(format!("새 주문 #{id}: {} x {}", body.item, body.qty))
        .await
        .is_ok();

    (StatusCode::CREATED, Json(OrderCreated { id, notified })).into_response()
}

// ── 테스트용 가짜 구현 ─────────────────────────────────────────────────────────

/// 메모리 저장소. 실제 DB 대신 쓴다.
#[derive(Clone, Default)]
pub struct MemRepo {
    pub rows: Arc<Mutex<Vec<(u32, String, u32)>>>,
}

impl OrderRepo for MemRepo {
    async fn insert(&self, item: String, qty: u32) -> Result<u32, String> {
        let mut rows = self.rows.lock().unwrap();
        let id = rows.len() as u32 + 1;
        rows.push((id, item, qty));
        Ok(id)
    }
}

/// 호출을 기록하는 가짜 알림기. `fail`을 켜면 외부 서비스 장애를 흉내 낸다.
#[derive(Clone, Default)]
pub struct FakeNotifier {
    pub sent: Arc<Mutex<Vec<String>>>,
    pub fail: bool,
}

impl Notifier for FakeNotifier {
    async fn notify(&self, message: String) -> Result<(), String> {
        if self.fail {
            return Err("외부 서비스 장애".into());
        }
        self.sent.lock().unwrap().push(message);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt; // oneshot

    async fn post_order(app: Router, json: &str) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri("/orders")
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn 주문하면_저장되고_알림이_간다() {
        let repo = MemRepo::default();
        let notifier = FakeNotifier::default();
        let app = router(AppState { repo: repo.clone(), notifier: notifier.clone() });

        let (status, body) = post_order(app, r#"{"item":"keyboard","qty":2}"#).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["notified"], true);
        // "무엇이 저장됐나", "무엇이 외부로 나갔나"를 직접 검사할 수 있다 — 진짜 DB·네트워크가 없어도.
        assert_eq!(repo.rows.lock().unwrap().len(), 1);
        assert_eq!(notifier.sent.lock().unwrap()[0], "새 주문 #1: keyboard x 2");
    }

    #[tokio::test]
    async fn 알림이_실패해도_주문은_성공한다() {
        let notifier = FakeNotifier { fail: true, ..Default::default() };
        let app = router(AppState { repo: MemRepo::default(), notifier });

        let (status, body) = post_order(app, r#"{"item":"mouse","qty":1}"#).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["notified"], false);
    }

    #[tokio::test]
    async fn 잘못된_입력은_저장도_알림도_하지_않는다() {
        let repo = MemRepo::default();
        let notifier = FakeNotifier::default();
        let app = router(AppState { repo: repo.clone(), notifier: notifier.clone() });

        let (status, _) = post_order(app, r#"{"item":"","qty":0}"#).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(repo.rows.lock().unwrap().is_empty());
        assert!(notifier.sent.lock().unwrap().is_empty());
    }
}
