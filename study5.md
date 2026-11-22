# study5 — 폼 · 입력 · 검증

> 🎯 **목표**: 사용자 입력을 서버에서 받아 **검증**하고, 틀리면 **필드별 오류 메시지**를 돌려주고, 같은 폼이 두 번 제출돼도 한 번만 처리되게 만든다. 입력을 보내고 **응답 코드(201 / 400 / 409 …)에 따라 화면이 달라지는** 흐름을 완성한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s05_form.rs](lab/examples/s05_form.rs) (포트 3005), 실제 프로젝트의 같은 패턴: [demo/src/extract.rs](demo/src/extract.rs) |
| 검증 | 아래 응답은 모두 실제 실행 결과 |

---

## ① 목표

- `validator`로 규칙을 **타입 옆에** 선언하고, 검증까지 해 주는 extractor(`ValidatedJson`)를 만든다.
- 오류 응답을 `{ error: { code, message, fields } }` 한 가지 모양으로 통일한다.
- 상태 코드(201 / 400 / 409 / 5xx)에 따라 화면을 분기한다.
- 클라이언트(버튼 잠금)와 서버(`Idempotency-Key`) 양쪽에서 중복 제출을 막는다.

---

## ② 📖 이론

### 클라이언트 검증과 서버 검증

| | 클라이언트(JS, HTML 속성) | 서버(Axum) |
|---|---|---|
| 목적 | **빠른 안내** (서버까지 안 가도 알려 줌) | **진짜 방어선** |
| 신뢰 | ❌ 우회 가능 (`curl`로 바로 보내면 끝) | ✅ 모든 입력은 여기서 다시 검사 |

**서버 검증은 생략할 수 없다.** 클라이언트 검증은 UX를 위한 덤이다.

### 검증이 일어나는 두 단계

```
요청 본문(JSON 텍스트)
   │ ① 파싱: serde가 구조체로 변환     ← 문법 오류, 타입 불일치 (`"age": "abc"`), 필수 필드 누락
   ▼
Signup { email, password, nickname }
   │ ② 규칙 검사: validator             ← 형식·길이·범위 (이메일 모양, 8~72자 …)
   ▼
핸들러 실행
```

① 실패와 ② 실패는 **원인이 다르다**. 그래서 오류 코드도 다르게 준다(`BAD_REQUEST` vs `VALIDATION_ERROR`).

### 상태 코드: 400 vs 422

| 코드 | 의미 | 이 저장소의 사용 |
|---|---|---|
| **201** Created | 만들었다 (+ `Location` 헤더 권장) | 가입·글 작성 성공 |
| **400** Bad Request | 요청을 이해할 수 없거나 규칙 위반 | JSON 문법 오류, 타입 불일치, **검증 실패** |
| **409** Conflict | 현재 상태와 충돌 | 이미 가입된 이메일 |
| **422** Unprocessable Entity | "문법은 맞는데 의미가 틀림" | axum `Json<T>`가 **타입 불일치에 기본으로 주는** 코드 |

공통 API 스펙 v1은 검증 실패를 **400 `VALIDATION_ERROR`** 로 정했다. 그런데 axum의 기본 `Json<T>`는 타입 불일치에 **422**를 준다(아래 실행 결과에서 확인). 그래서 이 저장소는 `AppError`가 **422를 400으로 정규화**한다. 화면 쪽 규칙은 단순해진다: **입력 문제는 모두 400 + `error.fields`**.

### 중복 제출이 생기는 이유

- 사용자가 버튼을 **두 번 클릭**한다
- 응답이 느려서 **새로고침**하거나 다시 누른다
- 네트워크 오류로 클라이언트가 **자동 재시도**한다

글 두 개가 생기거나 결제가 두 번 되면 곤란하다. 방어는 두 겹이다.

1. **클라이언트**: 요청 중에는 버튼을 잠근다 (UX, 우회 가능)
2. **서버**: 같은 `Idempotency-Key`의 요청은 **처음 결과를 그대로 재생**한다 (진짜 방어)

> **멱등성(idempotency)**: 같은 요청을 여러 번 보내도 결과가 한 번 보낸 것과 같다는 성질. `GET`/`PUT`/`DELETE`는 원래 그렇고, `POST`는 키를 붙여 흉내 낸다.

---

## ③ 🛠️ 실기

### 3-1. 규칙 선언 — 타입 옆에

```rust
use serde::Deserialize;
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
```

