# 페이지네이션 · 파일 업로드 · 정적 서빙 (Study 8 · 고급 3)

게시글이 25개만 되어도 `GET /posts`가 전부 다 내려주는 건 곤란합니다. 그리고 게시판이라면 썸네일 이미지도 올릴 수 있어야 합니다.

**이 문서에서 만드는 것**: 오프셋 페이지네이션 + 검색/태그 필터(`QueryBuilder`) + multipart 업로드 + `ServeDir` 정적 서빙

```powershell
cargo add axum --features multipart
cargo add tower-http --features fs
cargo add uuid --features v4
cargo add tokio --features fs
```

---

## 1. 페이지네이션

### 응답 껍데기부터 정한다

```rust
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
    pub total_pages: u32,
    pub has_next: bool,
}
```

배열을 그대로 반환하면 "전체가 몇 건인지", "다음 페이지가 있는지"를 알려줄 자리가 없습니다. **껍데기(envelope)로 감싸는 게 표준적인 해법**입니다.

> ⚠️ **응답 형식이 바뀌는 breaking change**입니다. study5까지는 `GET /posts`가 배열이었는데 이제 객체입니다. 실제 서비스라면 `/v2/posts`를 새로 내거나, 쿼리 파라미터가 있을 때만 감싸는 식으로 처리합니다.

### 쿼리 파라미터

```rust
#[derive(Debug, Deserialize, Validate)]
pub struct PageQuery {
    #[serde(default = "default_page")]
    #[validate(range(min = 1, message = "page는 1 이상이어야 합니다"))]
    pub page: u32,

    #[serde(default = "default_per_page")]
    #[validate(range(min = 1, max = 100, message = "per_page는 1~100이어야 합니다"))]
    pub per_page: u32,

    pub q: Option<String>,     // 검색어
    pub tag: Option<String>,   // 태그 필터
}
```

- **`#[serde(default = "함수명")]`** — 파라미터가 없을 때 쓸 기본값을 함수로 지정합니다. `Option<u32>`로 받고 `unwrap_or(1)` 하는 것보다 깔끔합니다.
- **`max = 100`이 중요합니다.** 상한이 없으면 `?per_page=1000000`으로 서버 메모리를 고갈시킬 수 있습니다. **페이지네이션에 상한을 두는 건 성능이 아니라 보안 문제**입니다.

검증을 적용하려면 `ValidatedQuery<T>` extractor가 필요합니다. study3에서 만든 `ValidatedJson`과 같은 패턴인데, 본문이 아니라 쿼리스트링이므로 **`FromRequestParts`** 로 구현합니다.

```rust
impl<T, S> FromRequestParts<S> for ValidatedQuery<T>
where T: DeserializeOwned + Validate, S: Send + Sync
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let axum::extract::Query(value) = axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|r| AppError::BadRequest(r.body_text()))?;
        value.validate()?;
        Ok(ValidatedQuery(value))
    }
}
```

### 오프셋 vs 커서

| | 오프셋 (`LIMIT ? OFFSET ?`) | 커서 (`WHERE id < ? LIMIT ?`) |
|---|---|---|
| 구현 | 쉬움 | 조금 복잡 |
| "3페이지로 점프" | ✅ | ❌ (순차 이동만) |
| 전체 건수 | ✅ | 보통 안 줌 |
| **깊은 페이지 성능** | ❌ **`OFFSET 100000`이면 10만 행을 읽고 버림** | ✅ 항상 일정 |
| **데이터가 바뀔 때** | ❌ 새 글이 추가되면 **항목이 밀려서 중복/누락** | ✅ 안전 |

이번엔 **오프셋**을 씁니다. 게시판처럼 "1 2 3 … 9" 페이지 번호를 보여줘야 하는 UI에는 이게 맞습니다. 무한 스크롤 피드라면 커서가 맞습니다.

> 커서 방식은 studyFinal에서 다루지 않지만, 개념은 간단합니다. `?after=23&limit=10` → `WHERE id < 23 ORDER BY id DESC LIMIT 10`. 응답에 마지막 항목의 id를 `next_cursor`로 실어 보냅니다.

---

## 2. ⭐ `QueryBuilder` — 동적 SQL을 안전하게

study6에서 만난 에러를 기억하시나요?

