# 공유 상태와 게시판 CRUD (Study 2)

[study1.md](study1.md)에서는 상태가 없는 핸들러만 만들었습니다. 이제 **여러 요청이 같은 데이터를 함께 보고 고치는** 진짜 API를 만듭니다. DB는 아직 안 씁니다. 메모리에 벡터 하나를 두고, **그걸 여러 스레드가 동시에 만질 때 Rust가 무엇을 요구하는지**를 먼저 이해하는 게 이번 목표입니다.

**이 문서에서 만드는 것**: `/posts` CRUD 5종 (목록·단건·생성·수정·삭제) + 모듈 분리

---

## 1. 문제: "전역 변수에 넣으면 되는 거 아니야?"

다른 언어라면 이렇게 씁니다.

```python
# FastAPI
posts = []          # 그냥 모듈 전역 리스트

@app.post("/posts")
def create(body: CreatePost):
    posts.append(...)
```

```typescript
// NestJS
@Injectable()
export class PostsService {
  private posts: Post[] = [];   // 서비스 인스턴스 필드 (싱글톤)
}
```

Rust에서 같은 걸 시도하면 **컴파일이 안 됩니다.**

```rust
static mut POSTS: Vec<Post> = Vec::new();   // ❌ unsafe 없이는 접근 불가
```

왜냐하면 Rust는 이렇게 묻기 때문입니다.

> "이 벡터를 **A 스레드가 밀어 넣는 중에** B 스레드가 읽으면 어떻게 되죠?"

Python은 GIL이, Node는 싱글 스레드 이벤트 루프가 이 질문을 덮어줍니다. Java/Spring은 그냥 **개발자 책임**으로 넘깁니다. Rust는 **덮지도, 넘기지도 않고 타입으로 답을 요구**합니다.

---

## 2. 답: `Arc<RwLock<T>>`

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

Arc<RwLock<Vec<Post>>>
```

두 겹으로 감싸는데, 각 겹의 역할이 다릅니다.

| 감싸개 | 해결하는 문제 | 없으면? |
|---|---|---|
| **`Arc<T>`** | "이 데이터의 주인이 여럿이다" — 여러 스레드/핸들러가 **같은 것**을 가리키게 함 | 소유권이 하나뿐이라 복제하면 각자 다른 벡터를 갖게 됨 |
| **`RwLock<T>`** | "동시에 수정하면 안 된다" — 읽기는 여럿, 쓰기는 하나만 허용 | 데이터 레이스 (컴파일 에러) |

- **`Arc`** = **A**tomically **R**eference **C**ounted. 참조 카운트를 원자적으로 관리하는 스마트 포인터입니다. `.clone()`하면 데이터가 아니라 **카운트만 증가**하고, 마지막 `Arc`가 사라질 때 데이터가 해제됩니다.
  - `Rc`라는 단일 스레드용 형제가 있는데, 스레드 간에 쓰면 컴파일 에러(`Rc`는 `Send`가 아님)입니다. **서버에서는 항상 `Arc`.**
- **`RwLock`** = 읽기-쓰기 잠금. `Mutex`는 읽기든 쓰기든 무조건 한 명이지만, `RwLock`은 **읽기끼리는 동시에** 허용합니다. 게시판은 읽기가 압도적으로 많으니 `RwLock`이 맞습니다.

### ⚠️ `std::sync::RwLock`이 아니라 `tokio::sync::RwLock`

가장 중요한 함정입니다.

```rust
// ❌ 표준 라이브러리 락 — .await 너머로 들고 가면 안 된다
let guard = std_lock.write().unwrap();
some_async_fn().await;                    // 이 동안 워커 스레드가 통째로 묶인다