| 코드 | 설명 |
|---|---|
| `#[derive(Validate)]` | `signup.validate()`를 쓸 수 있게 한다 |
| `#[validate(email(...))]` | 이메일 형식. `message`는 사용자에게 그대로 보여 줄 문구 |
| `length(min, max)` | **글자 수** 기준 (바이트가 아니다). 한글 닉네임도 1글자로 센다 |
| `#[serde(deny_unknown_fields)]` | 정의하지 않은 필드가 오면 거절. `{"role":"admin"}` 같은 **끼워 넣기 방지** |
| 비밀번호 상한 72 | Argon2 자체 제한은 아니지만, **긴 입력으로 해시 비용을 키우는 공격**을 막고 bcrypt류와도 호환되는 관례 |

### 3-2. 오류 타입과 응답 모양

```rust
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
                    .field_errors().into_iter()
                    .map(|(name, errs)| {
                        let msgs = errs.iter()
                            .map(|e| e.message.as_ref().map_or(e.code.to_string(), |m| m.to_string()))
                            .collect();
                        (name.to_string(), msgs)
                    })
                    .collect();
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "입력값을 확인하세요.".to_string(), Some(fields))
            }
            ApiError::BadJson(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg, None),
            ApiError::EmailTaken => (StatusCode::CONFLICT, "EMAIL_TAKEN", "이미 가입된 이메일입니다.".to_string(),
                Some(HashMap::from([("email".to_string(), vec!["이미 가입된 이메일입니다.".to_string()])]))),
        };
        let mut body = json!({ "error": { "code": code, "message": message } });
        if let Some(fields) = fields {
            body["error"]["fields"] = json!(fields);   // 없으면 키 자체를 넣지 않는다 (null이 아니라)
        }
        (status, Json(body)).into_response()
    }
}
```

- **오류는 값**이다. `enum`의 각 갈래가 "어떤 실패인가"를 나타내고, `IntoResponse` 한 곳에서 **전부 HTTP로 번역**한다. 새 갈래를 추가하고 `match`에 안 넣으면 컴파일 에러라서 매핑 누락이 없다.
- `fields`는 `{ "email": ["…"], "password": ["…"] }` — **필드 이름이 곧 화면의 입력칸 이름**이라 프론트가 반복문 하나로 처리한다.
- `e.message`(우리가 쓴 문구)가 없으면 `e.code`(규칙 이름, 예: `length`)로 대체한다.

### 3-3. 검증까지 해 주는 extractor

```rust
struct ValidatedJson<T>(T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where T: serde::de::DeserializeOwned + Validate, S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await
            .map_err(|e| ApiError::BadJson(e.body_text()))?;     // ① 파싱
        value.validate().map_err(ApiError::Validation)?;          // ② 규칙
        Ok(Self(value))
    }
}
```

- `FromRequest`는 **본문을 소비**하는 extractor 트레잇이다. 그래서 이 extractor는 **핸들러의 마지막 인자**여야 한다.
- 핸들러는 이제 `ValidatedJson(body): ValidatedJson<Signup>`만 적으면 된다. **검증을 잊을 수가 없다.** 인자 타입이 곧 규칙이다.
- `Json`의 거절 사유(문법 오류, 타입 불일치, `Content-Type` 누락)를 전부 `ApiError::BadJson`으로 모아 **형식을 통일**한다.

### 3-4. 핸들러 — 중복 제출 방지 포함

```rust
async fn signup(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(body): ValidatedJson<Signup>,       // 본문 extractor는 마지막
) -> Result<Response, ApiError> {
    let key = headers.get("idempotency-key").and_then(|v| v.to_str().ok()).map(str::to_owned);
    let mut db = state.db.lock().unwrap();

    // 같은 키의 요청이 이미 처리됐다면 그 응답을 그대로 재생한다.
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
```

| 코드 | 설명 |
|---|---|
| `headers.get("idempotency-key")` | 헤더 이름은 대소문자를 구분하지 않는다 (`HeaderMap`이 소문자로 다룬다) |
| `db.replays.get(k)` → 재생 | **검증·중복 검사보다 먼저** 키를 확인한다. 같은 키로 다시 와도 `EMAIL_TAKEN`이 아니라 **처음 응답(201)** 이 나가야 하기 때문 |
| `eq_ignore_ascii_case` | 이메일은 대소문자 무시 비교. (DB에서는 `UNIQUE` 제약이 최종 방어선 — study6) |
| `replays.insert(k, ...)` | **성공했을 때만** 저장. 실패(400/409)를 재생하면 사용자가 입력을 고쳐도 계속 같은 오류를 받는다 |
| `std::sync::Mutex` | 이 핸들러에는 `.await`가 락 구간에 없어서 표준 `Mutex`로 충분하다 (study3의 락 규칙) |
| `idempotent-replayed: true` | 재생된 응답임을 알리는 디버깅용 헤더 |

### 3-5. 화면 — 상태 코드별 분기

