//! study13 — 외부 서비스 연동: 웹훅(결제) · 메일 · 소셜 로그인(OAuth2).
//!
//! 실행: `cargo run --example s13_external`  →  http://127.0.0.1:3013
//!
//! 진짜 결제사·메일사·OAuth 제공자 계정 없이도 **흐름 전체**를 돌려 볼 수 있도록
//! 같은 프로세스 안에 가짜 OAuth 제공자(`/mock-oauth/*`)를 함께 둔다.
//! 실제 서비스(GitHub·Google 등)로 바꿀 때는 환경변수의 URL·키만 바꾼다.
//!
//! 환경변수 (모두 선택):
//!   WEBHOOK_SECRET       웹훅 서명 검증 키 (기본 dev-webhook-secret)
//!   MAIL_API_URL         설정하면 HTTP 메일 API로 보내고, 없으면 로그만 남긴다
//!   OAUTH_CLIENT_ID / OAUTH_CLIENT_SECRET / OAUTH_AUTHORIZE_URL / OAUTH_TOKEN_URL / OAUTH_USER_URL

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Form, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;
const BASE: &str = "http://127.0.0.1:3013";

#[derive(Clone)]
struct AppState {
    webhook_secret: Arc<String>,
    /// 이미 처리한 웹훅 이벤트 id. 결제사는 같은 이벤트를 **여러 번 보낼 수 있다**(재시도).
    seen_events: Arc<Mutex<HashSet<String>>>,
    http: reqwest::Client,
    mail_api_url: Option<Arc<String>>,
    oauth: Arc<OAuthConfig>,
    /// 진행 중인 로그인의 state 값. 실무에서는 쿠키나 세션에 둔다.
    pending_states: Arc<Mutex<HashSet<String>>>,
}

struct OAuthConfig {
    client_id: String,
    client_secret: String,
    authorize_url: String,
    token_url: String,
    user_url: String,
    redirect_uri: String,
}

#[tokio::main]
async fn main() {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());

    let state = AppState {
        webhook_secret: Arc::new(env("WEBHOOK_SECRET", "dev-webhook-secret")),
        seen_events: Default::default(),
        http: reqwest::Client::new(),
        mail_api_url: std::env::var("MAIL_API_URL").ok().map(Arc::new),
        oauth: Arc::new(OAuthConfig {
            client_id: env("OAUTH_CLIENT_ID", "dev-client"),
            client_secret: env("OAUTH_CLIENT_SECRET", "dev-secret"),
            authorize_url: env("OAUTH_AUTHORIZE_URL", &format!("{BASE}/mock-oauth/authorize")),
            token_url: env("OAUTH_TOKEN_URL", &format!("{BASE}/mock-oauth/token")),
            user_url: env("OAUTH_USER_URL", &format!("{BASE}/mock-oauth/user")),
            redirect_uri: format!("{BASE}/auth/callback"),
        }),
        pending_states: Default::default(),
    };

    let app = Router::new()
        // ① 결제 웹훅
        .route("/webhooks/payment", post(payment_webhook))
        // ② 메일
        .route("/api/mail/welcome", post(send_welcome))
        // ③ 소셜 로그인
        .route("/auth/login", get(oauth_login))
        .route("/auth/callback", get(oauth_callback))
        // 가짜 OAuth 제공자 (학습용)
        .route("/mock-oauth/authorize", get(mock_authorize))
        .route("/mock-oauth/token", post(mock_token))
        .route("/mock-oauth/user", get(mock_user))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3013").await.unwrap();
    println!("{BASE}  (소셜 로그인 시작: {BASE}/auth/login)");
    axum::serve(listener, app).await.unwrap();
}

// ═══ ① 웹훅 ═════════════════════════════════════════════════════════════════
//
// 웹훅은 **외부가 우리를 부르는** 요청이다. 누구나 URL만 알면 흉내 낼 수 있으므로
// "결제사가 보낸 게 맞다"를 서명으로 증명해야 한다. 서명은 **받은 그대로의 바이트**로 계산한다.

