# study13 — 외부 서비스 연동

> 🎯 **목표**: 결제(웹훅), 메일, 파일 업로드, 소셜 로그인(OAuth2) 같은 **바깥 서비스**를 Axum에 연결하는 방법을 예제로 익힌다. 핵심은 기능이 아니라 **신뢰·실패·비밀** 세 가지다: 바깥이 보낸 것을 어떻게 믿고, 바깥이 죽었을 때 우리가 어떻게 버티고, 키를 어떻게 숨기는가.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s13_external.rs](lab/examples/s13_external.rs) (포트 3013), 업로드: [demo/src/upload.rs](demo/src/upload.rs), 대용량 스트리밍: [lab/examples/s17_realtime.rs](lab/examples/s17_realtime.rs) |
| 검증 | 단위 테스트 3개(서명 검증), `curl`로 웹훅·메일·OAuth 흐름 전체 실행. **실제 결제사·메일사·OAuth 제공자 계정은 쓰지 않았다** — 같은 프로세스에 **가짜 OAuth 제공자**(`/mock-oauth/*`)를 두고 흐름을 돌렸다 |
| 사용 크레이트 | `reqwest` (HTTP 클라이언트, `json`·`form`·`rustls`), `hmac` + `sha2` + `hex` (서명), `uuid` |

---

## ① 목표

1. 외부가 **우리를 부르는**(웹훅) 요청을 서명으로 검증하고, 중복 전달에도 한 번만 처리한다.
2. 우리가 외부를 **부르는**(메일 API) 일을 요청 스레드에서 분리하고, 실패해도 핵심 기능을 막지 않는다.
3. 파일 업로드에서 클라이언트가 준 값(이름·MIME·크기)을 믿지 않는다.
4. OAuth2 Authorization Code 흐름을 `state` 검증까지 구현한다.
5. 외부 연동의 키·URL을 환경변수로 분리하고 로그에 남기지 않는다.

---

## ② 📖 이론

### 방향이 두 가지다

```
(A) 우리 → 외부   메일 전송, 결제 생성, 소셜 로그인의 토큰 교환     우리가 클라이언트 (reqwest)
(B) 외부 → 우리   결제 완료 웹훅, 소셜 로그인 콜백                  우리가 서버 (axum 핸들러)
```

| | (A) 호출 | (B) 수신 |
|---|---|---|
| 주된 위험 | **느림·장애·타임아웃** — 외부가 죽으면 우리 요청이 같이 멈춘다 | **위조·중복·순서 뒤바뀜** — 누구나 URL만 알면 흉내 낼 수 있다 |
| 방어 | 타임아웃, 백그라운드 분리, 재시도, 실패해도 핵심 기능 유지 | **서명 검증**, 멱등 처리, 빠른 2xx 응답 |
| 비밀 | API 키 → 환경변수, 로그에 남기지 않음 | 서명 키 → 환경변수 |

### 서명(HMAC)으로 "진짜 보낸 것"을 증명

결제사는 **우리와 공유한 비밀 키**로 요청 본문의 HMAC-SHA256을 계산해 헤더로 보낸다(`X-Signature: sha256=<hex>`). 우리가 같은 계산을 해서 같으면 ① 키를 아는 쪽이 보냈고 ② 본문이 중간에 바뀌지 않았다는 뜻이다.

> ⚠️ **서명은 받은 그대로의 바이트로 계산**한다. `Json<T>`로 먼저 파싱한 뒤 다시 직렬화하면 공백·키 순서가 달라져 서명이 안 맞는다. 그래서 웹훅 핸들러는 `Bytes`로 원문을 받고, **검증이 끝난 뒤에** JSON으로 읽는다.

> ⚠️ 비교는 `==`가 아니라 **상수 시간 비교**(`Mac::verify_slice`). 문자열 `==`는 앞에서부터 달라지는 지점까지의 시간 차이가 새어 나가 서명을 한 글자씩 추측하는 **타이밍 공격**을 허용한다.

### 멱등성 — 같은 이벤트가 두 번 와도