```
error[E0277]: dynamic SQL strings should be audited for possible injections
   = help: the trait `SqlSafeStr` is not implemented for `&String`
   = note: prefer literal SQL strings with bind parameters or `QueryBuilder`
```

**sqlx 0.9는 런타임에 만든 문자열을 SQL로 받지 않습니다.** 그런데 검색어와 태그 필터는 있을 수도 없을 수도 있어서 조건이 동적입니다. 이럴 때 쓰라고 있는 게 `QueryBuilder`입니다.

```rust
fn push_filters(builder: &mut QueryBuilder<sqlx::Sqlite>, query: &PageQuery) {
    if let Some(q) = query.q.as_deref().filter(|s| !s.trim().is_empty()) {
        let pattern = format!("%{}%", q.trim());
        builder.push(" AND (title LIKE ");
        builder.push_bind(pattern.clone());     // ← 값은 bind로
        builder.push(" OR content LIKE ");
        builder.push_bind(pattern);
        builder.push(")");
    }

    if let Some(tag) = query.tag.as_deref().filter(|s| !s.trim().is_empty()) {
        let needle = format!("%\"{}\"%", tag.trim());
        builder.push(" AND tags LIKE ");
        builder.push_bind(needle);
    }
}
```

### 핵심: `push`와 `push_bind`가 다르다

| 메서드 | 넣는 것 | 결과 |
|---|---|---|
| `.push("...")` | **SQL 조각** | 쿼리 문자열에 그대로 이어 붙음 |
| `.push_bind(value)` | **값** | `?` 자리표시자가 들어가고, 값은 별도로 전달됨 |

즉 **사용자 입력은 절대 SQL 문자열에 닿지 않습니다.** `q`에 `' OR 1=1 --`를 넣어도 그냥 "그런 문자열을 포함한 글"을 찾을 뿐입니다. (아래 실행 결과 8번에서 확인)

```rust
let mut builder = QueryBuilder::<sqlx::Sqlite>::new(
    "SELECT id, title, ... FROM posts WHERE 1 = 1",   // ← WHERE 1 = 1 관용구
);
push_filters(&mut builder, query);
builder.push(" ORDER BY id DESC LIMIT ");
builder.push_bind(query.limit());
builder.push(" OFFSET ");
builder.push_bind(query.offset());

let items = builder.build_query_as::<PostRow>().fetch_all(db).await?;
```

- **`WHERE 1 = 1`** 은 "첫 조건이냐 아니냐를 따지지 않고 무조건 `AND ...`를 이어붙이기 위한" 오래된 관용구입니다. 옵티마이저가 무시하므로 성능 영향이 없습니다.
- 전체 건수는 같은 필터로 `COUNT(*)`를 한 번 더 날립니다. `build_query_scalar()`가 단일 값을 꺼내는 메서드입니다.

> 💡 페이지 조회와 COUNT, **쿼리가 두 번** 나갑니다. 트래픽이 크면 COUNT가 병목이 됩니다. 대안: 근사치 사용, 캐싱, 또는 커서 페이지네이션으로 전환해 COUNT를 아예 없애기.

### `QueryBuilder<sqlx::Sqlite>`에 수명 인자를 붙이면 에러

```rust
fn push_filters(builder: &mut QueryBuilder<'_, sqlx::Sqlite>, ...)   // ❌
```

```
error[E0107]: struct takes 0 lifetime arguments but 1 lifetime argument was supplied
note: struct defined here, with 0 lifetime parameters
  --> sqlx-core-0.9.0\src\query_builder.rs:28:12
   | pub struct QueryBuilder<DB>
```

sqlx **0.8까지는 `QueryBuilder<'a, DB>`** 였는데 **0.9에서 수명 인자가 사라졌습니다.** 인터넷 예제를 그대로 쓰면 이 에러가 납니다. 컴파일러가 "정의는 여기, 수명 파라미터 0개"라고 정확히 알려주므로 금방 고칠 수 있습니다.

---

## 3. 파일 업로드 (multipart)

```powershell
cargo add axum --features multipart
```

### 라우트만 본문 한도를 키운다

```rust
.route(
    "/{id}/thumbnail",
    post(handler::upload_thumbnail).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
)
```

study7에서 전역 본문 한도를 2MB로 잡았습니다. 이미지는 그보다 클 수 있으니 **이 라우트만** 5MB로 올립니다. **더 안쪽에 붙은 `DefaultBodyLimit`이 이깁니다.**

