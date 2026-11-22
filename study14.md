# study14 — API 설계

> 🎯 **목표**: 프론트와 서버가 오래 함께 갈 수 있는 API의 **규칙**을 정리한다: REST 리소스 설계, OpenAPI(Swagger) 문서화, 버전 관리, 오프셋/커서 페이지네이션, 오류 응답 규칙, 캐시 헤더(ETag). 규칙을 문서에서 끝내지 않고 **실행되는 코드**로 확인한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | 문서화: [demo/src/openapi.rs](demo/src/openapi.rs), 오류 규칙: [demo/src/error.rs](demo/src/error.rs) · 설계 실험: [lab/examples/s14_api_design.rs](lab/examples/s14_api_design.rs) (포트 3014) |
| 검증 | `curl`로 v1/v2·`Deprecation` 헤더·정렬 허용 목록·커서 이동·ETag/304 확인, `GET /api-docs/openapi.json`에서 실제 생성된 경로/스키마 확인 |
| 기준 문서 | **공통 API 스펙 v1** (노션 "HTTP · REST · API 설계" 정리 영역) |

---

## ① 목표

1. 리소스(명사) 중심으로 경로를 설계하고, 메서드·상태 코드를 일관되게 쓴다.
2. 코드에서 OpenAPI 문서를 생성해 **문서와 구현이 어긋나지 않게** 한다.
3. 호환되지 않는 변경을 **버전**으로 분리하고, 옛 버전의 종료를 헤더로 알린다.
4. 오프셋과 커서 페이지네이션을 구분해 고른다.
5. 모든 오류를 같은 모양으로 내고, 내부 사정은 숨긴다.

---

## ② 📖 이론

### REST 설계 규칙 (공통 API 스펙 v1)

| 규칙 | 좋은 예 | 나쁜 예 |
|---|---|---|
| 리소스는 **복수 명사** | `/api/posts`, `/api/posts/{id}` | `/api/getPost`, `/api/post` |
| 소문자 + 하이픈 | `/api/post-tags` | `/api/postTags`, `/api/post_tags` |
| **동사 금지** — 행동은 메서드가 말한다 | `DELETE /api/posts/3` | `POST /api/deletePost` |
| 관계는 중첩 | `GET /api/posts/{id}/comments` | `GET /api/getComments?postId=` |
| 수정은 **PATCH** 하나로 | `PATCH /api/posts/3 {"title":"…"}` | `PUT`(전체 교체)과 `PATCH`가 섞임 |
| 필터·정렬·페이지는 **쿼리** | `?q=axum&sort=-createdAt&page=2&limit=10` | `/api/posts/page/2` |

| 메서드 | 의미 | 안전 | 멱등 | 성공 코드 |
|---|---|---|---|---|
| GET | 조회 | ✅ | ✅ | 200 |
| POST | 생성 / 행동 | ❌ | ❌ (`Idempotency-Key`로 흉내, study5) | **201** + `Location` |
| PATCH | 부분 수정 | ❌ | 보통 ✅ | 200 |
| PUT | 전체 교체 | ❌ | ✅ | 200 |
| DELETE | 삭제 | ❌ | ✅ | **204** (본문 없음) |

### 상태 코드 — 쓰는 것만 정확히

`200` `201` `204` / `400`(요청 불량·검증 실패) `401`(인증 필요) `403`(권한 없음) `404` `409`(충돌) `413` `429` / `500`. 의미가 겹치는 코드를 늘리지 않는다. **401과 403, 400과 409를 헷갈리지 않는 것**이 대부분이다.

### 오프셋 vs 커서 페이지네이션

| | **오프셋** (`?page=2&limit=10`) | **커서** (`?cursor=35&limit=10`) |
|---|---|---|
| SQL | `LIMIT 10 OFFSET 10` | `WHERE id < 35 ORDER BY id DESC LIMIT 11` |
| 장점 | 임의의 페이지로 이동, 전체 페이지 수 | **중복·누락 없음**, 깊은 곳도 일정하게 빠름 |
| 단점 | 보는 사이 새 글이 올라오면 **중복/누락**, 깊을수록 느림(`OFFSET`은 앞을 다 읽고 버린다) | "3페이지로 점프" 불가, 총 개수를 따로 세야 함 |
| 어울리는 곳 | 관리자 화면, 페이지 번호가 보이는 게시판 | 무한 스크롤, 피드, 타임라인 |

### 버전 관리