웹훅은 **최소 한 번(at-least-once)** 전달이 기본이다. 우리가 늦게 답하거나 5xx를 주면 결제사는 **같은 이벤트를 다시 보낸다.** 이벤트 `id`를 기억해 두고 **이미 처리한 것은 건너뛰어야** 재고가 두 번 깎이거나 메일이 두 번 가지 않는다.

### OAuth2 Authorization Code — 비밀번호를 우리가 받지 않는 로그인

```
 ① 브라우저 ──▶ 우리 /auth/login
 ② 우리 ──▶ 302 제공자 로그인 화면  (client_id, redirect_uri, scope, state=랜덤)
 ③ 사용자가 제공자에서 로그인·동의
 ④ 제공자 ──▶ 302 우리 /auth/callback?code=...&state=...
 ⑤ 우리 서버 ──▶ 제공자 /token  (code + client_secret)  ← 서버끼리. secret이 브라우저에 안 나온다
 ⑥ 제공자 ──▶ access_token
 ⑦ 우리 서버 ──▶ 제공자 /user  (Bearer access_token) → 사용자 정보 → 우리 쪽 로그인(세션/토큰 발급)
```

- **`state`**: 로그인을 시작한 브라우저와 콜백을 받는 브라우저가 **같음**을 확인하는 일회용 값 (CSRF 방지). 없거나 틀리면 거절한다.
- **`code`**는 일회용이고 수명이 짧다. 그 자체로는 쓸모가 없고 **`client_secret`을 가진 서버만** 토큰으로 바꿀 수 있다.

---

## ③ 🛠️ 실기

### 3-1. 결제 웹훅 — 서명 검증 + 멱등 처리

```rust
type HmacSha256 = Hmac<Sha256>;

fn verify_signature(secret: &str, body: &[u8], header_value: &str) -> bool {
    let Some(hex_sig) = header_value.strip_prefix("sha256=") else { return false; };
    let Ok(sig) = hex::decode(hex_sig) else { return false; };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else { return false; };
    mac.update(body);
    mac.verify_slice(&sig).is_ok()          // 상수 시간 비교
}

async fn payment_webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let signature = headers.get("x-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !verify_signature(&state.webhook_secret, &body, signature) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": { "code": "UNAUTHORIZED", "message": "서명이 올바르지 않습니다." } }))).into_response();
    }

    let Ok(event) = serde_json::from_slice::<Value>(&body) else { return StatusCode::BAD_REQUEST.into_response(); };
    let event_id = event["id"].as_str().unwrap_or_default().to_string();

    // 이미 처리한 이벤트면 건너뛴다. insert가 false를 돌려주면 "이미 있었다"는 뜻
    if !state.seen_events.lock().unwrap().insert(event_id.clone()) {
        return (StatusCode::OK, Json(json!({ "status": "duplicate-ignored" }))).into_response();
    }

    println!("[webhook] {} amount={}", event["type"], event["amount"]);
    (StatusCode::OK, Json(json!({ "status": "processed" }))).into_response()
}
```

| 코드 | 설명 |
|---|---|
| `body: Bytes` | **원문 바이트**를 그대로 받는다. `Json` extractor를 쓰지 않는 이유 |
| `strip_prefix("sha256=")` → `hex::decode` | 헤더 형식 검사. 형식이 틀리면 `false`(패닉하지 않는다) |
| `mac.verify_slice(&sig)` | **상수 시간** 비교. 직접 `==`로 비교하지 않는다 |
| 서명 검증 → **그다음에** JSON 파싱 | 검증 전의 입력은 아무것도 믿지 않는다 |
| `HashSet::insert`의 반환값 | 새로 들어갔으면 `true`, 이미 있으면 `false`. "확인 + 기록"이 **한 번의 락** 안에서 원자적으로 일어난다 |
| 중복이어도 **200** | 4xx를 주면 결제사가 "실패"로 알고 계속 재시도한다. 이미 처리했으니 성공으로 답한다 |
| 빨리 2xx | 웹훅 응답은 보통 몇 초 안에 와야 한다. 무거운 처리는 큐/백그라운드로 (study15) |

시험 (서명 검증 단위 테스트 3개):

```
test tests::올바른_서명은_통과한다 ... ok
test tests::본문이_바뀌면_서명이_깨진다 ... ok
test tests::키가_다르거나_형식이_틀리면_거부한다 ... ok
```