// ✅ tokio 락 — 락을 기다리는 동안에도 스레드를 양보한다
let guard = tokio_lock.write().await;
```

- `std`의 락은 **기다리는 동안 OS 스레드를 블로킹**합니다. async 런타임에서는 워커 스레드가 몇 개뿐이므로, 몇 개만 묶여도 서버 전체가 멈춥니다.
- `tokio::sync::RwLock`은 **락을 기다리는 동안 `.await`로 양보**합니다.
- 판단 기준: **락을 잡은 채로 `.await`를 하느냐.** 안 한다면(순수 메모리 조작만) `std` 락이 오히려 더 빠릅니다. 확신이 없으면 tokio 락을 쓰세요.

이 feature를 켜야 합니다.

```powershell
cargo add tokio --features sync
cargo add chrono --no-default-features --features clock,serde,std
```

> chrono는 `created_at` 타임스탬프용입니다. `--no-default-features`로 켠 이유는 기본 기능에 `oldtime` 등 필요 없는 것이 딸려 오기 때문입니다. `serde` feature가 있어야 `DateTime<Utc>`가 JSON으로 직렬화됩니다.

---

## 3. 모듈 분리

study1에서는 `main.rs` 하나였습니다. 이제 나눕니다.

```
src/
├─ main.rs           ← 진입점 + 라우터 조립
├─ state.rs          ← AppState
└─ posts/
   ├─ mod.rs         ← 모듈 선언 + 하위 라우터
   ├─ model.rs       ← 엔티티 + DTO
   └─ handler.rs     ← 핸들러(컨트롤러)
```

### Rust 모듈 규칙 — 자동이 아니다

```rust
// main.rs
mod posts;   // ← 이 줄이 있어야 posts/ 폴더가 컴파일에 포함된다
mod state;
```

- **파일을 만들어 두기만 하면 자동으로 인식되는 Node/Python과 다릅니다.** `mod X;`를 선언한 지점부터 모듈 트리에 편입됩니다. 선언을 빠뜨리면 그 파일은 **존재하지 않는 것과 같습니다**(에러도 안 나고 조용히 무시됩니다).
- `posts/mod.rs` 안에서 다시 `pub mod handler;`, `pub mod model;`을 선언해 하위 파일을 등록합니다.
- `pub`이 없으면 같은 모듈 안에서만 보입니다. 바깥에서 쓰려면 `pub`을 붙여야 합니다.
- 경로 참조:
  - `crate::state::AppState` — 크레이트 루트부터
  - `super::model::Post` — 부모 모듈부터
  - `self::handler` — 현재 모듈부터

---

## 4. `AppState` — `src/state.rs`

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::sync::RwLock;

use crate::posts::model::Post;

#[derive(Clone)]                       // ← 필수
pub struct AppState {
    posts: Arc<RwLock<Vec<Post>>>,
    next_id: Arc<AtomicI64>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            posts: Arc::new(RwLock::new(Vec::new())),
            next_id: Arc::new(AtomicI64::new(1)),
        }
    }

    pub async fn posts(&self) -> tokio::sync::RwLockReadGuard<'_, Vec<Post>> {
        self.posts.read().await
    }

    pub async fn posts_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, Vec<Post>> {
        self.posts.write().await
    }

    pub fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}
```

### `#[derive(Clone)]`이 필수인 이유

axum은 **요청이 올 때마다 상태를 복제**해서 핸들러에 넘깁니다. 그래서 `State<T>`의 `T`는 `Clone`이어야 합니다.

"매 요청마다 게시글 전체를 복사한다고?" — **아닙니다.** 필드가 전부 `Arc`라서 복제되는 건 **참조 카운트 뿐**입니다. 실제 `Vec<Post>`는 힙에 하나만 존재합니다. 이게 `Arc`로 감싸는 두 번째 이유입니다.

### `AtomicI64`로 id 발급

```rust
pub fn next_id(&self) -> i64 {
    self.next_id.fetch_add(1, Ordering::Relaxed)
}
```

- `fetch_add(1, ...)`는 **"1을 더하고 더하기 전 값을 반환"** 을 CPU 명령 하나로 원자적으로 처리합니다. 락 없이도 두 요청이 같은 id를 받는 일이 없습니다.
- `Ordering::Relaxed`는 메모리 순서 보장 수준입니다. "카운터를 겹치지 않게 증가"만 필요하고 다른 데이터와의 순서 관계는 상관없으므로 가장 느슨한 걸로 충분합니다. (이 부분은 지금 깊게 이해하지 않아도 됩니다)
- `AtomicI64`는 그 자체로는 `Clone`이 아니라서 `Arc`로 감쌌습니다.