> 반대 방향도 유용합니다. 로그인처럼 본문이 작아야 정상인 엔드포인트에 `DefaultBodyLimit::max(4 * 1024)`를 걸면 공격 표면이 줄어듭니다.

### ⭐ 저장 로직 — 보안이 전부다

```rust
pub async fn save_image(mut multipart: Multipart) -> AppResult<String> {
    while let Some(field) = multipart.next_field().await
        .map_err(|e| AppError::BadRequest(format!("multipart 파싱 실패: {e}")))?
    {
        if field.name() != Some("file") { continue; }

        // ① Content-Type 화이트리스트
        let content_type = field.content_type().map(|s| s.to_string()).unwrap_or_default();
        let extension = ALLOWED.iter()
            .find(|(mime, _)| *mime == content_type)
            .map(|(_, ext)| *ext)
            .ok_or_else(|| AppError::BadRequest(format!(
                "지원하지 않는 파일 형식입니다: {content_type} (jpeg/png/gif/webp만 가능)"
            )))?;

        // ② 크기 검사
        let bytes = field.bytes().await
            .map_err(|e| AppError::BadRequest(format!("파일을 읽지 못했습니다: {e}")))?;
        if bytes.is_empty() { return Err(AppError::BadRequest("빈 파일입니다".into())); }
        if bytes.len() > MAX_UPLOAD_BYTES { /* 413 상당 */ }

        // ③ ⭐ 파일명을 새로 만든다. 클라이언트가 준 이름은 쓰지 않는다.
        let filename = format!("{}.{extension}", uuid::Uuid::new_v4());
        let path = Path::new(UPLOAD_DIR).join(&filename);

        let mut file = tokio::fs::File::create(&path).await?;
        file.write_all(&bytes).await?;

        return Ok(filename);
    }

    Err(AppError::BadRequest("`file` 이름의 파일 필드가 필요합니다".into()))
}
```

### 업로드 보안 체크리스트

| # | 위협 | 대응 |
|---|---|---|
| 1 | **경로 조작** — `../../../etc/passwd` | **파일명을 UUID로 새로 생성.** 클라이언트 이름을 버린다 |
| 2 | 확장자 위장 — `evil.php`, `shell.jsp` | 확장자를 **검증된 MIME에서** 유도 |
| 3 | 크기 폭탄 | `DefaultBodyLimit` + 명시적 길이 검사 |
| 4 | 디스크 고갈 | 사용자당 개수/용량 제한 (이번엔 미구현) |
| 5 | 저장된 파일의 실행 | 업로드 폴더를 **정적 파일 전용**으로 두고 실행 권한 제거 |
| 6 | 내용 위장 — png로 신고된 스크립트 | 매직 넘버 검사(`infer` 크레이트), 이미지 재인코딩 |

**1번이 가장 중요합니다.** 클라이언트가 보낸 파일명을 그대로 `Path::join`에 넣으면 업로드 폴더 밖에 파일을 쓸 수 있습니다.

실제로 `../../../etc/passwd.png`라는 이름으로 올려봤더니 (아래 실행 결과 4번):

```
thumbnail: "46ebc483-5509-4d40-99b0-13392a3da64f.png"
```

**파일명이 통째로 무시되고 UUID가 쓰였습니다.** 이게 제일 확실한 방어입니다 — 위험한 입력을 "검사"하는 대신 **아예 안 씁니다.**

> ⚠️ `field.content_type()`은 **클라이언트가 보낸 값**이라 얼마든지 위조할 수 있습니다. 그래서 6번(매직 넘버 검사)이 따로 필요합니다. 다만 우리는 확장자를 이 값에서 유도할 뿐 **파일을 실행하지 않으므로** 실질 위험은 낮습니다.

### 새 파일 먼저, 기존 파일은 나중에

```rust
let filename = upload::save_image(multipart).await?;

// 새 파일을 저장한 뒤 기존 파일을 지운다. (순서가 반대면 실패 시 둘 다 잃는다)
if let Some(old) = existing.thumbnail {
    upload::remove_if_exists(&old).await;
}
```

**DB와 파일 시스템은 같은 트랜잭션에 묶을 수 없습니다.** 어느 쪽이 먼저 실패해도 덜 나쁜 순서를 골라야 합니다. "새 파일 저장 → DB 갱신 → 옛 파일 삭제"가 최악의 경우 **고아 파일 하나**로 끝납니다.

