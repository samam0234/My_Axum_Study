# JSON 제어와 입력 검증 (Study 3)

[study2.md](study2.md)의 마지막에서 문제를 두 개 남겨뒀습니다.

1. `{"title":""}` 같은 **빈 문자열도 그대로 통과**한다. 파싱은 성공했으니까.
2. 오류 응답이 어떨 땐 JSON, 어떨 땐 평문이라 **클라이언트가 파싱할 수 없다.**

이 문서에서 첫 번째를 해결하고, 두 번째의 **절반**(요청 본문 관련)을 해결합니다. 나머지 절반은 study4에서 마무리합니다.

**이 문서에서 만드는 것**: serde 세부 제어 + `validator` 규칙 + 커스텀 extractor `ValidatedJson<T>`

---

## 1. "파싱"과 "검증"은 다른 단계다

```
{"title":"", "content":"", "author":"w"}
        │
        ▼
 ① 역직렬화 (serde)   "JSON 문법이 맞고, 필드가 다 있고, 타입이 맞는가?"  → ✅ 통과
        │
        ▼
 ② 검증 (validator)   "그 값이 **말이 되는가**? 길이는? 형식은?"           → ❌ 실패해야 함
        │
        ▼
    핸들러 본문
```

study2까지는 ①만 있었습니다. `title: String`이니까 빈 문자열도 정당한 `String`이고, serde는 통과시킵니다.

| 프레임워크 | ① 역직렬화 | ② 검증 |
|---|---|---|
| FastAPI | Pydantic | Pydantic (같은 도구가 둘 다) |
| NestJS | `class-transformer` | `class-validator` + `ValidationPipe` |
| Spring | Jackson | Bean Validation (`@Valid` + `@NotBlank`) |
| **Axum** | **serde** | **`validator` 크레이트 (직접 호출해야 함)** |

**Axum에는 "검증 파이프"가 없습니다.** `validator`는 그냥 `value.validate()`를 호출하는 라이브러리일 뿐이고, **그걸 요청 처리 흐름에 끼워 넣는 건 내 몫**입니다. 그 끼워 넣는 자리가 바로 **커스텀 extractor**입니다.

---

## 2. serde 세부 제어

검증에 들어가기 전에, ① 단계에서 할 수 있는 것부터 정리합니다.

### 자주 쓰는 어트리뷰트

```rust
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]      // created_at → createdAt (구조체 전체)
#[serde(deny_unknown_fields)]           // 모르는 필드가 오면 거부
struct Example {
    #[serde(default)]                   // 없으면 Default::default()
    tags: Vec<String>,

    #[serde(rename = "id")]             // 개별 필드 이름 변경
    identifier: i64,

    #[serde(skip_serializing_if = "Option::is_none")]  // None이면 응답에서 생략
    nickname: Option<String>,

    #[serde(skip)]                      // 직렬화/역직렬화 모두에서 제외
    internal_flag: bool,

    #[serde(skip_serializing)]          // 응답에는 안 나감 (비밀번호 해시 등)
    password_hash: String,
}
```

| 어트리뷰트 | 용도 | 다른 곳의 대응 |
|---|---|---|
| `rename_all = "camelCase"` | Rust는 snake_case, JS는 camelCase — 경계에서 변환 | Jackson `@JsonNaming`, Pydantic `alias_generator` |
| `deny_unknown_fields` | 오타로 보낸 필드를 조용히 무시하지 않음 | Pydantic `extra="forbid"`, Nest `forbidNonWhitelisted` |
| `default` | 선택 필드 | Pydantic 기본값 |
| `skip_serializing` | 응답에서 감추기 | TypeORM `select: false`, Prisma `omit` |

### 이번 실습에서 쓴 것

```rust
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]           // ← 오타를 잡기 위해
pub struct CreatePost {
    pub title: String,
    pub content: String,
    pub author: String,

    #[serde(default)]                   // ← 안 보내면 빈 배열
    pub tags: Vec<String>,
}
```

`deny_unknown_fields`를 켜면 `{"titel": "오타"}`가 **조용히 무시되는 대신 오류**가 됩니다. 실제 실행 결과:

```
422 | {"error":"invalid_body","message":"Failed to deserialize the JSON body into the target type:
       titel: unknown field `titel`, expected one of `title`, `content`, `author`, `tags`
       at line 1 column 51"}
```

**기대한 필드 이름을 전부 알려줍니다.** 프론트엔드 개발자가 디버깅하기 아주 좋은 메시지입니다.

