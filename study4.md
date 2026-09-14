# 오류 타입으로 응답 통일하기 (Study 4)

[study3.md](study3.md)에서 본문 관련 오류는 JSON으로 통일했지만, 아직 이런 상태입니다.

```
GET /posts/999  → 404 | {"error":"not_found","message":"post 999 not found"}   ← 내가 만든 것
GET /posts/abc  → 400 | Invalid URL: Cannot parse `abc` to a `i64`             ← 평문
GET /nope       → 404 | (본문이 아예 없음)
DELETE /health  → 405 | (본문이 아예 없음)
```

클라이언트 입장에서 **"오류가 나면 `error`와 `message` 필드가 있는 JSON이 온다"** 고 믿을 수 없습니다. 이 문서에서 그걸 보장합니다.

**이 문서에서 만드는 것**: `AppError` 단일 오류 타입 + 모든 extractor 실패·404·405·500까지 하나의 형식으로 통일

---

## 1. 목표 — 오류 응답 계약 정하기

먼저 "모든 오류는 이 모양이다"를 정합니다.

```json
{
  "error": "not_found",                       // 기계가 읽는 코드 (분기용)
  "message": "post 999 not found",            // 사람이 읽는 설명
  "details": { "title": ["제목은 1~100자..."] }  // 선택 — 검증 실패일 때만
}
```

| 필드 | 역할 | 왜 필요한가 |
|---|---|---|
| `error` | 안정적인 에러 코드 | 메시지 문구가 바뀌어도 클라이언트 분기 코드가 안 깨짐 |
| `message` | 사람이 읽는 설명 | 디버깅·토스트 메시지 |
| `details` | 필드별 상세 | 폼 검증 UI에서 필드 아래에 표시 |

---

## 2. `thiserror`로 에러 타입 정의하기

```powershell
cargo add thiserror anyhow
```

두 크레이트의 역할이 다릅니다. 이건 Rust 에러 처리에서 가장 자주 헷갈리는 부분입니다.

| 크레이트 | 용도 | 성격 |
|---|---|---|
| **`thiserror`** | **구체적인 에러 타입을 정의**할 때. `enum`에 `Display`/`Error` 구현을 자동 생성 | 호출자가 **경우를 구분해서 처리**해야 하는 에러 |
| **`anyhow`** | **에러를 뭉뚱그려 전파**할 때. "뭐가 됐든 실패했다" | 호출자가 **구분할 필요 없는** 에러 (보통 500) |

우리 `AppError`는 `thiserror`로 만들고, "예상 못 한 내부 오류" 변형 하나만 `anyhow::Error`를 담습니다.