### 락 가드를 반환하는 메서드

```rust
pub async fn posts(&self) -> tokio::sync::RwLockReadGuard<'_, Vec<Post>> {
    self.posts.read().await
}
```

- `.read().await`가 반환하는 **`RwLockReadGuard`** 는 "락을 쥐고 있다는 증서"이자 데이터에 접근하는 손잡이입니다.
- **가드가 스코프를 벗어나면 락이 자동 해제**됩니다. `unlock()`을 호출할 필요도, 잊어버릴 위험도 없습니다. (RAII 패턴)
- `'_`는 수명(lifetime) 표기로, "이 가드는 `self`가 살아 있는 동안만 유효하다"는 뜻입니다. 컴파일러가 **락이 데이터보다 오래 사는 일**을 막아줍니다.

---

## 5. 모델 — `src/posts/model.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 게시글 엔티티. 응답으로 나가므로 Serialize.
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 생성 요청 본문. 들어오기만 하므로 Deserialize.
#[derive(Debug, Deserialize)]
pub struct CreatePost {
    pub title: String,
    pub content: String,
    pub author: String,
}

/// 수정 요청 본문. 모든 필드가 Option이라 부분 수정이 가능하다.
#[derive(Debug, Deserialize)]
pub struct UpdatePost {
    pub title: Option<String>,
    pub content: Option<String>,
}
```

### 엔티티와 DTO를 나누는 이유

`CreatePost`에 `id`가 없다는 점이 핵심입니다. 엔티티를 그대로 요청 타입으로 쓰면 **클라이언트가 `id`나 `created_at`을 마음대로 보낼 수 있습니다.** 타입을 나누면 그런 요청은 애초에 파싱 단계에서 무시되거나 거부됩니다.

| | 이 프로젝트 | NestJS | FastAPI | Spring |
|---|---|---|---|---|
| 엔티티 | `Post` | Entity | SQLAlchemy 모델 | `@Entity` |
| 생성 DTO | `CreatePost` | `CreatePostDto` | `PostCreate` | `PostCreateRequest` |
| 수정 DTO | `UpdatePost` | `UpdatePostDto` (`PartialType`) | `PostUpdate` | `PostUpdateRequest` |

Nest는 `PartialType(CreatePostDto)`로 "전부 옵셔널인 버전"을 자동 생성해 주지만, Rust에는 그런 마법이 없어서 `Option<T>`를 손으로 적습니다.

### `#[derive(...)]`는 무슨 일을 하나

| derive | 의미 |
|---|---|
| `Debug` | `{:?}`로 출력 가능. 디버깅/로깅용 |
| `Clone` | `.clone()`으로 깊은 복사 가능 |
| `Serialize` | 구조체 → JSON (응답용) |
| `Deserialize` | JSON → 구조체 (요청용) |

derive는 **컴파일 타임에 구현 코드를 생성**하는 매크로입니다. 리플렉션이 아니라 실제 코드가 만들어지므로 런타임 비용이 없습니다.

---

## 6. 핸들러 — `src/posts/handler.rs`

### 목록 조회

```rust
pub async fn list(State(state): State<AppState>) -> Json<Vec<Post>> {
    let posts = state.posts().await;       // 읽기 락
    let mut items = posts.clone();         // 락 안에서 복제
    items.sort_by(|a, b| b.id.cmp(&a.id)); // 최신순
    Json(items)
}                                          // ← 여기서 락 자동 해제
```

- `posts.clone()`이 필요한 이유: 락 가드는 **빌린 참조**라서 함수 밖으로 내보낼 수 없습니다. 응답으로 내보내려면 소유권이 있는 값이 필요하므로 복제합니다.
- (실제 서비스라면 매번 전체 복제는 낭비입니다. study5에서 DB로 옮기면 이 문제가 자연스럽게 사라집니다.)