```js
let idemKey = crypto.randomUUID();      // 폼을 연 순간 키 하나

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (button.disabled) return;          // ① 요청 중 재제출 차단
  button.disabled = true;
  showErrors(); result.textContent = "";

  try {
    const res = await fetch("/api/signup", {
      method: "POST",
      headers: { "Content-Type": "application/json", "Idempotency-Key": idemKey },  // ② 서버 재생
      body: JSON.stringify(Object.fromEntries(new FormData(form))),
    });
    const body = await res.json();

    if (res.status === 201) {
      result.textContent = `가입 완료: ${body.data.nickname}`;
      form.reset(); idemKey = crypto.randomUUID();           // 성공한 뒤에만 새 키
    } else if (res.status === 400 || res.status === 409) {
      showErrors(body.error.fields);                         // 입력 오류 → 필드 아래에 표시
      if (!body.error.fields) result.textContent = body.error.message;
    } else {
      result.textContent = "서버 오류입니다. 잠시 뒤 다시 시도하세요.";
    }
  } catch {
    result.textContent = "네트워크 오류입니다.";               // 서버에 닿지도 못함
  } finally {
    button.disabled = false;                                  // 어떤 경우든 버튼 복구
  }
});
```

| 분기 | 화면 동작 |
|---|---|
| **201** | 성공 메시지, 폼 비우기, 새 키 발급 |
| **400** (+ `fields`) | 각 입력칸 아래에 메시지 표시. 키는 **유지**(같은 폼의 재시도) |
| **409** | `fields.email`에 "이미 가입된 이메일입니다." 표시 |
| 그 밖 (5xx) | "잠시 뒤 다시" 안내. 서버 사정을 자세히 보여 주지 않는다 |
| `catch` | 서버에 **닿지도 못한** 경우 (오프라인) |
| `finally` | 성공이든 실패든 버튼을 반드시 복구. 안 하면 영원히 잠긴 버튼이 된다 |

### 3-6. 실행 결과

```
# ① 규칙 위반 3개
$ curl -X POST .../api/signup -H 'content-type: application/json' \
       -d '{"email":"bad","password":"123","nickname":""}'
{"error":{"code":"VALIDATION_ERROR",
          "fields":{"email":["이메일 형식이 아닙니다."],
                    "nickname":["닉네임은 1~30자로 입력하세요."],
                    "password":["비밀번호는 8~72자로 입력하세요."]},
          "message":"입력값을 확인하세요."}}                                  → 400

# ② 성공 (키 k1)
HTTP/1.1 201 Created
{"data":{"email":"mina@example.com","id":1,"nickname":"mina"}}

# ③ 같은 키 k1로 한 번 더 — 새 사용자를 만들지 않고 처음 응답을 재생
HTTP/1.1 201 Created
idempotent-replayed: true
{"data":{"email":"mina@example.com","id":1,"nickname":"mina"}}

# ④ 키 없이 같은 이메일로 가입
{"error":{"code":"EMAIL_TAKEN","fields":{"email":["이미 가입된 이메일입니다."]},"message":"이미 가입된 이메일입니다."}}   → 409

# ⑤ JSON 문법 오류
{"error":{"code":"BAD_REQUEST","message":"Failed to parse the request body as JSON: key must be a string at line 1 column 2"}}   → 400

# ⑥ 비교: 검증·정규화 없는 기본 Json<T> (/api/signup-raw), 타입 불일치 {"email":1,...}
→ 422     ← 우리 extractor를 거치면 400, 기본 Json은 422
```

---

## ④ 🔗 연동 포인트 — 입력값을 서버 API로 보내고 응답 코드로 화면 바꾸기

```
[폼 입력]  ─ fetch POST /api/signup ─▶ [ValidatedJson] ─ 파싱 실패 ─▶ 400 BAD_REQUEST
   ▲                                          │ 검증 실패 ─────────▶ 400 VALIDATION_ERROR + fields
   │                                          │ 통과
   │                                    [핸들러] ─ 중복 ───────────▶ 409 EMAIL_TAKEN + fields.email
   │                                          │ 성공 ──────────────▶ 201 + data
   └──── 응답 코드로 분기 ◀────────────────────┘
```

**프론트와의 계약**(공통 API 스펙 v1 §2, §5):

- 요청 본문: `{ "email": "...", "password": "...", "nickname": "..." }` (필드 이름 camelCase)
- 성공: `201` + `{ "data": { ... } }`
- 실패: `{ "error": { "code", "message", "fields?", "requestId?" } }` — 프론트는 **`error.code`로 분기하고, `error.fields`를 입력칸에 꽂는다.** `message`는 보조 문구다. 오류 문구를 프론트가 파싱하지 않아도 되는 것이 통일의 목적이다.