> ⚠️ 단, API를 공개했다면 `deny_unknown_fields`는 **하위 호환을 깨기 쉽습니다.** 새 클라이언트가 새 필드를 보내면 구 서버가 거부합니다. 내부 API나 학습용에서는 켜고, 공개 API에서는 신중하게 판단하세요.

---

## 3. `validator` 크레이트

```powershell
cargo add validator --features derive
```

### 규칙 선언

```rust
#[derive(Debug, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct CreatePost {
    #[validate(length(min = 1, max = 100, message = "제목은 1~100자여야 합니다"))]
    pub title: String,

    #[validate(length(min = 1, max = 5000, message = "본문은 1~5000자여야 합니다"))]
    pub content: String,

    #[validate(length(min = 2, max = 20, message = "작성자는 2~20자여야 합니다"))]
    pub author: String,

    #[serde(default)]
    #[validate(length(max = 5, message = "태그는 최대 5개까지 가능합니다"))]
    pub tags: Vec<String>,
}
```

### 주요 규칙

| 규칙 | 예시 | Bean Validation 대응 |
|---|---|---|
| `length` | `length(min = 1, max = 100)` | `@Size` |
| `range` | `range(min = 0, max = 150)` | `@Min` / `@Max` |
| `email` | `email` | `@Email` |
| `url` | `url` | `@URL` |
| `regex` | `regex(path = "*RE_SLUG")` | `@Pattern` |
| `contains` | `contains(pattern = "@")` | — |
| `must_match` | `must_match(other = "password")` | 비밀번호 확인 |
| `custom` | `custom(function = "check_forbidden")` | `@AssertTrue` |
| `nested` | `nested` | `@Valid` (중첩 객체) |

### `Option<T>`에 규칙을 달면?

```rust
#[validate(length(min = 1, max = 100))]
pub title: Option<String>,
```

**`Some`일 때만 검사합니다.** `None`은 그냥 통과합니다. 부분 수정(PATCH) DTO에 딱 맞는 동작입니다.

### `length`가 세는 건 "문자" 수

`String::len()`은 **바이트** 수라서 한글 1글자가 3을 차지합니다. 하지만 `validator`의 `length`는 **`chars().count()`** 를 쓰므로 한글도 1글자로 셉니다. 의도한 대로 동작합니다.

---

## 4. 커스텀 Extractor — `ValidatedJson<T>`

여기가 이번 study의 핵심입니다.

### 왜 필요한가

`validate()`를 핸들러마다 직접 호출할 수도 있습니다.

```rust
pub async fn create(Json(body): Json<CreatePost>) -> ... {
    body.validate().map_err(|e| /* 응답 만들기 */)?;   // ← 매번 이걸 쓴다고?
    // ...
}
```

- 빠뜨리기 쉽고, 오류 응답 만드는 코드가 핸들러마다 중복됩니다.
- **extractor로 만들면 "타입을 바꾸는 것"만으로 검증이 적용**됩니다.

```rust
// Before
pub async fn create(Json(body): Json<CreatePost>) -> ...

// After — 핸들러 본문은 한 글자도 안 바뀐다
pub async fn create(ValidatedJson(body): ValidatedJson<CreatePost>) -> ...
```

### 구현 — `src/extract.rs`

```rust
use axum::{
    Json,
    extract::{FromRequest, Request, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use validator::Validate;

#[derive(Debug, Clone, Copy, Default)]
pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ValidatedJsonRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // 1단계: 기본 Json extractor 재사용
        let Json(value) = Json::<T>::from_request(req, state).await?;
        // 2단계: validator 규칙 검사
        value.validate()?;
        Ok(ValidatedJson(value))
    }
}
```

**읽는 법:**

| 부분 | 의미 |
|---|---|
| `FromRequest<S>` | **본문을 소비하는** extractor용 trait. 마지막 인자에만 쓸 수 있다 |
| `FromRequestParts<S>` | 본문을 안 건드리는 extractor용 trait (헤더, 경로 등). 위치 제한 없음 |
| `T: DeserializeOwned + Validate` | "역직렬화 가능하고 검증 규칙을 가진 타입"에만 적용 |
| `S: Send + Sync` | 어떤 상태 타입이든 상관없음 (상태를 안 쓰므로) |
| `type Rejection` | **실패했을 때의 타입.** `IntoResponse`여야 한다 |
| `Json::<T>::from_request(...)` | **기본 extractor를 안에서 재사용**한다. 바퀴를 다시 발명하지 않는다 |