실행 결과 — 결제사 흉내는 `openssl`로 서명을 만들어 `curl`로 보냈다:

```
$ BODY='{"id":"evt_1","type":"payment.succeeded","amount":1000}'
$ SIG="sha256=$(printf '%s' "$BODY" | openssl dgst -sha256 -hmac dev-webhook-secret | sed 's/^.* //')"

$ curl -X POST .../webhooks/payment -H "x-signature: $SIG" -d "$BODY"
{"status":"processed"}
$ curl -X POST .../webhooks/payment -H "x-signature: $SIG" -d "$BODY"         # 같은 이벤트 재전송
{"status":"duplicate-ignored"}
$ curl ... -H "x-signature: sha256=00" -d "$BODY"                             → 401
$ curl ... -H "x-signature: $SIG" -d '{"id":"evt_2",...,"amount":999999}'     → 401   (금액을 바꾸면 서명이 깨진다)
```

> 결제 생성(체크아웃 세션 만들기)은 (A) 방향의 일반적인 HTTP 호출이다: `reqwest`로 결제사 API에 금액·주문 id를 보내 결제 페이지 URL을 받아 사용자를 보낸다. **결제 완료 여부는 사용자가 돌아온 화면(리다이렉트)이 아니라 웹훅으로 확정**한다 — 리다이렉트는 사용자가 조작할 수 있다.

### 3-2. 메일 — 요청에서 분리해 백그라운드로

```rust
async fn send_welcome(State(state): State<AppState>, Json(body): Json<WelcomeMail>) -> Response {
    if !body.to.contains('@') { return StatusCode::BAD_REQUEST.into_response(); }

    tokio::spawn(async move {
        let payload = json!({ "to": body.to, "subject": "가입을 환영합니다", "text": "환영합니다!" });
        match &state.mail_api_url {
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

    StatusCode::ACCEPTED.into_response()          // 202: 접수했다. 처리는 아직
}
```

| 코드 | 설명 |
|---|---|
| `tokio::spawn` | 응답을 먼저 보내고 전송은 **다른 태스크**에서. 메일 서버가 느려도 가입 요청이 안 막힌다 |
| `202 Accepted` | "받았고 처리 중"이라는 정직한 상태 코드. `200`이나 `201`로 답하면 이미 보낸 것처럼 읽힌다 |
| `MAIL_API_URL` 없으면 로그만 | **개발·테스트에서 진짜 메일이 나가지 않게.** 설정이 없으면 안전한 쪽(보내지 않음)으로 |
| `bearer_auth(key)` | 메일 API는 보통 `Authorization: Bearer <키>` + JSON POST. 키는 환경변수 |
| 실패 처리가 로그뿐 | ⚠️ **한계**: 프로세스가 죽으면 이 메일은 사라진다. 중요한 메일은 DB에 "보낼 일"을 먼저 저장(outbox)하고 워커가 처리·재시도한다 (study15의 작업 큐) |

`reqwest::Client`는 **한 번 만들어 공유**한다(`AppState.http`). 요청마다 만들면 연결 풀을 못 쓴다. **타임아웃은 반드시** 건다(`Client::builder().timeout(...)`, s18 예제 참고) — 기본값은 무한이다.

### 3-3. 파일 업로드

**① 이미지 썸네일 — [demo/src/upload.rs](demo/src/upload.rs)** (5MB 이하, 메모리에 올림)