이 단계의 `ValidatedJson`/`ApiError`는 학습용 축소판이고, 실제 프로젝트 버전은 [demo/src/extract.rs](demo/src/extract.rs)와 [demo/src/error.rs](demo/src/error.rs)다. 상세한 형태(`requestId`, 전체 코드 표)는 [study6](study6.md)과 [study14](study14.md)에서 완성한다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 컴파일 에러: `the trait bound ... Handler<_, _> is not satisfied` | `ValidatedJson`(본문 extractor)이 **마지막 인자가 아님** | 본문 extractor를 마지막으로 이동 |
| 컴파일 에러: `the trait bound CreatePost: Validate is not satisfied` | 구조체에 `#[derive(Validate)]`를 빠뜨림 | derive 추가 |
| `Content-Type`을 안 줬더니 **415**가 나옴 | `Json` extractor는 `application/json`을 요구 | 클라이언트가 헤더를 보낸다. 서버에서는 415도 `BAD_REQUEST`류로 통일해 줄지 정한다 |
| `{"role":"admin"}`을 보냈는데 통과해 버림 | `deny_unknown_fields`가 없어 **알 수 없는 필드를 무시** | `#[serde(deny_unknown_fields)]`. 그리고 **권한·작성자 같은 값은 요청 본문에서 받지 않고** 서버가 정한다 |
| 한글 이름이 `length(max=30)`에서 너무 일찍 걸림 / 안 걸림 | `length`는 글자 수 기준. 바이트 기준이라고 착각 | 이 동작이 맞다. DB 컬럼 길이(바이트 기준 DB)와는 따로 확인 |
| 같은 오류인데 응답마다 `fields`의 키 순서가 다름 | `HashMap`은 순서가 없다 | 순서가 필요하면 `BTreeMap`. (이 예제는 `serde_json::Map`으로 옮겨 정렬되어 나간다) |
| 버튼이 **영원히 비활성** | 예외가 났는데 `finally`에서 복구하지 않음 | `finally`에서 `button.disabled = false` |
| 중복 제출 방지 후 **오류를 고쳐서 다시 보냈는데도 계속 같은 오류** | 실패 응답까지 키로 재생함 | 성공(2xx)만 저장한다 |
| 같은 키를 **다른 본문**으로 재사용해도 이전 응답이 나감 | 키만 보고 본문은 비교하지 않음 | 키와 함께 **본문 해시**를 저장해 다르면 422/409로 거절 (직접 해보기 3) |
| 재생 저장소가 계속 커짐 | 키를 지우지 않음 | **TTL**(예: 24시간)을 두거나 DB 테이블 + 만료 정리 |
| curl로 한글 JSON을 보냈더니 `invalid UTF-8` 류 오류 | 터미널 코드페이지 | UTF-8 파일 + `--data-binary @file.json` |

---

## ⑥ 직접 해보기

1. **비밀번호 확인 필드**: `passwordConfirm`을 추가하고 `password`와 같은지 검사한다. `validator`의 `must_match`(또는 커스텀 검증 함수)를 쓰고, 오류가 **어느 필드 아래**에 표시되는지 확인한다.
2. **서버에서 입력 정리**: 공백만 있는 닉네임(`"   "`)이 통과하는지 확인하고, `trim` 후 검증하도록 고친다. (DTO에서 정리 → 검증 순서가 중요하다)
3. **키 + 본문 해시**: 같은 `Idempotency-Key`로 **다른 본문**이 오면 `422 IDEMPOTENCY_KEY_REUSED`로 거절한다. 본문의 해시를 `replays`에 함께 저장한다.

---

## ⑦ ☐ 체크리스트

- ☐ 클라이언트 검증과 서버 검증의 역할 차이를 설명할 수 있다
- ☐ 요청 본문 처리가 **파싱 → 규칙 검사** 두 단계라는 것을 안다
- ☐ `#[validate(...)]`로 이메일·길이 규칙을 선언했다
- ☐ `ValidatedJson`이 왜 **마지막 인자**여야 하는지 설명할 수 있다
- ☐ 오류 응답이 `{ error: { code, message, fields } }` 한 모양이다
- ☐ 400 / 409 / 201일 때 화면이 각각 어떻게 달라지는지 구현했다
- ☐ axum 기본 `Json`은 타입 불일치에 422를 주고, 이 저장소는 400으로 정규화한다는 것을 안다
- ☐ 요청 중 버튼 잠금 + `Idempotency-Key` 재생으로 중복 제출을 막았다 (`finally`에서 버튼 복구 포함)
- ☐ `deny_unknown_fields`와 "권한 값은 본문에서 받지 않는다"는 원칙을 설명할 수 있다

---

## ⑧ 다음 단계 연결

폼으로 받은 입력을 지금은 메모리에 담았다. [study6](study6.md)에서는 이것을 **DB에 저장하는 CRUD**로 확장하고, 오류 본문을 **공통 API 스펙 v1**에 맞추고(`requestId` 포함), 연결 정보를 환경변수로 분리한다.