> 💡 axum 0.8에서는 trait 안에 `async fn`을 그대로 쓸 수 있습니다. 0.7 이전 예제에 붙어 있는 `#[async_trait]` 매크로는 이제 필요 없습니다. (Rust 1.75의 AFIT 안정화 덕분)

### 실패 타입 — 두 종류를 하나로

```rust
#[derive(Debug)]
pub enum ValidatedJsonRejection {
    Json(JsonRejection),                       // 파싱 실패
    Validation(validator::ValidationErrors),   // 검증 실패
}

impl From<JsonRejection> for ValidatedJsonRejection {
    fn from(value: JsonRejection) -> Self { Self::Json(value) }
}

impl From<validator::ValidationErrors> for ValidatedJsonRejection {
    fn from(value: validator::ValidationErrors) -> Self { Self::Validation(value) }
}
```

**`From`을 구현하는 이유**가 중요합니다. `from_request` 본문의 `?` 두 개를 다시 보세요.

```rust
let Json(value) = Json::<T>::from_request(req, state).await?;   // JsonRejection
value.validate()?;                                              // ValidationErrors
```

서로 다른 오류 타입인데 함수 반환 타입은 하나입니다. **`?`는 `From`이 구현되어 있으면 자동으로 변환**해 줍니다. 그래서 `From`만 써두면 `?` 한 글자로 끝납니다. — **Rust 오류 처리의 핵심 관용구**이고, study4에서 이 패턴을 앱 전체로 확장합니다.

### 응답 만들기

```rust
impl IntoResponse for ValidatedJsonRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Json(rejection) => (
                rejection.status(),        // axum이 정한 코드를 살린다 (400/415/422)
                Json(json!({
                    "error": "invalid_body",
                    "message": rejection.body_text(),
                })),
            ).into_response(),

            Self::Validation(errors) => {
                // ValidationErrors → { 필드명: [메시지...] }
                let details: HashMap<String, Vec<String>> = errors
                    .field_errors()
                    .into_iter()
                    .map(|(field, errs)| {
                        let messages = errs.iter()
                            .map(|e| e.message.as_ref()
                                .map(|m| m.to_string())
                                .unwrap_or_else(|| e.code.to_string()))
                            .collect();
                        (field.to_string(), messages)
                    })
                    .collect();

                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({
                        "error": "validation_failed",
                        "message": "요청 값이 규칙에 맞지 않습니다.",
                        "details": details,
                    })),
                ).into_response()
            }
        }
    }
}
```

- `rejection.status()`와 `rejection.body_text()`는 `JsonRejection`이 제공하는 메서드입니다. **axum이 이미 잘 판단한 상태 코드를 버리지 않고**, 본문 형식만 JSON으로 감쌉니다.
- `e.message`가 `None`일 수 있어서 `unwrap_or_else(|| e.code.to_string())`으로 대비합니다. `message = "..."`를 안 적은 규칙은 `"length"` 같은 코드만 들어 있습니다.

---

## 5. 실행 및 검증

| # | 요청 본문 | 응답 |
|---|---|---|
| 1 | `{"title":"Valid post","content":"body text","author":"while","tags":["rust","axum"]}` | `201` `{"id":1,...,"tags":["rust","axum"],...}` |
| 2 | `{"title":"No tags","content":"body","author":"while"}` | `201` `{"id":2,...,"tags":[],...}` ← `#[serde(default)]` 동작 |
| 3 | `{"title":"","content":"","author":"w"}` | `422` (아래 참고) |
| 4 | `{...,"tags":["1","2","3","4","5","6"]}` | `422` `{"details":{"tags":["태그는 최대 5개까지 가능합니다"]},...}` |
| 5 | `{...,"titel":"typo"}` | `422` `{"error":"invalid_body","message":"... unknown field 'titel', expected one of 'title', 'content', 'author', 'tags' ..."}` |
| 6 | `{"title": ` (깨진 JSON) | `400` `{"error":"invalid_body","message":"Failed to parse the request body as JSON: ..."}` |
| 7 | `Content-Type: text/plain` | `415` `{"error":"invalid_body","message":"Expected request with 'Content-Type: application/json'"}` |
| 8 | `PATCH {"title":""}` | `422` `{"details":{"title":["제목은 1~100자여야 합니다"]},...}` |
| 9 | `PATCH {"tags":["edited"]}` | `200` (제목·본문은 그대로, 태그만 교체) |

3번의 전체 응답:

```json
{
  "error": "validation_failed",
  "message": "요청 값이 규칙에 맞지 않습니다.",
  "details": {
    "author": ["작성자는 2~20자여야 합니다"],
    "content": ["본문은 1~5000자여야 합니다"],
    "title": ["제목은 1~100자여야 합니다"]
  }
}
```