- **호환되는 변경**(필드 추가)은 버전을 올리지 않는다. 프론트가 모르는 필드를 **무시**하게 만든다.
- **호환되지 않는 변경**(필드 이름 변경·삭제, 응답 모양 변경)은 새 버전(`/api/v2`)으로 낸다.
- 옛 버전은 **갑자기 끄지 않고**, 응답 헤더로 종료를 예고한다: `Deprecation: true`, `Sunset: <날짜>`, `Link: <새 경로>; rel="successor-version"`.
- 경로에 넣는 이유: 브라우저·`curl`·로그·캐시 키에서 **눈에 보인다.**

---

## ③ 🛠️ 실기

### 3-1. OpenAPI 문서 — [demo/src/openapi.rs](demo/src/openapi.rs)

런타임 리플렉션이 없는 Rust에서는 핸들러에 `#[utoipa::path(...)]`로 **명시적으로** 적는다. 손이 가는 대신 상태 코드·설명·보안 요구사항을 정확히 통제한다.

```rust
#[utoipa::path(
    patch, path = "/posts/{id}", tag = "posts",
    params(("id" = i64, Path, description = "게시글 id")),
    request_body = UpdatePost,
    security(("bearer_auth" = [])),                        // 🔒 인증 필요 표시
    responses(
        (status = 200, description = "수정됨", body = Post),
        (status = 403, description = "본인 글이 아님"),
        (status = 404, description = "없는 게시글"),
    )
)]
pub async fn update(...) -> AppResult<Json<Post>> { ... }
```

```rust
#[derive(OpenApi)]
#[openapi(
    info(title = "Axum 학습용 게시판 API", version = "0.1.0", ...),
    paths(auth::handler::register, auth::handler::login, ..., posts::handler::upload_thumbnail),
    components(schemas(Post, CreatePost, UpdatePost, Page<Post>, UserPublic, RegisterRequest, LoginRequest, TokenResponse)),
    modifiers(&SecurityAddon),       // Swagger UI의 "Authorize" 버튼을 만드는 Bearer 스킴 등록
    tags(...)
)]
pub struct ApiDoc;
```

```rust
.merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
```

실제 생성된 결과 (`GET /api-docs/openapi.json`):

```
paths:      /auth/login [post]   /auth/me [get]   /auth/register [post]
            /posts [get, post]   /posts/{id} [get, delete, patch]   /posts/{id}/thumbnail [post]
schemas:    CreatePost, LoginRequest, Page_Post, Post, RegisterRequest, TokenResponse, UpdatePost, UserPublic
security:   bearer_auth → { type: http, scheme: bearer, bearerFormat: JWT }
Post.properties:  authorId, createdAt, id, tags, thumbnail, ...      ← camelCase가 문서에도 반영됨
GET /docs/  → 200 (Swagger UI)
```

`#[serde(rename_all = "camelCase")]`를 **한 번** 적으면 응답 JSON과 OpenAPI 스키마가 **함께** 바뀐다. 문서와 구현이 같은 타입에서 나오므로 어긋나지 않는다.

> ⚠️ 문서가 구현과 어긋나는 곳: 핸들러에 적은 `responses(...)`는 **손으로 쓴 약속**이다. 상태 코드를 코드에서 바꾸고 어트리뷰트를 안 고치면 문서가 거짓말이 된다. 공통 오류 본문 스키마(`ErrorEnvelope`)도 아직 `components`에 등록하지 않았다 (직접 해보기 3). 스펙 v1은 문서 경로를 `/api-docs`로 정했는데 현재는 `/docs`(UI) + `/api-docs/openapi.json`이다.

**문서의 쓸모**: ① 프론트가 `openapi.json`에서 **타입을 생성**해 서버와 어긋나지 않는다 ② Swagger UI(`/docs`)에서 "Authorize"로 토큰을 넣고 바로 호출해 본다 ③ 리뷰·변경 시 계약 차이를 diff로 본다.

### 3-2. 설계 실험 서버 — [lab/examples/s14_api_design.rs](lab/examples/s14_api_design.rs)

**v1 — 스펙 v1 모양 (오프셋, `{ data, meta }`)**

```rust
async fn list_v1(State(state): State<AppState>, Query(q): Query<ListV1>) -> Response {
    let page = q.page.unwrap_or(1);
    if page == 0 { return error(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "입력값을 확인하세요.",
                                Some(json!({ "page": ["1 이상이어야 합니다."] }))); }
    let limit = q.limit.unwrap_or(10).clamp(1, 50);                  // 스펙: 최대 50

    // sort는 허용 목록으로만 받는다. 아무 문자열이나 ORDER BY에 넣으면 인젝션이다.
    let sort = q.sort.as_deref().unwrap_or("-createdAt");
    if !["createdAt", "-createdAt", "-commentCount"].contains(&sort) {
        return error(StatusCode::BAD_REQUEST, "VALIDATION_ERROR", "입력값을 확인하세요.",
                     Some(json!({ "sort": ["createdAt, -createdAt, -commentCount 중 하나여야 합니다."] })));
    }
    // ... 필터(q) → 정렬 → 페이지 자르기
    Json(json!({ "data": data, "meta": { "page": page, "limit": limit, "total": total, "totalPages": total_pages, "q": q.q } }))
}
```