### 단건 조회 — `Option` → `Result` 변환

```rust
pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Post>, (StatusCode, Json<serde_json::Value>)> {
    let posts = state.posts().await;

    posts
        .iter()
        .find(|p| p.id == id)   // Option<&Post>
        .cloned()               // Option<Post>
        .map(Json)              // Option<Json<Post>>
        .ok_or_else(|| not_found(id))   // Result<Json<Post>, (StatusCode, Json<Value>)>
}
```

- `.ok_or_else(|| ...)`는 **`Option` → `Result` 변환**의 관용구입니다. `Some(v)` → `Ok(v)`, `None` → `Err(에러)`.
  - `ok_or`(값)와 `ok_or_else`(클로저) 둘 다 있는데, 에러 값을 만드는 데 비용이 든다면 `_else` 쪽이 낫습니다. 실패했을 때만 실행되기 때문입니다.
- 반환 타입 `Result<Json<Post>, (StatusCode, Json<Value>)>` — **성공 타입과 실패 타입이 둘 다 시그니처에 적혀 있습니다.** "이 함수는 404를 낼 수 있다"가 타입에 드러납니다. 대신 **보기 흉합니다.** 핸들러마다 이 긴 튜플을 적어야 합니다. → study4에서 `AppError` 하나로 정리합니다.

### 생성 — 201 + Location 헤더

```rust
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreatePost>,      // ← 본문 extractor는 반드시 마지막
) -> impl IntoResponse {
    let now = Utc::now();
    let post = Post {
        id: state.next_id(),
        title: body.title,             // 소유권 이동 (복제 없음)
        content: body.content,
        author: body.author,
        created_at: now,
        updated_at: now,
    };

    let location = format!("/posts/{}", post.id);
    state.posts_mut().await.push(post.clone());

    (
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(post),
    )
}
```

- **`(StatusCode, 헤더배열, Json<T>)` 3-튜플도 `IntoResponse`** 입니다. 헤더는 `[(name, value)]` 배열로 넘깁니다.
- `body.title`을 `.clone()` 없이 넘긴 것에 주목하세요. `body`는 이 함수가 소유하고 있고 뒤에서 더 안 쓰므로 **그냥 이동**시키면 됩니다. 불필요한 복제를 안 하는 게 Rust의 기본 태도입니다.
- `post.clone()`은 필요합니다. 벡터에 넣고 나서 응답으로도 내보내야 하기 때문입니다.

### 수정 — 부분 수정

```rust
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdatePost>,
) -> Result<Json<Post>, (StatusCode, Json<serde_json::Value>)> {
    let mut posts = state.posts_mut().await;    // 쓰기 락

    let post = posts.iter_mut().find(|p| p.id == id).ok_or_else(|| not_found(id))?;

    if let Some(title) = body.title {           // Some일 때만 덮어쓴다
        post.title = title;
    }
    if let Some(content) = body.content {
        post.content = content;
    }
    post.updated_at = Utc::now();

    Ok(Json(post.clone()))
}
```

- `iter_mut()`은 **가변 참조**를 주는 이터레이터입니다. 그냥 `iter()`면 읽기만 되고 `post.title = ...`에서 컴파일 에러가 납니다.
- `let mut posts = ...` — 가드 자체도 `mut`이어야 안쪽 데이터를 고칠 수 있습니다.
- 끝의 `?`가 `Option`이 아니라 `Result`에 붙어 있습니다. `ok_or_else`로 이미 `Result`로 바꿨기 때문에 가능합니다.

### 삭제 — 204

```rust
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let mut posts = state.posts_mut().await;

    let before = posts.len();
    posts.retain(|p| p.id != id);       // 조건을 만족하는 것만 남긴다

    if posts.len() == before {
        Err(not_found(id))              // 아무것도 안 지워졌다 = 없는 id
    } else {
        Ok(StatusCode::NO_CONTENT)      // 204, 본문 없음
    }
}
```