**확인할 것 3가지:**

1. **여러 필드의 오류가 한 번에 나옵니다.** 첫 번째 실패에서 멈추지 않기 때문에, 사용자가 폼을 여러 번 왕복할 필요가 없습니다.
2. **본문 관련 오류(400/415/422)가 전부 같은 JSON 껍데기**를 갖게 되었습니다. `error` 필드로 종류를 구분할 수 있습니다.
3. **한글 메시지가 깨지지 않습니다.** axum의 `Json` 응답은 `Content-Type: application/json`을 붙이고 UTF-8로 직렬화합니다.

### 아직 남은 문제

```
GET /posts/abc  → 400 | Invalid URL: Cannot parse `abc` to a `i64`     ← 평문 그대로
GET /posts/999  → 404 | {"error":"not_found", ...}                     ← 내가 만든 JSON
GET /nope       → 404 | (본문 없음)                                      ← 빈 응답
```

**본문 밖의 오류는 아직 제각각입니다.** `Path`/`Query` extractor의 실패, 404, 405, 그리고 앞으로 생길 DB 오류까지 전부 한 형식으로 묶어야 합니다. → **study4.**

---

## 6. 처음 만난 오류

### 6-1. `HashMap<&str, _> cannot be built from an iterator over (Cow<'_, str>, _)`

```rust
let details: HashMap<&str, Vec<String>> = errors.field_errors().into_iter()
    .map(|(field, errs)| { ... (field, messages) })
    .collect();
```

```
error[E0277]: a value of type `HashMap<&str, Vec<String>>` cannot be built from
              an iterator over elements of type `(Cow<'_, str>, _)`
  = help: for that trait implementation, expected `&str`, found `Cow<'_, str>`
```

- `validator` 0.21의 `field_errors()` 키 타입이 `&str`이 아니라 **`Cow<'static, str>`** 입니다.
- `Cow`(Clone-On-Write)는 **"빌린 값일 수도 있고 소유한 값일 수도 있다"** 를 하나의 타입으로 표현합니다. 라이브러리가 정적 문자열(`&'static str`)과 동적으로 만든 `String`을 섞어 반환할 때 씁니다.
- 해결: 맵 타입을 `HashMap<String, Vec<String>>`으로 바꾸고 `field.to_string()`.

> 이 에러가 좋은 예인 이유: **Rust 컴파일러는 "기대한 타입 / 실제 타입 / 대신 구현된 것"까지 다 알려줍니다.** 메시지를 끝까지 읽으면 대부분 해결책이 그 안에 있습니다.

### 6-2. `Json<T>`와 `ValidatedJson<T>`를 한 핸들러에 같이 쓰면?

둘 다 본문을 소비하므로 **둘 다 마지막 인자일 수 없습니다.** 컴파일 에러가 납니다. 본문 extractor는 핸들러당 딱 하나입니다.

### 6-3. `#[derive(Validate)]`를 빠뜨리면

```
error[E0277]: the trait bound `CreatePost: Validate` is not satisfied
```

`ValidatedJson<T>`의 `where T: DeserializeOwned + Validate` 제약에 걸립니다. **에러가 extractor 정의가 아니라 사용 지점에서** 나므로, 메시지의 `required by a bound in ...` 줄을 보면 원인을 찾을 수 있습니다.

---

## 7. 이번 study 정리

- **파싱(serde)과 검증(validator)은 별개 단계**다. `String`이면 빈 문자열도 파싱은 통과한다.
- serde 어트리뷰트(`rename_all`, `default`, `deny_unknown_fields`, `skip_serializing`)로 JSON 경계를 세밀하게 제어한다.
- `validator`의 `#[validate(...)]`로 규칙을 선언하고, `Option<T>`에 붙이면 `Some`일 때만 검사한다.
- **Axum에는 검증 파이프가 없다.** `FromRequest`를 구현한 **커스텀 extractor**가 그 자리를 대신한다.
- extractor를 만들 때는 **기본 extractor를 안에서 재사용**하고, 여러 실패를 `enum` + `From`으로 모아 `?`로 전파한다.
- `FromRequest`(본문 소비, 마지막 인자 전용) vs `FromRequestParts`(그 외) 구분을 기억한다. — study6에서 `FromRequestParts`로 인증 extractor를 만든다.

다음: [study4.md](study4.md) — 앱 전체의 오류를 `AppError` 하나로 통일하기.