### `src/error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    /// 리소스를 찾지 못함 → 404
    #[error("{resource} {id} not found")]
    NotFound { resource: &'static str, id: String },

    /// 요청 자체가 잘못됨(경로/쿼리 파싱 실패 등) → 400
    #[error("{0}")]
    BadRequest(String),

    /// 본문 파싱 실패 → axum이 판단한 상태 코드를 그대로 사용 (400 / 415 / 422)
    #[error("{message}")]
    InvalidBody { status: StatusCode, message: String },

    /// 값 검증 실패 → 422
    #[error("validation failed")]
    Validation(validator::ValidationErrors),

    /// 인증 실패 → 401 (study6)
    #[error("{0}")]
    Unauthorized(String),

    /// 권한 없음 → 403 (study7)
    #[error("{0}")]
    Forbidden(String),

    /// 중복 등 상태 충돌 → 409 (study6)
    #[error("{0}")]
    Conflict(String),

    /// 예상하지 못한 내부 오류 → 500
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
```

### `thiserror`의 어트리뷰트 읽는 법

| 어트리뷰트 | 의미 |
|---|---|
| `#[error("...")]` | 이 변형의 `Display` 출력 형식. `{0}`은 튜플 필드, `{name}`은 이름 있는 필드 |
| `#[error(transparent)]` | 감싼 에러의 메시지를 **그대로** 쓴다 (껍데기를 씌우지 않음) |
| `#[from]` | **`From` 구현을 자동 생성** → `?`가 이 타입을 자동으로 변환해 준다 |
| `#[source]` | 원인 에러 체인 연결 (`{:?}`에 함께 출력됨) |

`#[from] anyhow::Error` 한 줄이 하는 일이 큽니다. 덕분에 이런 코드가 그냥 됩니다.

```rust
async fn boom() -> AppResult<&'static str> {
    let _n: i32 = "not-a-number".parse()
        .map_err(|e: std::num::ParseIntError| anyhow::anyhow!("설정 값 파싱 실패: {e}"))?;
    //                                                                              ↑
    //                              anyhow::Error → AppError::Internal 자동 변환
    Ok("unreachable")
}
```

### 왜 `enum`인가

`match`로 분기할 때 **컴파일러가 빠뜨린 경우를 잡아줍니다.**

```rust
fn status(&self) -> StatusCode {
    match self {
        Self::NotFound { .. } => StatusCode::NOT_FOUND,
        Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        // ... 하나라도 빠지면 컴파일 에러: "non-exhaustive patterns"
    }
}
```

나중에 `AppError::RateLimited`를 추가하면, **상태 코드 매핑을 빠뜨린 모든 자리가 컴파일 에러로 표시**됩니다. 런타임에 "어? 이 에러는 500으로 나가네?" 하고 발견하는 일이 없습니다. — Nest/Spring의 전역 예외 필터가 절대 줄 수 없는 안전망입니다.

---

## 3. `IntoResponse` 구현 — 하나의 출구

```rust
#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]   // None이면 키 자체를 뺀다
    details: Option<HashMap<String, Vec<String>>>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();

        let details = match &self {
            Self::Validation(errors) => Some(flatten_validation_errors(errors)),
            _ => None,
        };

        let message = match &self {
            // 500은 내부 사정을 밖으로 흘리지 않는다. 원인은 서버 로그에만.
            Self::Internal(err) => {
                eprintln!("[internal error] {err:?}");
                "서버 내부 오류가 발생했습니다.".to_string()
            }
            Self::Validation(_) => "요청 값이 규칙에 맞지 않습니다.".to_string(),
            other => other.to_string(),     // thiserror가 만들어준 Display
        };

        (status, Json(ErrorBody { error: code, message, details })).into_response()
    }
}
```

### ⭐ 500 오류의 메시지를 숨기는 이유 — 보안

```rust
Self::Internal(err) => {
    eprintln!("[internal error] {err:?}");            // 서버 로그에는 전부
    "서버 내부 오류가 발생했습니다.".to_string()        // 클라이언트에는 일반적인 문구만
}
```

DB 에러 메시지를 그대로 내보내면 **테이블명, 컬럼명, 쿼리 구조, 때로는 파일 경로까지** 노출됩니다. 공격자에게 스키마를 알려주는 셈입니다.

- 클라이언트에게는 "내부 오류"만
- 서버 로그에는 `{err:?}`로 **전체 체인**을 (`{err}`는 최상위 메시지만, `{err:?}`는 원인까지)
- 실무에서는 여기에 **요청 ID**를 붙여서, 사용자가 신고한 ID로 로그를 찾을 수 있게 합니다.

이건 **Nest/Spring에서도 똑같이 해야 하는 일**인데, 거기서는 기본 동작이 스택트레이스 노출이라 끄는 걸 잊기 쉽습니다. 여기서는 애초에 내가 매핑을 쓰기 때문에 의식하게 됩니다.

### 타입 별칭으로 시그니처 줄이기

```rust
pub type AppResult<T> = Result<T, AppError>;
```

핸들러가 이렇게 짧아집니다.

```rust
// Before (study3)
) -> Result<Json<Post>, (StatusCode, Json<serde_json::Value>)> {

// After
) -> AppResult<Json<Post>> {
```

---

## 4. extractor 실패를 `AppError`로 흡수하기

여기가 study4의 실질적인 작업량입니다. axum 기본 extractor는 각자의 rejection 타입을 갖고, 그 응답은 평문입니다. **한 겹 감싸서 `Rejection = AppError`로 바꿉니다.**

### `src/extract.rs`

```rust
/// axum::extract::Path와 같지만 실패가 AppError::BadRequest가 된다.
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Path(value))
            .map_err(|rejection| AppError::BadRequest(rejection.body_text()))
    }
}
```

`Query<T>`도 똑같은 패턴입니다. 그리고 `ValidatedJson<T>`의 `Rejection`도 `AppError`로 바꿉니다.

```rust
impl<T, S> FromRequest<S> for ValidatedJson<T> {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await?;   // JsonRejection → AppError
        value.validate()?;                                             // ValidationErrors → AppError
        Ok(ValidatedJson(value))
    }
}
```

두 `?`가 동작하려면 `From` 두 개가 필요합니다.

```rust
impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        AppError::InvalidBody {
            status: rejection.status(),      // axum의 판단(400/415/422)을 살린다
            message: rejection.body_text(),
        }
    }
}

impl From<validator::ValidationErrors> for AppError {
    fn from(errors: validator::ValidationErrors) -> Self {
        AppError::Validation(errors)
    }
}
```

### ⚠️ 이름이 같은 타입을 쓰는 트릭

```rust
use crate::extract::{Path, Query};        // 내 래퍼
// axum::extract::Path 는 import하지 않는다
```

내 `Path`가 `axum::extract::Path`를 **가립니다(shadowing).** 덕분에 핸들러 코드는 **한 글자도 안 바뀝니다.**

```rust
pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Post>> {
```

`extract.rs` 안에서만 `axum::extract::Path::<T>::from_request_parts(...)`처럼 **완전 경로**로 원본을 부릅니다. — 실무에서 자주 쓰는 패턴입니다. 다만 **"이 `Path`가 어느 쪽이지?"** 하고 헷갈릴 수 있으니, 파일 상단 주석으로 의도를 남겨두는 게 좋습니다.

---

## 5. 404와 405 — 라우터 차원의 fallback

extractor를 다 감싸도 **라우터에 도달조차 못 한 요청**은 여전히 빈 응답입니다.

```rust
fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .nest("/posts", posts::router())
        .fallback(handle_404)                       // 매칭되는 경로 없음
        .method_not_allowed_fallback(handle_405)    // 경로는 있는데 메서드가 없음
        .with_state(state)
}
```

```rust
/// 404 — 핸들러가 AppError를 그냥 반환해도 된다. AppError가 IntoResponse니까.
pub async fn handle_404() -> AppError {
    AppError::NotFound { resource: "route", id: "(unknown)".to_string() }
}

/// 405
pub async fn handle_405() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "error": "method_not_allowed",
            "message": "이 경로에서 지원하지 않는 HTTP 메서드입니다.",
        })),
    ).into_response()
}
```

- **`handle_404`의 반환 타입이 `AppError`** 인 것에 주목하세요. `AppError`가 `IntoResponse`이므로 **핸들러 반환 타입으로 직접 쓸 수 있습니다.** "항상 실패하는 핸들러"라면 이게 가장 짧습니다.
- `.method_not_allowed_fallback()`을 안 붙이면 405는 **본문 없이** 나갑니다. 별개 메서드라는 걸 몰라서 빠뜨리기 쉽습니다.
- `.fallback()`을 `.nest()` **뒤에** 붙였는데도 하위 라우터까지 적용됩니다. (하위 라우터가 자체 fallback을 갖고 있으면 그쪽이 우선)

---

## 6. 실행 및 검증

| # | 요청 | 상태 | 응답 본문 |
|---|---|---|---|
| 1 | `GET /nope` | `404` | `{"error":"not_found","message":"route (unknown) not found"}` |
| 2 | `DELETE /health` | `405` | `{"error":"method_not_allowed","message":"이 경로에서 지원하지 않는 HTTP 메서드입니다."}` |
| 3 | `GET /posts/999` | `404` | `{"error":"not_found","message":"post 999 not found"}` |
| 4 | `GET /posts/abc` | `400` | `{"error":"bad_request","message":"Invalid URL: Cannot parse 'abc' to a 'i64'"}` |
| 5 | `GET /echo` | `400` | `{"error":"bad_request","message":"Failed to deserialize query string: missing field 'message'"}` |
| 6 | `POST /posts` `{"title":"",...}` | `422` | `{"error":"validation_failed","message":"요청 값이 규칙에 맞지 않습니다.","details":{"title":["제목은 1~100자여야 합니다"]}}` |
| 7 | `POST /posts` (`text/plain`) | `415` | `{"error":"invalid_body","message":"Expected request with 'Content-Type: application/json'"}` |
| 8 | `GET /boom` | `500` | `{"error":"internal_error","message":"서버 내부 오류가 발생했습니다."}` |
| 9 | `GET /posts/1` | `200` | `{"id":1,"title":"Seed",...}` (정상 동작 확인) |

**8번의 서버 콘솔에는** 클라이언트에게 안 나간 원인이 찍힙니다.

```
[internal error] 설정 값 파싱 실패: invalid digit found in string
```

### 목표 달성 확인

이제 **모든 오류 응답이 `{"error": ..., "message": ...}` 형태**입니다. 클라이언트는 이렇게 쓸 수 있습니다.

```typescript
if (!res.ok) {
  const { error, message, details } = await res.json();
  switch (error) {
    case "validation_failed": showFieldErrors(details); break;
    case "not_found":         show404(); break;
    case "unauthorized":      redirectToLogin(); break;
    default:                  toast(message);
  }
}
```

---

## 7. 다른 프레임워크와 비교

| | Spring Boot | NestJS | FastAPI | Axum |
|---|---|---|---|---|
| 방식 | `@RestControllerAdvice` + `@ExceptionHandler` | 전역 `ExceptionFilter` | `@app.exception_handler` | `IntoResponse for AppError` |
| 적용 지점 | 예외가 **던져지면** 프레임워크가 가로챔 | 동일 | 동일 | **반환 타입이 `Result`라서 자연스럽게 통과** |
| 누락 감지 | 런타임 (안 잡히면 500) | 런타임 | 런타임 | **컴파일 타임** (`match` 완전성) |
| 예상 못 한 예외 | 자동으로 500 (스택트레이스 노출 주의) | 동일 | 동일 | **`?`로 전파하지 않으면 애초에 컴파일 안 됨** |

**Axum 쪽의 장단:**

- 👍 "이 함수가 실패할 수 있다"가 **시그니처에 드러납니다.** 호출자가 모를 수 없습니다.
- 👍 에러 변형을 추가하면 **매핑을 빠뜨린 곳이 전부 컴파일 에러**로 보입니다.
- 👎 **`?`를 일일이 붙여야 합니다.** 예외처럼 "알아서 위로 올라가는" 편의는 없습니다.
- 👎 extractor rejection을 흡수하려면 **래퍼를 직접 만들어야 합니다.** 위에서 한 작업이 그겁니다.

---

## 8. 처음 만난 오류 / 주의점

### 8-1. `variants ... are never constructed` 경고

```
warning: variants `Unauthorized`, `Forbidden`, and `Conflict` are never constructed
```

- study6/7에서 쓸 변형을 미리 선언해 두면 **dead_code 경고**가 뜹니다. 오류는 아니지만 빌드 로그가 지저분해집니다.
- 해결: 해당 변형에 `#[allow(dead_code)]`를 붙이거나, **실제로 쓸 때 추가**합니다.
- Rust는 이런 "쓰이지 않는 코드"를 꽤 집요하게 알려줍니다. 처음엔 잔소리 같지만, 리팩터링 후 남은 죽은 코드를 찾는 데 실제로 유용합니다.

### 8-2. `?`가 안 되는데 "trait `From` is not implemented"

```
error[E0277]: `?` couldn't convert the error to `AppError`
   = help: the trait `From<sqlx::Error>` is not implemented for `AppError`
```

`?`는 **`From`이 있어야만** 자동 변환합니다. 새 오류 종류를 만나면 둘 중 하나를 하세요.

```rust
// 방법 1: AppError에 변형 추가 + #[from]
#[error(transparent)]
Database(#[from] sqlx::Error),

// 방법 2: 그 자리에서 map_err
.map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?
```

study5에서 sqlx를 붙일 때 이 선택을 실제로 하게 됩니다.

### 8-3. `#[error(transparent)]`와 `#[error("{0}")]`의 차이

```rust
#[error(transparent)]
Internal(#[from] anyhow::Error),      // Display = 감싼 에러의 메시지 그대로

#[error("내부 오류: {0}")]
Internal2(#[from] anyhow::Error),     // Display = "내부 오류: <원본 메시지>"
```

`transparent`는 **껍데기를 씌우지 않겠다**는 선언입니다. `Internal`은 어차피 클라이언트에게 메시지를 안 보내므로 껍데기가 의미 없어서 `transparent`를 썼습니다.

---

## 9. 이번 study 정리

- 오류 응답 **계약**(`error` / `message` / `details`)을 먼저 정하고 시작한다.
- `thiserror`로 앱 전용 `enum AppError`를 만들고 **`IntoResponse`를 한 번만** 구현한다. 이게 전역 예외 핸들러를 대신한다.
- `#[from]`을 붙이면 `?`가 자동 변환한다. `thiserror`(구분이 필요한 에러) / `anyhow`(구분이 필요 없는 에러)를 나눠 쓴다.
- **500 응답에는 내부 메시지를 절대 담지 않는다.** 로그에만 남긴다.
- extractor 실패까지 통일하려면 `Path`/`Query`/`Json`을 **감싼 래퍼**를 만들고 `type Rejection = AppError`로 지정한다.
- 라우터 밖의 오류는 `.fallback()`(404)과 `.method_not_allowed_fallback()`(405)로 잡는다.
- `enum` + `match`라서 **에러 종류를 추가하면 처리 누락이 컴파일 에러**로 드러난다. 이게 다른 프레임워크 대비 가장 큰 이점이다.

다음: [study5.md](study5.md) — sqlx + SQLite로 데이터를 실제로 영속화하기.