- `StatusCode` 자체도 `IntoResponse`입니다. 본문 없는 응답은 이렇게 상태 코드만 반환하면 됩니다.

---

## 7. 라우터 조립

### 하위 라우터 — `src/posts/mod.rs`

```rust
pub mod handler;
pub mod model;

use axum::{Router, routing::get};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        // nest("/posts", ...) 아래에 붙으므로 여기서 "/"가 곧 "/posts"다.
        .route("/", get(handler::list).post(handler::create))
        .route(
            "/{id}",
            get(handler::get_one)
                .patch(handler::update)
                .delete(handler::delete),
        )
}
```

### ⭐ `Router<AppState>` vs `Router<()>` — 이해해야 할 핵심

axum의 `Router`는 **"어떤 상태를 필요로 하는가"를 타입 파라미터로** 들고 다닙니다.

| 타입 | 의미 |
|---|---|
| `Router<AppState>` | **아직 `AppState`가 주입되지 않은** 라우터. 서빙 불가 |
| `Router<()>` (= `Router`) | 필요한 상태가 전부 채워진 라우터. `axum::serve`에 넘길 수 있음 |

`.with_state(state)`가 `Router<AppState>` → `Router<()>` 로 바꿔주는 함수입니다.

```rust
fn app(state: AppState) -> Router {          // Router = Router<()>
    Router::new()
        .route("/", get(root))
        .nest("/posts", posts::router())     // Router<AppState>를 장착
        .with_state(state)                   // ← 여기서 <AppState>가 채워진다
}
```

이 구조 덕분에 **"상태를 주입하지 않고 서버를 띄우는 실수"가 컴파일 타임에 막힙니다.** 대신 처음에는 타입 에러가 낯설게 느껴집니다.

| 증상 | 원인 |
|---|---|
| `Router<AppState>`를 `axum::serve`에 넘겼더니 에러 | `.with_state()` 누락 |
| 하위 라우터 함수 반환 타입을 `Router`로 적었더니 에러 | `Router<AppState>`로 고쳐야 함 |
| `.with_state()`를 하위 라우터에서 먼저 호출 | 가능은 하지만, 보통 최상위에서 한 번만 하는 게 깔끔 |

### `nest` vs `merge`

| 메서드 | 동작 |
|---|---|
| `.nest("/posts", r)` | `r`의 모든 경로 앞에 `/posts`를 붙여 장착 |
| `.merge(r)` | 접두사 없이 같은 레벨에 합침 |
| `.nest_service("/static", svc)` | 라우터가 아닌 임의의 tower `Service`를 장착 (study8의 정적 파일) |

---

## 8. 실행 및 검증

```powershell
cargo run
```

실제 요청/응답 (순서대로 실행):

| # | 요청 | 응답 |
|---|---|---|
| 1 | `POST /posts` `{"title":"First post","content":"hello axum","author":"while"}` | `201` `{"id":1,...,"created_at":"2026-09-14T06:11:50.856609300Z",...}` |
| 2 | `POST /posts` `{"title":"Second post",...}` | `201` `{"id":2,...}` |
| 3 | `GET /posts` | `200` `[{"id":2,...},{"id":1,...}]` ← 최신순 |
| 4 | `GET /posts/1` | `200` `{"id":1,"title":"First post",...}` |
| 5 | `PATCH /posts/1` `{"title":"First post (edited)"}` | `200` `{"id":1,"title":"First post (edited)","content":"hello axum",...}` |
| 6 | `DELETE /posts/2` | `204` (본문 없음) |
| 7 | `GET /posts` | `200` `[{"id":1,"title":"First post (edited)",...}]` |
| 8 | `GET /posts/999` | `404` `{"error":"post 999 not found"}` |
| 9 | `POST /posts` `{"title":"no author"}` | `422` `Failed to deserialize the JSON body into the target type: missing field 'content' at line 1 column 21` |
| 10 | `GET /posts/abc` | `400` `Invalid URL: Cannot parse 'abc' to a 'i64'` |

**5번**에서 `content`는 안 보냈는데 그대로 남아 있고 `updated_at`만 바뀐 것 — 부분 수정이 의도대로 동작합니다.

