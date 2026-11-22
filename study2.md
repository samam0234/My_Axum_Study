# study2 — 핵심 구조

> 🎯 **목표**: Axum의 기본 단위(핸들러 → Router → 모듈)가 무엇인지 직접 만들어 보고, 모듈을 나누고 서로 부르는 방법과 `State`로 데이터를 공유하는 방법을 익힌다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s02_modules.rs](lab/examples/s02_modules.rs) (포트 3002), 실제 프로젝트 구조는 [demo/src/](demo/src/) |

---

## ① 목표

- "컴포넌트/컨트롤러" 같은 클래스가 없는 구조에서 코드를 **어떻게 나누는지** 설명할 수 있다.
- `Router`를 돌려주는 함수로 모듈을 만들고 `nest`로 합친다.
- 모듈 간 호출이 **그냥 함수 호출**임을 확인한다.
- `State<AppState>`로 모두가 공유하는 값을 넘기고, `Router<AppState>`와 `Router<()>`의 차이를 설명한다.

---

## ② 📖 이론

### 기본 단위 세 겹

```
핸들러 (async fn)          ← 요청 하나를 처리하는 가장 작은 단위
   ↓ 묶는다
Router (경로 표)            ← 핸들러들을 경로로 묶은 "값"
   ↓ 돌려주는 함수
모듈 (mod users { pub fn router() -> Router<AppState> })   ← 기능 단위
```

- **핸들러**: 상태를 갖지 않는 평범한 함수. 필요한 것은 전부 인자로 받는다.
- **Router**: `Router::new().route(...)`가 돌려주는 **값**이다. 값이므로 함수가 반환할 수 있고, 다른 라우터에 끼워 넣을 수 있다(`nest`, `merge`).
- **모듈**: 파일/`mod`로 나눈 기능 묶음. 보통 "라우터를 만들어 주는 함수 + 핸들러 + (모델, 저장소)"를 한 폴더에 둔다.

### 상태(State)는 어떻게 들어가나

핸들러는 전역 변수를 쓰지 않는다. 대신 **앱 상태를 인자로 받는다.**

```rust
#[derive(Clone)]            // ← 필수: 요청마다 복제되어 핸들러에 들어간다
struct AppState { app_name: Arc<String> }

async fn get_user(State(state): State<AppState>, ...) { ... }
```

- `Clone`이 필수라서, 큰 값은 `Arc<T>`로 감싸 **복제 비용을 포인터 복사 수준**으로 만든다.
- DB 풀(`sqlx::Pool`)처럼 이미 내부가 `Arc`인 타입은 그대로 넣어도 된다.

### `Router<S>`의 `S`가 뜻하는 것

| 타입 | 의미 |
|---|---|
| `Router<AppState>` | "**아직 상태를 못 받은** 라우터". 핸들러가 `State<AppState>`를 요구한다 |
| `Router<()>` | 상태가 이미 주입되어 **바로 서빙할 수 있는** 라우터 |
| `.with_state(state)` | `Router<AppState>` → `Router<()>` 로 바꾸는 한 줄 |

`.with_state()`를 빠뜨리면 컴파일 에러(`Handler ... is not satisfied`)가 난다. 하위 모듈은 `Router<AppState>`를 반환하고, **가장 바깥에서 한 번만** 주입한다.

### 비유 — 건물

핸들러는 **방**, Router는 **층 안내도**, 모듈은 **층**이다. `nest("/users", users::router())`는 "2층 안내도를 1층 로비 안내판에 `/users`라는 이름으로 붙인다"는 뜻이다.

---

## ③ 🛠️ 실기

### 3-1. 전체 코드 — [lab/examples/s02_modules.rs](lab/examples/s02_modules.rs)

```rust
use std::sync::Arc;
use axum::{Json, Router, extract::{Path, State}, http::StatusCode, routing::get};
use serde::Serialize;

#[derive(Clone)]
struct AppState {
    app_name: Arc<String>,
}

#[tokio::main]
async fn main() {
    let state = AppState { app_name: Arc::new("lab-s02".to_string()) };

    let app = Router::new()
        .route("/", get(|| async { "모듈 구조 예제: /users/1, /posts/1" }))
        .nest("/users", users::router())
        .nest("/posts", posts::router())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3002").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

mod users {
    use super::*;

    #[derive(Serialize)]
    pub struct User { pub id: u32, pub name: &'static str }

    pub fn find(id: u32) -> Option<User> {
        match id {
            1 => Some(User { id: 1, name: "mina" }),
            2 => Some(User { id: 2, name: "joon" }),
            _ => None,
        }
    }

    pub fn router() -> Router<AppState> {
        Router::new().route("/{id}", get(get_user))
    }

    async fn get_user(State(state): State<AppState>, Path(id): Path<u32>)
        -> Result<Json<User>, StatusCode>
    {
        println!("[{}] GET /users/{id}", state.app_name);
        find(id).map(Json).ok_or(StatusCode::NOT_FOUND)
    }
}

mod posts {
    use super::*;

    #[derive(Serialize)]
    struct PostView { id: u32, title: &'static str, author: Option<&'static str> }

    pub fn router() -> Router<AppState> {
        Router::new().route("/{id}", get(get_post))
    }

    async fn get_post(Path(id): Path<u32>) -> Result<Json<PostView>, StatusCode> {
        let (title, author_id) = match id {
            1 => ("첫 글", 1),
            2 => ("두 번째 글", 2),
            _ => return Err(StatusCode::NOT_FOUND),
        };
        Ok(Json(PostView { id, title, author: users::find(author_id).map(|u| u.name) }))
    }
}
```