| 방어 | 코드 |
|---|---|
| 형식 허용 목록 | `ALLOWED = [("image/jpeg","jpg"), ("image/png","png"), ("image/gif","gif"), ("image/webp","webp")]` — 목록에 없으면 400 |
| 확장자는 **검증된 MIME에서** | 클라이언트가 보낸 파일명의 확장자를 쓰지 않는다 |
| 파일명은 **서버가 생성** | `format!("{}.{extension}", Uuid::new_v4())` — `../../etc/passwd` 같은 입력이 경로에 닿지 않는다 |
| 크기 제한 | 전역 본문 2MB, 업로드 라우트만 `DefaultBodyLimit::max(5MB)` (안쪽 값이 이긴다) |
| 삭제 시 방어 | `remove_if_exists`가 파일명에 `/`, `\`, `..`가 있으면 무시 |
| 새 파일 저장 후 기존 파일 삭제 | 순서가 반대면 실패 시 둘 다 잃는다 |

> ⚠️ 한계: `Content-Type`은 **클라이언트가 주장하는 값**이다. 실제 파일 내용(매직 바이트)을 확인하는 것이 더 안전하다. 그리고 이 방식은 파일 전체를 메모리에 올리므로 **작은 파일**용이다.

**② 대용량 — 디스크로 스트리밍** ([lab/examples/s17_realtime.rs](lab/examples/s17_realtime.rs), study17): `field.chunk()`로 도착하는 만큼만 읽어 파일에 쓰고 해시를 계산한다. 실측: 3MB 파일의 sha256이 서버 응답과 일치했고, 22MB를 보내면 20MB를 넘는 순간 **413으로 끊고 반쯤 받은 파일을 지웠다.** 클라이언트가 `../../evil.txt`라고 보낸 파일명은 무시되고 서버가 만든 UUID 이름으로 저장됐다.

**③ 외부 저장소(S3 등)**: 파일을 우리 서버가 중계하지 않고, 서버가 **짧은 수명의 업로드 URL(presigned URL)** 만 발급해 브라우저가 저장소에 직접 올리게 한다. 서버 대역폭·메모리를 쓰지 않고, 서버는 "업로드가 끝났다"는 알림(웹훅)으로 DB에 기록한다. (이 저장소에서는 구현하지 않았다.)

### 3-4. 소셜 로그인 (OAuth2) — 흐름 전체 실행

```rust
async fn oauth_login(State(state): State<AppState>) -> Redirect {
    let st = uuid::Uuid::new_v4().to_string();                    // state: 일회용 랜덤
    state.pending_states.lock().unwrap().insert(st.clone());
    let c = &state.oauth;
    Redirect::to(&format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=read:user&state={}",
        c.authorize_url, c.client_id, urlencode(&c.redirect_uri), st))
}

async fn oauth_callback(State(state): State<AppState>, Query(q): Query<CallbackQuery>) -> Response {
    // 1) state 검증: 우리가 발급한 값이어야 하고, 한 번만 쓸 수 있다 (remove가 true일 때만 통과)
    if !state.pending_states.lock().unwrap().remove(&q.state) {
        return (StatusCode::BAD_REQUEST, "state가 올바르지 않습니다 (CSRF 의심)").into_response();
    }

    // 2) code → access_token. 서버끼리 호출하므로 client_secret이 브라우저에 노출되지 않는다
    let token: Value = match state.http.post(&c.token_url)
        .form(&[("grant_type","authorization_code"), ("code", q.code.as_str()),
                ("client_id", c.client_id.as_str()), ("client_secret", c.client_secret.as_str()),
                ("redirect_uri", c.redirect_uri.as_str())])
        .send().await.and_then(|r| r.error_for_status())
    { Ok(res) => res.json().await.unwrap_or_default(),
      Err(e) => return (StatusCode::BAD_GATEWAY, format!("토큰 교환 실패: {e}")).into_response() };

    // 3) 사용자 정보 조회 → 4) 우리 쪽 로그인 처리(세션 쿠키 발급)
    let user: Value = state.http.get(&c.user_url).bearer_auth(access_token).send().await ...;
    ([(header::SET_COOKIE, "sid=oauth-session; HttpOnly; SameSite=Lax; Path=/")],
     Json(json!({ "data": { "provider": "mock", "user": user } }))).into_response()
}
```

| 코드 | 설명 |
|---|---|
| `pending_states.remove(&q.state)` | 있으면 지우고 `true`. **한 번 쓰면 재사용 불가** (리플레이 방지) |
| `.form(&[...])` | 토큰 엔드포인트는 보통 `application/x-www-form-urlencoded`. (reqwest 0.13은 `form` feature를 켜야 한다) |
| `.error_for_status()` | 4xx/5xx를 `Err`로 바꿔 `?`/`match`로 다루기 쉽게 |
| `BAD_GATEWAY(502)` | **우리 잘못이 아니라 뒤쪽(제공자) 문제**임을 알리는 코드 |
| 환경변수 | `OAUTH_CLIENT_ID`, `OAUTH_CLIENT_SECRET`, `OAUTH_AUTHORIZE_URL`, `OAUTH_TOKEN_URL`, `OAUTH_USER_URL`. 실제 제공자(GitHub·Google 등)로 바꿀 때는 이 값들만 바꾼다 |

실행 결과 (`curl -L`로 리다이렉트를 따라감):

```
$ curl -s -L -c jar http://127.0.0.1:3013/auth/login
{"data":{"provider":"mock","user":{"email":"mina@example.com","id":12345,"login":"mina"}}}