/// 서명 검증. `Mac::verify_slice`는 **상수 시간 비교**라서 타이밍 공격에 안전하다.
/// (`==`로 문자열을 비교하면 앞에서부터 달라지는 지점까지의 시간 차이가 새어 나간다)
fn verify_signature(secret: &str, body: &[u8], header_value: &str) -> bool {
    let Some(hex_sig) = header_value.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(sig) = hex::decode(hex_sig) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

async fn payment_webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    // ⚠️ `Json<T>` extractor를 쓰지 않는다. 파싱된 값을 다시 직렬화하면 바이트가 달라져 서명이 안 맞는다.
    //    먼저 `Bytes`로 원문을 받고, 검증이 끝난 뒤에 JSON으로 읽는다.
    let signature = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !verify_signature(&state.webhook_secret, &body, signature) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": { "code": "UNAUTHORIZED", "message": "서명이 올바르지 않습니다." } }))).into_response();
    }

    let Ok(event) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let event_id = event["id"].as_str().unwrap_or_default().to_string();

    // 멱등성: 같은 이벤트가 다시 와도 **한 번만** 처리한다. (두 번 재고를 깎거나 두 번 메일을 보내면 안 된다)
    if !state.seen_events.lock().unwrap().insert(event_id.clone()) {
        return (StatusCode::OK, Json(json!({ "status": "duplicate-ignored" }))).into_response();
    }

    println!("[webhook] {} amount={}", event["type"], event["amount"]);
    // 웹훅은 **빨리 2xx로 답해야** 한다(보통 몇 초 안). 오래 걸리는 일은 큐/백그라운드로 넘긴다.
    (StatusCode::OK, Json(json!({ "status": "processed" }))).into_response()
}

// ═══ ② 메일 ═════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct WelcomeMail {
    to: String,
}

/// 메일 전송은 느리고 실패한다. **요청 스레드에서 기다리지 않고** 백그라운드로 보낸다.
async fn send_welcome(State(state): State<AppState>, Json(body): Json<WelcomeMail>) -> Response {
    if !body.to.contains('@') {
        return StatusCode::BAD_REQUEST.into_response();
    }

    tokio::spawn(async move {
        let payload = json!({ "to": body.to, "subject": "가입을 환영합니다", "text": "환영합니다!" });
        match &state.mail_api_url {
            // 실제 메일 API(SendGrid·Resend·SES 등)는 보통 "Bearer 키 + JSON POST" 모양이다.
            Some(url) => {
                let key = std::env::var("MAIL_API_KEY").unwrap_or_default();
                match state.http.post(url.as_str()).bearer_auth(key).json(&payload).send().await {
                    Ok(res) if res.status().is_success() => println!("[mail] 전송 성공"),
                    Ok(res) => eprintln!("[mail] 실패 status={}", res.status()),
                    Err(e) => eprintln!("[mail] 네트워크 오류: {e}"),
                }
            }
            None => println!("[mail] (MAIL_API_URL 없음 → 로그만) {payload}"),
        }
    });

    // 202 Accepted: "접수했다. 처리는 아직"
    StatusCode::ACCEPTED.into_response()
}

// ═══ ③ OAuth2 Authorization Code ════════════════════════════════════════════
//
//  브라우저 ──(1) /auth/login──▶ 우리 서버 ──(2) 302 제공자 로그인 화면(+state)──▶ 브라우저
//  브라우저 ──(3) 제공자에서 동의──▶ 제공자 ──(4) 302 /auth/callback?code&state──▶ 브라우저
//  우리 서버 ──(5) code + client_secret으로 토큰 교환──▶ 제공자 ──(6) access_token──▶ 우리 서버
//  우리 서버 ──(7) access_token으로 사용자 정보 조회 → 우리 쪽 로그인 처리