### 3-2. 줄별 설명

| 코드 | 설명 |
|---|---|
| `#[derive(Clone)] struct AppState` | 앱 전체 공유 상태. 모든 필드가 `Clone`이어야 derive가 된다. `Arc<String>`은 포인터만 복사한다 |
| `.nest("/users", users::router())` | `users` 라우터의 `/{id}`가 최종 경로 **`/users/{id}`** 가 된다. 접두어를 한 곳에서 관리 |
| `.with_state(state)` | 가장 **마지막에 한 번**. 하위 라우터가 `Router<AppState>`였으므로 여기서 `Router<()>`가 된다 |
| `mod users { use super::*; ... }` | 인라인 모듈. `use super::*`로 바깥의 `Router`, `AppState` 등을 가져온다. 실제 프로젝트에서는 `users/mod.rs` 파일로 분리 |
| `pub fn find(id) -> Option<User>` | **다른 모듈이 부를 수 있는 공개 함수**. 모듈의 "공개 인터페이스" |
| `fn get_user`에 `pub`이 없음 | 핸들러는 모듈 안에서만 쓰이므로 **비공개**. 바깥에는 `router()`만 노출 |
| `find(id).map(Json).ok_or(StatusCode::NOT_FOUND)` | `Option<User>` → `Option<Json<User>>` → `Result<Json<User>, StatusCode>`. 없으면 404. 한 줄 체인 |
| `users::find(author_id).map(\|u\| u.name)` | **posts가 users를 호출**한다. HTTP도 주입 설정도 없다. 함수 인자와 반환값이 곧 "props" |
| `return Err(StatusCode::NOT_FOUND)` | 핸들러 중간에서 일찍 반환 |

### 3-3. 실행 결과

```
$ curl http://127.0.0.1:3002/users/1
{"id":1,"name":"mina"}

$ curl http://127.0.0.1:3002/posts/1
{"id":1,"title":"첫 글","author":"mina"}        ← posts가 users::find를 호출해 합친 결과

$ curl -o /dev/null -w "%{http_code}" http://127.0.0.1:3002/users/9
404
```

### 3-4. 실제 프로젝트의 구조 — [demo/src/](demo/src/)

한 파일에 `mod`로 나눈 것을 **폴더로 펼치면** 이 저장소의 `demo/`가 된다.

```
demo/src/
├─ main.rs          ← 설정 읽기 → 자원 준비 → 서버 실행 (조립만)
├─ lib.rs           ← app(): 모든 라우터와 미들웨어를 조립
├─ state.rs         ← AppState
├─ routes.rs        ← 데모용 라우트
├─ posts/
│  ├─ mod.rs        ← router() — 이 기능의 경로 표
│  ├─ handler.rs    ← 핸들러들
│  ├─ model.rs      ← 요청/응답/DB 타입
│  └─ repo.rs       ← DB 접근
└─ auth/            ← 같은 모양 (mod / handler / model / repo + extractor, jwt, password)
```

[demo/src/posts/mod.rs](demo/src/posts/mod.rs)의 핵심:

```rust
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handler::list).post(handler::create))
        .route("/{id}", get(handler::get_one).patch(handler::update).delete(handler::delete))
}
```

`get(a).post(b)`처럼 **같은 경로에 메서드별 핸들러를 체인**한다. 그리고 [demo/src/lib.rs](demo/src/lib.rs)에서 `.nest("/posts", posts::router())`로 장착한다.

---

## ④ 🔗 연동 포인트 — 단위끼리 데이터를 주고받는 방법