---

## 4. 정적 파일 서빙 — `ServeDir`

```rust
.nest_service("/uploads", ServeDir::new(upload::UPLOAD_DIR))
```

- **`nest`가 아니라 `nest_service`** 입니다. `ServeDir`은 `Router`가 아니라 tower `Service`이기 때문입니다.
- `ServeDir`은 MIME 타입 추론, `Range` 요청(이어받기), `If-Modified-Since` 캐싱, **경로 조작 방어**를 다 해줍니다.

실측:

```
GET /uploads/25d45003-b0e7-4392-8310-d387d498bad7.png
→ 200  content-type: image/png  bytes: 70

GET /uploads/nope.png       → 404
GET /uploads/../data.db     → 404
```

> 마지막 줄은 HTTP 클라이언트가 URL을 먼저 정규화해 버리기 때문에 서버까지 `../`가 도달하지 않을 수도 있습니다. **`ServeDir` 자체도 정규화 후 루트를 벗어나는 경로를 거부**하므로 이중으로 막혀 있습니다.

SPA를 서빙하려면 이렇게 씁니다. (studyFinal에서 실제로 사용)

```rust
.fallback_service(
    ServeDir::new("static").not_found_service(ServeFile::new("static/index.html"))
)
```

> 운영에서는 정적 파일을 Nginx/CDN에 맡기는 게 보통입니다. 하지만 **작은 서비스라면 Rust 바이너리 하나로 API + 정적 파일을 다 서빙**하는 것도 충분히 현실적입니다. `rust-embed` 크레이트를 쓰면 정적 파일을 바이너리에 넣어버릴 수도 있습니다.

---

## 5. 실행 및 검증

게시글 25개를 만들고(짝수 id는 `rust` 태그, 홀수는 `axum`) 테스트했습니다.

### 페이지네이션

| # | 요청 | 결과 |
|---|---|---|
| 1 | `?page=1&per_page=3` | `items`=[25,24,23] `total`=25 `total_pages`=9 `has_next`=true |
| 2 | `?page=2&per_page=3` | `items`=[22,21,20] `has_next`=true |
| 3 | `?page=9&per_page=3` | `items`=[1] `has_next`=**false** ← 마지막 페이지 |
| 4 | `?tag=rust&per_page=2` | `items`=[24,22] `total`=**12** ← 필터가 COUNT에도 적용됨 |
| 5 | `?q=Post 24` | `items`=[24] `total`=1 |
| 6 | `?page=0` | `422` `{"details":{"page":["page는 1 이상이어야 합니다"]}}` |
| 7 | `?per_page=1000` | `422` `{"details":{"per_page":["per_page는 1~100이어야 합니다"]}}` |
| 8 | `?q=' OR 1=1 --` | `200` `{"items":[],"total":0,...}` ← **SQL 인젝션 무력화** |

**8번이 핵심입니다.** 고전적인 인젝션 페이로드를 넣었는데 **에러도 안 나고, 전체 목록도 안 나오고, 그냥 "그런 문자열을 포함한 글이 0건"** 이라는 정직한 결과가 나왔습니다. `push_bind` 덕분입니다.

**4번도 잘 보세요.** `total`이 25가 아니라 **12**입니다. 필터가 목록 쿼리와 COUNT 쿼리에 **똑같이** 적용되었습니다. 이걸 빠뜨리면 "1페이지만 있는데 페이지 번호는 9까지 보이는" 흔한 버그가 생깁니다.

### 파일 업로드

| # | 요청 | 결과 |
|---|---|---|
| 1 | PNG를 `file` 필드로 업로드 | `200` `{"thumbnail":"25d45003-b0e7-4392-8310-d387d498bad7.png",...}` |
| 2 | `text/plain`을 올림 | `400` `{"error":"bad_request","message":"지원하지 않는 파일 형식입니다: text/plain (jpeg/png/gif/webp만 가능)"}` |
| 3 | 필드 이름을 `image`로 | `400` `{"error":"bad_request","message":"'file' 이름의 파일 필드가 필요합니다"}` |
| 4 | 파일명 `../../../etc/passwd.png` | `200` `{"thumbnail":"46ebc483-5509-4d40-99b0-13392a3da64f.png",...}` ← **경로 무시됨** |