### 🔍 9번과 10번의 상태 코드가 다른 이유

같은 "입력이 잘못됐다"인데 하나는 **422**, 하나는 **400**입니다. 처음 보면 버그처럼 느껴지는데 axum의 의도된 동작입니다.

| 상황 | 코드 | axum의 판단 |
|---|---|---|
| JSON 문법은 맞는데 필드가 빠짐 | **422** Unprocessable Entity | "문법은 이해했는데 의미적으로 처리 불가" |
| JSON 문법 자체가 깨짐 | 400 Bad Request | "아예 못 읽겠다" |
| `Content-Type`이 `application/json`이 아님 | 415 Unsupported Media Type | |
| 경로 파라미터 타입 불일치 | 400 Bad Request | |

FastAPI도 검증 실패에 422를 씁니다(Pydantic). 반면 Spring/Nest는 보통 400입니다. **팀 컨벤션에 맞추려면 직접 바꿔야 하고, 그게 study4에서 할 일**입니다.

### 🔍 오류 응답 형식이 제각각이다

지금 상태를 보면:

```
404 → {"error":"post 999 not found"}            내가 만든 JSON
422 → Failed to deserialize the JSON body...    axum의 평문
400 → Invalid URL: Cannot parse `abc` to...     axum의 평문
```

클라이언트 입장에서는 **오류를 파싱할 방법이 없습니다.** 어떨 땐 JSON이고 어떨 땐 텍스트니까요. 이게 "오류 타입으로 응답 통일"이 필요한 이유이고, study3(검증)과 study4(통일)에서 순서대로 해결합니다.

---

## 9. 처음 만난 오류

### 9-1. `AppState: Clone is not satisfied`

```rust
pub struct AppState { ... }      // derive(Clone)을 빼먹음
```

`State<AppState>`를 쓰는 핸들러가 전부 "Handler trait을 만족하지 않는다"는 긴 에러를 냅니다. 실제 원인은 한 줄, `#[derive(Clone)]` 누락입니다.

> 교훈: **핸들러 에러는 원인이 시그니처가 아니라 상태 타입에 있을 수 있습니다.** `State<T>`를 쓰는 핸들러가 한꺼번에 빨개지면 `T`부터 의심하세요.

### 9-2. `cannot borrow as mutable`

```rust
let posts = state.posts_mut().await;   // let mut 이 아님
posts.push(post);                      // ❌ cannot borrow `posts` as mutable
```

가드를 `let mut`으로 받아야 안쪽 데이터를 수정할 수 있습니다. 락을 쓰기 모드로 잡았느냐와, 가드 변수가 `mut`이냐는 **별개**입니다.

### 9-3. 모듈을 만들었는데 "cannot find module"

`src/posts/handler.rs`를 만들었는데 `posts::handler`를 못 찾는다면, `src/posts/mod.rs`에 `pub mod handler;`가 없는 것입니다. **파일 생성 ≠ 모듈 등록.**

---

## 10. 이번 study 정리

- 공유 가변 상태 = **`Arc<RwLock<T>>`**. `Arc`는 "주인이 여럿", `RwLock`은 "동시 수정 금지"를 담당한다.
- async 코드에서는 `std::sync` 락이 아니라 **`tokio::sync` 락**을 쓴다 (락을 잡은 채 `.await`할 때).
- `AppState`는 `#[derive(Clone)]` 필수. 필드를 `Arc`로 감싸서 복제 비용을 없앤다.
- Rust 모듈은 **`mod X;` 선언이 있어야** 컴파일에 포함된다.
- 하위 라우터는 `Router<AppState>`를 반환하고, 최상위에서 `.with_state()`로 채운다.
- 엔티티(`Post`)와 DTO(`CreatePost`/`UpdatePost`)를 분리한다.
- 지금 오류 응답은 형식이 제각각이다 — **다음 두 문서의 숙제.**

다음: [study3.md](study3.md) — serde 세부 제어와 `validator`로 입력 검증하기.