$ curl -s "http://127.0.0.1:3013/auth/callback?code=mock-code-123&state=forged"      # 위조된 state
state가 올바르지 않습니다 (CSRF 의심)
```

---

## ④ 🔗 연동 포인트

| 외부 서비스 | 이 단계 결과를 잇는 곳 |
|---|---|
| **결제 웹훅** | 검증된 이벤트 → 주문 상태 갱신(DB, study11) → 후속 메일 큐(study15). 멱등 키 저장소는 메모리가 아니라 **DB의 UNIQUE 컬럼**(이벤트 id)으로 옮겨야 서버가 여러 대여도 안전하다 |
| **메일** | 가입 핸들러(study7)가 `202`로 끝내고 메일은 백그라운드로 → 실패 재시도는 작업 큐(study15) → 테스트에서는 `Notifier` 같은 trait로 가짜(study9) |
| **파일 업로드** | 저장 파일명 → DB(`posts.thumbnail`, study6) → `ServeDir`로 서빙(`/uploads`) → 프론트 `<img>` |
| **소셜 로그인** | 콜백이 사용자 정보를 받으면 우리 `users`에서 찾거나 만들고 → study7의 **JWT/쿠키 발급**으로 이어진다. 외부 제공자는 "누구인가"만 증명하고, 이후 인증은 우리 방식 그대로다 |
| **환경변수** | 모든 키·URL은 `Config`(study8)에 모아 **시작 시 검증**. 운영에서는 시크릿 저장소에서 주입(study10, 16). 로그에 키를 남기지 않는다(DB URL처럼 마스킹) |
| **오류 계약** | 외부 장애는 우리 규칙으로 번역한다: 외부 4xx 일부는 그대로, 연결 실패·5xx·타임아웃은 **502 `BAD_GATEWAY`** (study18에서 같은 규칙을 AI 서버에 적용) |

### 외부 연동의 공통 체크

```
□ 타임아웃을 걸었다 (reqwest Client::builder().timeout)
□ 재시도는 멱등한 요청에만, 간격을 늘려 가며(백오프), 횟수 제한
□ 외부 장애가 핵심 기능을 막지 않는다 (메일 실패해도 가입 성공)
□ 수신(웹훅)은 서명 검증 → 멱등 처리 → 빠른 2xx
□ 키는 환경변수, 코드/이미지/로그에 없다
□ 테스트에서는 가짜로 (실제 결제·메일이 나가지 않는다)
```

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 컴파일 에러: `no method named form found for struct RequestBuilder` | **reqwest 0.13**은 `form`이 선택 feature | `cargo add reqwest --features json,form,stream,rustls` |
| 웹훅이 **항상 401** (서명은 맞게 만들었는데) | 핸들러가 `Json<T>`로 파싱한 뒤 다시 직렬화해서 검증 → 바이트가 달라짐 | `Bytes`로 원문을 받아 **그 바이트로** 검증 |
| 웹훅 서명을 만들 때 줄바꿈 때문에 불일치 | `echo "$BODY"`는 끝에 `\n`을 붙인다 | `printf '%s' "$BODY"` |
| 같은 이벤트가 두 번 처리됨 | 멱등 키 저장 없음 / 서버가 여러 대인데 메모리에 저장 | 이벤트 id를 **DB UNIQUE**로 |
| 결제사가 같은 웹훅을 계속 재전송 | 우리가 5xx/타임아웃 또는 중복에 4xx를 줌 | 처리했으면(중복 포함) 2xx. 오래 걸리면 큐에 넣고 먼저 2xx |
| 메일이 요청을 오래 붙잡음 | 요청 안에서 `.await`로 메일 API를 기다림 | `tokio::spawn` + `202` |
| `tokio::spawn` 안에서 `State`를 쓰려는데 `'static` 에러 | 태스크는 요청보다 오래 살 수 있어 **빌린 값을 못 쓴다** | `AppState`는 `Clone`이므로 `move`로 소유권을 가져간다 (핸들러가 `State`를 값으로 받음) |
| OAuth 콜백에서 `state가 올바르지 않습니다` | 서버 재시작으로 메모리의 `pending_states`가 사라짐 / 다른 서버 인스턴스로 콜백이 감 | `state`를 **서명된 쿠키**나 서버 세션/DB에 저장 |
| OAuth `redirect_uri_mismatch` | 제공자에 등록한 주소와 요청의 `redirect_uri`가 글자 단위로 다름 (끝 슬래시 포함) | 제공자 설정과 환경변수를 똑같이 |
| 외부 호출이 **영원히** 멈춤 | `reqwest` 기본 타임아웃 없음 | `Client::builder().timeout(Duration::from_secs(10))` |
| 업로드가 `413 Payload Too Large` | 전역 본문 한도(2MB) | 업로드 라우트에만 `DefaultBodyLimit::max(...)` |
| 파일명에 `../`가 들어 있는 업로드로 **다른 폴더에 저장** | 클라이언트 파일명을 경로에 사용 | 서버가 만든 이름(UUID)만 사용 |
| 키가 로그·에러 응답에 노출 | 요청/설정 객체를 `Debug`로 출력 | 비밀 필드는 출력하지 않는다. `redacted_*` 같은 헬퍼 (study10) |