| 방법 | 언제 | 예 |
|---|---|---|
| **함수 호출** (인자·반환값) | 모듈 → 모듈 | `users::find(author_id)` |
| **`State<AppState>`** | 모든 핸들러가 공유하는 값 (DB 풀, 설정, 캐시) | `State(state): State<AppState>` |
| **Extractor** | 요청에서 온 값 (경로·쿼리·본문·인증 사용자) | `Path<u32>`, `Json<T>`, `AuthUser` |
| **`Extension<T>` / 요청 확장** | 미들웨어가 **요청 중간에** 만든 값을 핸들러로 전달 | 요청 ID, 인증 결과 (study7) |
| **채널** (`mpsc`, `broadcast`) | 서로 다른 태스크 사이 메시지 | 백그라운드 작업 큐, 알림 (study15, 17) |

**계층을 나누는 기준** — 이 프로젝트에서는 한 기능을 이렇게 나눈다.

```
handler  ← HTTP를 아는 층 (extractor, 상태 코드)
   ↓ 호출
repo     ← DB를 아는 층 (SQL)
```

핸들러는 SQL을 모르고, repo는 `StatusCode`를 모른다. 이 경계가 study6~9에서 오류 처리와 테스트를 쉽게 만든다. 이 경계를 **빌드 시스템으로 강제**하는 방법(크레이트 분리)은 [studyFinal](studyFinal.md)에서 다룬다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| `the trait bound ... Handler<_, _> is not satisfied` | `State<AppState>`를 쓰는 핸들러를 등록했는데 **`.with_state()`를 안 붙임** | 가장 바깥 라우터에 `.with_state(state)` |
| 같은 에러 | `AppState`에 `#[derive(Clone)]`이 빠짐 | derive 추가. 필드 중 `Clone`이 아닌 것이 있으면 `Arc`로 감싼다 |
| `expected Router<()>, found Router<AppState>` | 하위 라우터를 `axum::serve`에 직접 넘김 | 상태를 주입한 뒤(`.with_state`)에 넘긴다 |
| `function ... is private` (E0603) | 다른 모듈의 함수에 `pub`이 없음 | 공개하려는 것만 `pub`. **핸들러는 비공개, `router()`만 공개**가 기본 |
| `cannot find ... in this scope` (모듈 안에서) | 인라인 `mod` 안에서는 바깥의 `use`가 보이지 않음 | `use super::*;` |
| `/users` 로 접속했는데 404 | `nest("/users", ...)` 안의 경로가 `/{id}`뿐이라 `/users` 자체는 없음 | 목록 경로(`"/"`)를 추가. **`nest` 안의 `"/"`가 `/users`가 된다** |
| `expected Router, found Router<AppState>` 로 `.merge`/`.nest`가 실패 | 서로 다른 상태 타입의 라우터를 섞음 | 모든 하위 라우터의 `S`를 같은 `AppState`로 통일 |

---

## ⑥ 직접 해보기

1. **`/posts/{id}/author` 추가**: posts 모듈에서 `users::find`를 호출해 작성자 객체 전체(`User`)를 돌려주는 라우트를 만든다. `User`에 `#[derive(Serialize)]`가 있는지 확인한다.
2. **상태에 카운터 넣기**: `AppState`에 `Arc<AtomicU64>`를 넣고, 모든 요청에서 `fetch_add(1, Relaxed)`로 세어 `/stats`로 보여 준다. 동시에 100번 요청해도 숫자가 맞는지 `curl` 반복문으로 확인한다.
3. **모듈을 파일로 분리**: `mod users`를 `users.rs`로 옮기고 `main.rs`에는 `mod users;`만 남긴다. `pub`이 필요한 곳을 컴파일러가 알려 주는 대로 고친다.

---

## ⑦ ☐ 체크리스트

- ☐ 핸들러 / Router / 모듈의 관계를 그림 없이 설명할 수 있다
- ☐ `nest`로 접두어를 붙이고 `merge`와 다르다는 것을 안다
- ☐ 모듈 간 호출이 일반 함수 호출임을 코드로 확인했다 (`users::find`)
- ☐ `AppState`에 `Clone`이 필요한 이유와 `Arc`를 쓰는 이유를 설명할 수 있다
- ☐ `Router<AppState>`와 `Router<()>`의 차이, 그리고 `.with_state()`의 위치를 설명할 수 있다
- ☐ 핸들러는 비공개, `router()`만 공개하는 이유를 안다
- ☐ `handler → repo` 계층 분리가 왜 필요한지 한 문장으로 말할 수 있다

---

## ⑧ 다음 단계 연결

지금의 사용자·글은 코드에 박힌 값이었다. [study3](study3.md)에서는 **바뀌는 데이터**를 `Arc<RwLock<…>>` 상태에 담고, 화면과 묶고(바인딩), 파일로 저장해 서버를 껐다 켜도 남게 만든다.