**v2 — 커서**

```rust
async fn list_v2(State(state): State<AppState>, Query(q): Query<ListV2>) -> Json<Value> {
    let limit = q.limit.unwrap_or(10).clamp(1, 50) as usize;
    let mut rows: Vec<&Post> = state.posts.iter().filter(|p| q.cursor.is_none_or(|c| p.id < c)).collect();
    rows.sort_by_key(|p| std::cmp::Reverse(p.id));

    let has_more = rows.len() > limit;                 // 한 건 더 읽어서 "다음이 있는가"를 안다 (COUNT(*) 불필요)
    rows.truncate(limit);
    let next_cursor = if has_more { rows.last().map(|p| p.id) } else { None };

    Json(json!({ "data": rows, "meta": { "limit": limit, "nextCursor": next_cursor, "hasMore": has_more } }))
}
```

**버전 종료 예고 + ETag**

```rust
let v1 = Router::new().route("/posts", get(list_v1)).route("/posts/{id}", get(get_one))
    .layer(middleware::from_fn(deprecation_headers));          // v1 전체에 헤더를 붙인다
let app = Router::new().nest("/api/v1", v1).nest("/api/v2", v2).with_state(state);

async fn deprecation_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert("deprecation", HeaderValue::from_static("true"));
    h.insert("sunset", HeaderValue::from_static("Wed, 31 Dec 2026 23:59:59 GMT"));
    h.insert(header::LINK, HeaderValue::from_static("</api/v2/posts>; rel=\"successor-version\""));
    response
}

// 단건: 내용 지문을 ETag로. 클라이언트가 같은 걸 갖고 있으면 본문 없이 304
if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
    return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
}
```

| 코드 | 설명 |
|---|---|
| `clamp(1, 50)` | 한도를 **서버가** 강제. 클라이언트의 `limit=1000000`이 DB를 괴롭히지 못한다 |
| `sort` 허용 목록 | 값을 SQL의 `ORDER BY`에 이어 붙이는 순간 인젝션. 목록 밖이면 400 |
| `has_more` = `len > limit` | 한 건 더 읽는 요령. 전체 개수 쿼리를 아낀다 |
| `.layer(from_fn(...))`를 v1 라우터에만 | 옛 버전에만 종료 예고 헤더 |
| ETag + `If-None-Match` → **304** | 바뀌지 않았으면 본문을 안 보낸다. 대역폭과 프론트 파싱 비용을 아낀다 (실무에서는 `updatedAt`이나 버전 컬럼으로 만드는 편이 해시 계산보다 싸다) |

### 3-3. 실행 결과

```
$ curl "http://127.0.0.1:3014/api/v1/posts?page=2&limit=5&q=1"
meta: {limit: 5, page: 2, q: '1', total: 13, totalPages: 3}    ids: [16, 15, 14, 13, 12]

$ curl -i "http://127.0.0.1:3014/api/v1/posts?limit=1"  (헤더만)
deprecation: true
sunset: Wed, 31 Dec 2026 23:59:59 GMT
link: </api/v2/posts>; rel="successor-version"

$ curl "http://127.0.0.1:3014/api/v1/posts?sort=title;drop"
{"error":{"code":"VALIDATION_ERROR","fields":{"sort":["createdAt, -createdAt, -commentCount 중 하나여야 합니다."]},"message":"입력값을 확인하세요."}}

$ curl "http://127.0.0.1:3014/api/v2/posts?limit=3"
ids 37, 36, 35     meta: {"hasMore":true,"limit":3,"nextCursor":35}
$ curl "http://127.0.0.1:3014/api/v2/posts?limit=3&cursor=35"
ids 34, 33, 32     meta: {"hasMore":true,"limit":3,"nextCursor":32}      ← 이어서, 겹침 없음

$ curl -i http://127.0.0.1:3014/api/v1/posts/5          → etag: "6c92edf1256d65b2"
$ curl -H 'If-None-Match: "6c92edf1256d65b2"' .../posts/5  → 304 (본문 없음)
```

### 3-4. 오류 응답 규칙 (스펙 v1 §5) — `demo/`의 구현