async fn oauth_login(State(state): State<AppState>) -> Redirect {
    // state: 로그인을 시작한 사람이 곧 콜백을 받는 사람임을 확인하는 일회용 값 (CSRF 방지)
    let st = uuid::Uuid::new_v4().to_string();
    state.pending_states.lock().unwrap().insert(st.clone());

    let c = &state.oauth;
    Redirect::to(&format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=read:user&state={}",
        c.authorize_url,
        c.client_id,
        urlencode(&c.redirect_uri),
        st
    ))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn oauth_callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Response {
    // 1) state 검증: 우리가 발급한 값이어야 하고, 한 번만 쓸 수 있다.
    if !state.pending_states.lock().unwrap().remove(&q.state) {
        return (StatusCode::BAD_REQUEST, "state가 올바르지 않습니다 (CSRF 의심)").into_response();
    }

    let c = &state.oauth;

    // 2) code → access_token. 이 호출은 **서버끼리** 하므로 client_secret이 브라우저에 노출되지 않는다.
    let token: Value = match state
        .http
        .post(&c.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", q.code.as_str()),
            ("client_id", c.client_id.as_str()),
            ("client_secret", c.client_secret.as_str()),
            ("redirect_uri", c.redirect_uri.as_str()),
        ])
        .send()
        .await
        .and_then(|r| r.error_for_status())
    {
        Ok(res) => res.json().await.unwrap_or_default(),
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("토큰 교환 실패: {e}")).into_response(),
    };
    let Some(access_token) = token["access_token"].as_str() else {
        return (StatusCode::BAD_GATEWAY, "토큰 응답에 access_token이 없습니다").into_response();
    };

    // 3) 사용자 정보 조회. 여기서 받은 id/email로 우리 DB의 회원을 찾거나 만든다.
    let user: Value = match state.http.get(&c.user_url).bearer_auth(access_token).send().await {
        Ok(res) => res.json().await.unwrap_or_default(),
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("사용자 조회 실패: {e}")).into_response(),
    };

    // 4) 우리 쪽 로그인 처리(세션 쿠키 발급 등)는 study7의 방식을 그대로 쓴다.
    (
        [(header::SET_COOKIE, "sid=oauth-session; HttpOnly; SameSite=Lax; Path=/")],
        Json(json!({ "data": { "provider": "mock", "user": user } })),
    )
        .into_response()
}

/// 리다이렉트 URL에 넣을 값의 최소 인코딩. (실무에서는 `url` 크레이트 등을 쓴다)
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ── 가짜 OAuth 제공자 ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AuthorizeQuery {
    redirect_uri: String,
    state: String,
}

/// 실제 제공자라면 로그인·동의 화면이 뜬다. 여기서는 바로 승인한 것으로 치고 code를 붙여 돌려보낸다.
async fn mock_authorize(Query(q): Query<AuthorizeQuery>) -> Redirect {
    Redirect::to(&format!("{}?code=mock-code-123&state={}", q.redirect_uri, q.state))
}

#[derive(Deserialize)]
struct TokenForm {
    code: String,
    client_secret: String,
}

async fn mock_token(Form(f): Form<TokenForm>) -> Response {
    if f.code == "mock-code-123" && f.client_secret == "dev-secret" {
        Json(json!({ "access_token": "mock-token-abc", "token_type": "bearer" })).into_response()
    } else {
        StatusCode::BAD_REQUEST.into_response()
    }
}

async fn mock_user(headers: HeaderMap) -> Response {
    if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) == Some("Bearer mock-token-abc") {
        Json(json!({ "id": 12345, "login": "mina", "email": "mina@example.com" })).into_response()
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn 올바른_서명은_통과한다() {
        let body = br#"{"id":"evt_1","type":"payment.succeeded","amount":1000}"#;
        assert!(verify_signature("s3cret", body, &sign("s3cret", body)));
    }

    #[test]
    fn 본문이_바뀌면_서명이_깨진다() {
        let sig = sign("s3cret", br#"{"amount":1000}"#);
        assert!(!verify_signature("s3cret", br#"{"amount":9999}"#, &sig));
    }

    #[test]
    fn 키가_다르거나_형식이_틀리면_거부한다() {
        let body = b"{}";
        assert!(!verify_signature("other", body, &sign("s3cret", body)));
        assert!(!verify_signature("s3cret", body, "not-a-signature"));
        assert!(!verify_signature("s3cret", body, "sha256=zzzz"));
    }
}
