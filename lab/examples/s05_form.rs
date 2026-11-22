//! study5 — 폼 · 입력 · 검증 · 중복 제출 방지.
//!
//! 실행: `cargo run --example s05_form`  →  http://127.0.0.1:3005
//!
//! 1. 서버 검증:     `validator` 규칙 → 400 + `error.fields` (공통 API 스펙 v1 §5 모양)
//! 2. 상태 코드 분기: 201(성공) / 400(입력 오류) / 409(이메일 중복) 에 따라 화면이 달라진다
//! 3. 중복 제출 방지: 버튼 비활성화(클라이언트) + `Idempotency-Key` 재생(서버)
//! 4. 비교:          검증 없는 기본 `Json<T>`는 타입이 틀리면 **422**를 준다 → `/api/signup-raw`

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{FromRequest, Request, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
struct Signup {
    #[validate(email(message = "이메일 형식이 아닙니다."))]
    email: String,
    #[validate(length(min = 8, max = 72, message = "비밀번호는 8~72자로 입력하세요."))]
    password: String,
    #[validate(length(min = 1, max = 30, message = "닉네임은 1~30자로 입력하세요."))]
    nickname: String,
}

#[derive(Serialize)]
struct UserOut {
    id: u32,
    email: String,
    nickname: String,
}

#[derive(Default)]
struct Db {
    users: Vec<UserOut>,
    /// Idempotency-Key → 이미 만들어 준 응답. 같은 키로 다시 오면 **새로 만들지 않고 이걸 돌려준다.**
    replays: HashMap<String, (StatusCode, Value)>,
}

#[derive(Clone, Default)]
struct AppState {
    db: Arc<Mutex<Db>>,
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/api/signup", post(signup))
        .route("/api/signup-raw", post(signup_raw))
        .with_state(AppState::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3005").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ── 오류 타입 ────────────────────────────────────────────────────────────────

enum ApiError {
    Validation(validator::ValidationErrors),
    BadJson(String),
    EmailTaken,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message, fields) = match self {
            ApiError::Validation(errors) => {
                let fields: HashMap<String, Vec<String>> = errors
                    .field_errors()
                    .into_iter()
                    .map(|(name, errs)| {
                        let msgs = errs
                            .iter()
                            .map(|e| e.message.as_ref().map_or(e.code.to_string(), |m| m.to_string()))
                            .collect();
                        (name.to_string(), msgs)
                    })
                    .collect();
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "입력값을 확인하세요.".to_string(), Some(fields))
            }
            ApiError::BadJson(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg, None),
            ApiError::EmailTaken => (
                StatusCode::CONFLICT,
                "EMAIL_TAKEN",
                "이미 가입된 이메일입니다.".to_string(),
                Some(HashMap::from([("email".to_string(), vec!["이미 가입된 이메일입니다.".to_string()])])),
            ),
        };
        let mut body = json!({ "error": { "code": code, "message": message } });
        if let Some(fields) = fields {
            body["error"]["fields"] = json!(fields); // 없으면 키 자체를 넣지 않는다 (null이 아니라)
        }
        (status, Json(body)).into_response()
    }
}

// ── 검증까지 해 주는 extractor ──────────────────────────────────────────────────

struct ValidatedJson<T>(T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: serde::de::DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // 1단계: 파싱 (실패하면 axum은 400/415/422를 고르는데, 여기서는 전부 400으로 통일한다)
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e| ApiError::BadJson(e.body_text()))?;
        // 2단계: 규칙 검사
        value.validate().map_err(ApiError::Validation)?;
        Ok(Self(value))
    }
}

// ── 핸들러 ──────────────────────────────────────────────────────────────────

async fn signup(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(body): ValidatedJson<Signup>,
) -> Result<Response, ApiError> {
    let key = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_owned);
    let mut db = state.db.lock().unwrap();

    // 같은 키의 요청이 이미 처리됐다면 그 응답을 그대로 재생한다. (더블 클릭·재시도 대비)
    if let Some(k) = &key {
        if let Some((status, value)) = db.replays.get(k) {
            return Ok((*status, [("idempotent-replayed", "true")], Json(value.clone())).into_response());
        }
    }

    if db.users.iter().any(|u| u.email.eq_ignore_ascii_case(&body.email)) {
        return Err(ApiError::EmailTaken);
    }

    let user = UserOut { id: db.users.len() as u32 + 1, email: body.email, nickname: body.nickname };
    let value = json!({ "data": { "id": user.id, "email": user.email, "nickname": user.nickname } });
    db.users.push(user);

    if let Some(k) = key {
        db.replays.insert(k, (StatusCode::CREATED, value.clone()));
    }
    Ok((StatusCode::CREATED, Json(value)).into_response())
}

/// 비교용: 검증도, 오류 통일도 없는 "기본" Json. 타입이 틀리면 axum이 **422**를 준다.
async fn signup_raw(Json(_body): Json<Signup>) -> StatusCode {
    StatusCode::CREATED
}

const PAGE: &str = r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><title>회원가입 (study5)</title>
<style>
  body{font-family:sans-serif;max-width:360px;margin:2rem auto}
  label{display:block;margin:.8rem 0 .2rem}
  input{width:100%;padding:.4rem;box-sizing:border-box}
  .err{color:#c00;font-size:.85rem;min-height:1em}
  button:disabled{opacity:.5}
  #result{margin-top:1rem}
</style></head>
<body>
<h1>회원가입</h1>
<form id="form" novalidate>
  <label>이메일</label><input name="email" type="email"><div class="err" data-for="email"></div>
  <label>비밀번호</label><input name="password" type="password"><div class="err" data-for="password"></div>
  <label>닉네임</label><input name="nickname"><div class="err" data-for="nickname"></div>
  <button id="submit">가입</button>
</form>
<div id="result"></div>
<script>
const form = document.getElementById("form");
const button = document.getElementById("submit");
const result = document.getElementById("result");
// 폼을 연 순간 키를 하나 만든다. 같은 폼에서 보낸 요청은 몇 번을 보내도 같은 키를 쓴다.
let idemKey = crypto.randomUUID();

function showErrors(fields = {}) {
  document.querySelectorAll(".err").forEach((el) => {
    el.textContent = (fields[el.dataset.for] ?? []).join(" ");
  });
}

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (button.disabled) return;           // ① 클라이언트: 요청이 끝나기 전 재제출 차단
  button.disabled = true;
  showErrors(); result.textContent = "";

  try {
    const res = await fetch("/api/signup", {
      method: "POST",
      headers: { "Content-Type": "application/json", "Idempotency-Key": idemKey }, // ② 서버가 재생
      body: JSON.stringify(Object.fromEntries(new FormData(form))),
    });
    const body = await res.json();

    // 상태 코드에 따라 화면을 나눈다.
    if (res.status === 201) {
      result.textContent = `가입 완료: ${body.data.nickname}`;
      form.reset(); idemKey = crypto.randomUUID();      // 성공한 뒤에만 새 키
    } else if (res.status === 400 || res.status === 409) {
      showErrors(body.error.fields);                    // 입력 오류 → 필드 아래에 표시
      if (!body.error.fields) result.textContent = body.error.message;
    } else {
      result.textContent = "서버 오류입니다. 잠시 뒤 다시 시도하세요.";
    }
  } catch {
    result.textContent = "네트워크 오류입니다.";          // 서버에 닿지도 못함
  } finally {
    button.disabled = false;
  }
});
</script>
</body></html>"#;