모든 4xx·5xx는 같은 모양이다. 화면은 **`code`로 분기하고 `fields`를 입력칸에 꽂는다.**

```json
{ "error": { "code": "VALIDATION_ERROR", "message": "입력값을 확인하세요.",
             "fields": { "title": ["제목은 1~100자여야 합니다"] }, "requestId": "772252c7-…" } }
```

| 상태 | `code` | 언제 | 비고 |
|---|---|---|---|
| 400 | `VALIDATION_ERROR` | 검증 실패 | `fields` 포함 |
| 400 | `BAD_REQUEST` | JSON 문법 오류, 숫자가 아닌 id | |
| 401 | `UNAUTHORIZED` | 토큰 없음·만료·위조 | |
| 401 | `INVALID_CREDENTIALS` | 로그인 실패 | **이메일/비밀번호 중 어느 쪽인지 숨김** |
| 403 | `FORBIDDEN` | 남의 글 수정·삭제 | |
| 404 | `NOT_FOUND` | 없는 자원, 없는 경로 | |
| 409 | `EMAIL_TAKEN` | 중복 이메일 | |
| 413 | `PAYLOAD_TOO_LARGE` | 업로드 한도 초과 | |
| 429 | `RATE_LIMITED` | 호출 제한 | 이 저장소는 **미구현** (study15에서 다룰 후보) |
| 500 | `INTERNAL` | 서버 오류 | **스택·예외 원문은 응답에 넣지 않고 로그에만** |

규칙 요약:
1. **한 가지 모양** — 404·405·408·500처럼 `AppError`를 거치지 않는 응답도 `error_envelope` 미들웨어가 같은 모양으로 감싼다 (실측: 404/405/408/패닉 500 모두 `{error:{code,message,requestId}}`).
2. **`code`는 안정적인 계약**, `message`는 사람용이라 바뀔 수 있다. 프론트는 `message`를 파싱하지 않는다.
3. **내부 사정을 숨긴다** — DB 오류·패닉은 고정 문구 + 서버 로그. 응답의 `requestId`로 로그를 찾는다.
4. **같은 실패는 같은 응답** — 없는 아이디/틀린 비밀번호가 같은 `INVALID_CREDENTIALS`.
5. **상태 코드 정규화** — 프레임워크 기본값(422 등)이 스펙과 다르면 한 곳에서 맞춘다 (`AppError::status()`).

---

## ④ 🔗 연동 포인트

| 연동 | 어떻게 |
|---|---|
| **프론트 타입 생성** | `GET /api-docs/openapi.json` → TypeScript 타입 생성 도구(예: `openapi-typescript`) → 프론트가 서버 DTO와 같은 타입. camelCase가 자동 일치 |
| **프론트 서버 상태 캐시** | 경로+쿼리가 캐시 키. `page`/`limit`/`q`/`sort` 이름을 지키면 프론트 코드가 그대로 (study12) |
| **스펙 v1 차이 해소** | [study6 ④](study6.md)의 ❌ 목록 → 이 단계의 규칙으로 하나씩: `/api` 접두어, `{ data, meta }`, `limit`(최대 50), `sort`, 글 `body`, 댓글 |
| **오류 계약 → 화면 분기** | `error.code` 표가 곧 프론트의 `switch` 문. `api.js`의 `ApiError.code` (study12) |
| **버전 → 배포** | v1/v2를 한 서버에서 동시에 서빙하다가 `Sunset` 날짜에 v1 제거. 배포 점검표(study10)에 "옛 버전 호출 로그"를 추가 |
| **ETag → CDN/브라우저 캐시** | `ETag`/`304`, `Cache-Control`을 붙이면 중간 캐시가 활용된다 (study15) |
| **DB** | 커서 페이지네이션은 `WHERE id < $1 ORDER BY id DESC LIMIT n+1` + id 인덱스. 오프셋 `OFFSET`이 큰 값일 때의 비용을 `EXPLAIN`으로 본다 (study11) |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 같은 오류인데 엔드포인트마다 응답 모양이 다름 | 핸들러마다 `(StatusCode, Json(json!(...)))`를 직접 만듦 | 오류는 `AppError` 한 곳에서만 만든다. 우회하는 응답은 `error_envelope`가 잡는다 |
| 목록 응답이 배열이라 "총 몇 건인지"를 알릴 곳이 없음 | 배열을 그대로 반환 | `{ data: [...], meta: { page, limit, total, totalPages } }` |
| `page`가 0이나 음수일 때 `OFFSET` 계산이 패닉 (`(page-1)*limit` 언더플로) | 검증 없이 `u32` 연산 | 입력을 **먼저 검증**(`page >= 1`). `saturating_sub`도 방어선 |
| 깊은 페이지(`page=10000`)가 느림 | `OFFSET`은 앞의 행을 전부 읽고 버린다 | 커서 방식, 또는 최대 페이지 제한 |
| 1페이지와 2페이지에 같은 글이 보임 | 보는 사이 새 글이 추가됨 (오프셋의 약점) | 커서 방식 |
| `?sort=` 값을 `ORDER BY`에 그대로 → 인젝션/500 | 허용 목록 없음 | 허용 목록으로 검증 후 **미리 정해 둔 SQL 조각**으로 매핑 |
| 프론트가 `message`로 분기하다가 문구 수정에 깨짐 | `message`를 계약으로 사용 | `code`로 분기 |
| `DELETE`가 200 + 본문 | 스펙은 **204**, 본문 없음 | 핸들러가 `StatusCode::NO_CONTENT` |
| `POST` 201인데 `Location`이 없음 | 스펙은 `Location: /api/posts/{id}` | `(StatusCode::CREATED, [(header::LOCATION, ...)], Json(...))` |
| 옛 버전을 끄자 프론트 일부가 깨짐 | 예고 없이 제거 | `Deprecation`/`Sunset`을 먼저 내보내고, 로그로 **아직 부르는 클라이언트**를 확인한 뒤 제거 |
| 문서(Swagger)와 실제 상태 코드가 다름 | `responses(...)`는 손으로 쓴 약속 | 핸들러를 고칠 때 어트리뷰트도 함께. 통합 테스트로 계약 검증 |
| ETag가 항상 달라서 304가 안 나옴 | 본문에 매번 바뀌는 값(시각, 난수)이 섞임 | ETag는 **안정적인 내용**(또는 `updatedAt`/버전)에서 |
| `Sunset` 헤더 날짜 형식 오류 | HTTP 날짜 형식(RFC 7231)이 아님 | `Wed, 31 Dec 2026 23:59:59 GMT` |