---

## ⑥ 직접 해보기

1. **타임스탬프로 재전송 공격 막기**: 서명 대상을 `"{timestamp}.{body}"`로 바꾸고(헤더 `X-Timestamp`), 5분보다 오래된 요청은 거절한다. 같은 서명을 5분 뒤에 다시 보내면 401이 나는지 확인한다.
2. **메일 전송을 trait로**: study9의 `Notifier` 패턴을 따라 `MailSender` trait + `LogMailer`/`HttpMailer`를 만들고, "메일 실패해도 가입은 201"인 시험을 쓴다.
3. **OAuth `state`를 쿠키로**: `pending_states`(메모리) 대신 `HttpOnly` 쿠키에 `state`를 담아 보내고 콜백에서 비교하게 바꾼다. 서버를 재시작해도 로그인이 끊기지 않는지 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ 웹훅이 **원문 바이트**로 서명 검증되어야 하는 이유와, 상수 시간 비교가 필요한 이유를 설명할 수 있다
- ☐ 같은 웹훅이 두 번 와도 한 번만 처리되고, 중복에도 2xx를 주는 이유를 안다
- ☐ 메일 같은 느린 외부 호출을 `spawn` + `202`로 분리했고, 그 방식의 한계(프로세스가 죽으면 유실)를 설명할 수 있다
- ☐ 파일 업로드에서 **파일명·MIME·크기를 믿지 않는** 이유 세 가지를 말할 수 있다
- ☐ OAuth2 Authorization Code의 7단계와 `state`/`code`/`client_secret`의 역할을 그릴 수 있다
- ☐ 위조된 `state`로 콜백했을 때 거절되는 것을 `curl`로 확인했다
- ☐ 외부 연동 키·URL이 환경변수에 있고 로그에 남지 않는다
- ☐ 외부 장애를 우리 오류 계약(502 등)으로 번역하는 규칙을 안다
- ☐ 테스트에서는 실제 결제·메일이 나가지 않도록 가짜로 바꾸는 방법을 안다

---

## ⑧ 다음 단계 연결

외부 서비스와 대화하게 되었으니 [study14](study14.md)에서는 우리 API 자체의 **설계 규칙**을 정리한다: REST 리소스 설계, OpenAPI 문서화, 버전 관리, 오프셋/커서 페이지네이션, 오류 응답 규칙, 캐시 헤더(ETag).