저장된 파일:

```
uploads/
├─ 25d45003-b0e7-4392-8310-d387d498bad7.png
└─ 46ebc483-5509-4d40-99b0-13392a3da64f.png
```

**`etc/` 폴더도, `passwd.png`도 만들어지지 않았습니다.**

### 정적 서빙

```
GET /uploads/25d45003-b0e7-4392-8310-d387d498bad7.png
→ 200  content-type: image/png  bytes: 70
```

---

## 6. 처음 만난 오류 / 주의점

### 6-1. `QueryBuilder`의 수명 인자 (위 2절)

sqlx 0.9에서 `QueryBuilder<'a, DB>` → `QueryBuilder<DB>`로 바뀌었습니다.

### 6-2. `warning: function 'list' is never used`

페이지네이션을 넣으면서 기존 `repo::list()`가 안 쓰이게 되었는데, 컴파일러가 바로 알려줬습니다.

```
warning: function `list` is never used
  --> src\posts\repo.rs:18:14
```

**리팩터링하면서 죽은 코드를 남기지 않게 해주는 잔소리**입니다. 삭제했습니다.

### 6-3. multipart extractor의 위치

`Multipart`는 본문을 소비하므로 **마지막 인자**여야 합니다.

```rust
pub async fn upload_thumbnail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    multipart: Multipart,   // ← 반드시 마지막
) -> AppResult<Json<Post>>
```

순서를 틀리면 study1에서 본 "Handler trait is not satisfied" 에러가 납니다.

### 6-4. `field.bytes()`는 전체를 메모리에 올린다

작은 이미지면 괜찮지만, 큰 파일이면 **스트리밍**으로 디스크에 직접 써야 합니다.

```rust
// 메모리에 다 올림 (현재 구현)
let bytes = field.bytes().await?;

// 스트리밍 (대용량이라면)
while let Some(chunk) = field.chunk().await? {
    file.write_all(&chunk).await?;
}
```

스트리밍이면 크기 제한을 **직접 누적해서** 검사해야 합니다(`DefaultBodyLimit`이 1차 방어를 해주긴 합니다).

### 6-5. PowerShell 5.1에는 `Invoke-WebRequest -Form`이 없다

multipart 업로드 테스트를 하려면 .NET `HttpClient` + `MultipartFormDataContent`를 직접 써야 합니다. (PowerShell 7+에는 `-Form`이 있습니다)

```powershell
Add-Type -AssemblyName System.Net.Http
$form = New-Object System.Net.Http.MultipartFormDataContent
$bc = New-Object System.Net.Http.ByteArrayContent(,$bytes)
$bc.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::Parse("image/png")
$form.Add($bc, "file", "photo.png")
```

> `ByteArrayContent(,$bytes)`의 **앞쪽 쉼표**는 PowerShell이 배열을 펼치지 않게 하는 문법입니다. 없으면 "생성자를 못 찾겠다"는 에러가 납니다.

---

## 7. 이번 study 정리

- 목록 응답은 **껍데기(`Page<T>`)로 감싸** `total`/`has_next`를 함께 준다.
- `per_page`에 **상한을 두는 것은 보안 조치**다. 없으면 메모리 고갈 공격이 가능하다.
- 오프셋 페이지네이션은 UI에 편하지만 **깊은 페이지에서 느리고, 데이터 변경에 취약**하다. 무한 스크롤엔 커서.
- 필터는 **목록 쿼리와 COUNT 쿼리에 똑같이** 적용해야 한다.
- 동적 조건은 **`QueryBuilder`** 로. `push`(SQL 조각)와 `push_bind`(값)를 구분하면 인젝션이 원천 차단된다.
- 업로드 파일명은 **UUID로 새로 만든다.** 클라이언트 이름은 검사하는 게 아니라 **버린다.**
- 확장자는 **검증된 MIME에서 유도**하고, MIME 자체는 위조 가능함을 전제한다.
- "새 파일 저장 → DB 갱신 → 옛 파일 삭제" 순서로 **최악의 경우가 고아 파일 하나**가 되게 한다.
- 정적 파일은 `ServeDir` + **`nest_service`** (Router가 아니라 Service이므로).

다음: [study9.md](study9.md) — 통합 테스트, 설정 관리, 로깅 정리. (고급 4)