---

## ⑥ 직접 해보기

1. **스펙 v1 차이 하나 해소**: `demo/`의 `GET /posts` 응답을 `{ data, meta }`로 감싸고 `per_page`를 `limit`(최대 50)으로 바꾼다. 테스트를 먼저 고쳐서 실패시킨 뒤 구현한다. 프론트(`api.js`)가 코드 변경 없이 `res.data`/`res.meta`를 읽는지 확인한다.
2. **불투명 커서**: `nextCursor`를 id 숫자 그대로 주지 말고 `base64(json)`으로 인코딩해 내부 구조를 숨긴다. 잘못된 커서는 400 `BAD_REQUEST`로 처리한다.
3. **오류 스키마를 OpenAPI에 등록**: `ErrorEnvelope`에 `ToSchema`를 달고 `components(schemas(...))`에 넣은 뒤, 각 핸들러의 `responses(...)`에서 `body = ErrorEnvelope`로 참조한다. Swagger UI에서 오류 모양이 보이는지 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ 리소스는 복수 명사, 동사 금지, 수정은 PATCH, 삭제는 204, 생성은 201 + `Location`이라는 규칙을 말할 수 있다
- ☐ 401/403, 400/409, 404의 구분을 예와 함께 설명할 수 있다
- ☐ `#[utoipa::path]`로 문서를 만들고 `/docs`와 `/api-docs/openapi.json`을 열어 확인했다
- ☐ `rename_all = "camelCase"`가 응답과 OpenAPI 스키마에 함께 반영되는 것을 확인했다
- ☐ 오프셋과 커서 페이지네이션의 차이와 각각이 어울리는 곳을 설명할 수 있다
- ☐ `limit` 상한과 `sort` 허용 목록을 **서버에서** 강제하는 이유를 안다
- ☐ `Deprecation`/`Sunset`/`Link` 헤더로 옛 버전의 종료를 예고하는 방법을 안다
- ☐ ETag + `If-None-Match` → 304 흐름을 `curl`로 확인했다
- ☐ 모든 4xx·5xx가 한 가지 모양이고, `code`가 계약이고 `message`는 아니라는 것을 안다 (`AppError`의 JSON 본문 맞춤은 [study6](study6.md) 체크리스트)

---

## ⑧ 다음 단계 연결

API의 모양이 정리되었으니 [study15](study15.md)에서는 **빠르게** 만든다: 느린 쿼리를 캐시하고, 오래 걸리는 일을 백그라운드 작업으로 분리하고, CPU 작업이 서버 전체를 멈추지 않게 격리하고, 이 모든 효과를 **측정**한다.
