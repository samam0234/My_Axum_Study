# study3 — 데이터 다루기

> 🎯 **목표**: 서버가 데이터를 **메모리 상태**로 들고 있게 만들고(`Arc<RwLock<…>>`), 그것을 화면과 묶고(바인딩), 파일에 저장해 서버를 껐다 켜도 남게 한다. DB를 쓰기 전 단계에서 "데이터가 어디에 살고 누가 접근하는가"를 몸으로 익힌다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s03_store.rs](lab/examples/s03_store.rs) (`cd lab` → `cargo run --example s03_store`, 포트 3003) |
| 검증 | 추가 → 토글 → **서버 재시작** → 데이터 유지까지 실제로 확인 |

---

## ① 목표

- 모델(`struct`)과 JSON 이름 규칙(`camelCase`)을 정한다.
- 공유 상태에서 **읽기 락과 쓰기 락**을 구분해 쓴다.
- 데이터를 JSON 파일로 저장하고 시작할 때 복구한다 (쓰는 도중 죽어도 안전하게).
- 서버 데이터를 DOM에 묶는 바인딩, 브라우저 `localStorage`의 용도를 이해한다.

---

## ② 📖 이론

### 데이터는 어디에 사는가

| 위치 | 수명 | 누가 접근 | 이 저장소의 단계 |
|---|---|---|---|
| 핸들러의 지역 변수 | 요청 하나 | 그 요청만 | study1~2 |
| **서버 메모리(State)** | 서버 프로세스 | 모든 요청 | **study3 (여기)** |
| **서버 파일** | 디스크 | 서버 | study3 (여기) |
| DB | 서버와 독립 | 여러 서버·도구 | study6 |
| 브라우저 `localStorage` | 브라우저 | 그 브라우저의 JS만 | study3 (여기) |

### 동시성: 왜 락이 필요한가

요청은 **동시에** 여러 개 처리된다. 두 요청이 같은 `Vec`을 동시에 수정하면 데이터가 깨진다. Rust는 이것을 **컴파일 단계에서** 막는다: 여러 스레드가 공유하는 값을 그냥 `&mut`로 바꿀 수 없다.

```rust
Arc<RwLock<Vec<Todo>>>
//  │   │       └ 실제 데이터
//  │   └ 락: 읽기는 여러 명이 동시에, 쓰기는 한 명만
//  └ 참조 카운트: 여러 핸들러가 같은 값을 "공유 소유"
```

| 도구 | 의미 |
|---|---|
| `Arc<T>` | 여러 곳이 같은 `T`를 소유. `clone()`은 포인터 복사 |
| `RwLock<T>` | `read()`는 동시에 여러 개, `write()`는 하나만(그동안 다른 읽기·쓰기가 대기) |
| `Mutex<T>` | 읽기/쓰기 구분 없이 한 번에 하나. 더 단순하고, 읽기가 압도적으로 많지 않으면 충분 |

**tokio의 `RwLock` vs 표준 `std::sync::RwLock`** — 락을 잡은 채로 `.await`(파일 쓰기 등)를 해야 하면 **tokio의 락**을 쓴다. 표준 락의 guard를 `.await` 너머로 들고 있으면 컴파일 에러가 난다(아래 오류 노트).

### 모델과 JSON 이름

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Todo { id: u32, title: String, done: bool }
```

Rust 필드는 관례대로 `snake_case`, JSON은 프론트 관례에 맞춰 `camelCase`. `#[serde(rename_all = "camelCase")]`가 **직렬화 경계에서만** 이름을 바꾼다. (공통 API 스펙 v1이 camelCase를 요구한다 → study6)

### 바인딩이란

"**데이터가 바뀌면 화면이 따라 바뀐다**"는 연결. 이 단계에서는 가장 원시적인 형태로 직접 한다.

```
서버 JSON ──fetch──▶ JS 배열 ──render()──▶ DOM
   ▲                                      │ 클릭/제출
   └──────────── fetch(POST/PATCH/DELETE) ◀┘   → 성공하면 다시 render()
```

프레임워크(React, Vue 등)는 이 `render()` 호출을 자동화해 줄 뿐, 본질은 같다(study12).

---

## ③ 🛠️ 실기

### 3-1. 전체 코드 — [lab/examples/s03_store.rs](lab/examples/s03_store.rs) (서버 부분)

```rust
use std::{path::PathBuf, sync::Arc};
use axum::{Json, Router, extract::{Path, State}, http::StatusCode, response::Html,
           routing::{get, patch}};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Todo { id: u32, title: String, done: bool }

#[derive(Debug, Deserialize)]
struct NewTodo { title: String }

#[derive(Clone)]
struct AppState {
    todos: Arc<RwLock<Vec<Todo>>>,
    file: Arc<PathBuf>,
}

impl AppState {
    async fn load(file: PathBuf) -> Self {
        let todos = match tokio::fs::read_to_string(&file).await {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Self { todos: Arc::new(RwLock::new(todos)), file: Arc::new(file) }
    }

    async fn save(&self, todos: &[Todo]) -> std::io::Result<()> {
        let tmp = self.file.with_extension("json.tmp");
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(todos)?).await?;
        tokio::fs::rename(&tmp, &*self.file).await
    }
}

#[tokio::main]
async fn main() {
    let file = std::env::var("TODO_FILE").unwrap_or_else(|_| "todos.json".into());
    let state = AppState::load(PathBuf::from(file)).await;

    let app = Router::new()
        .route("/", get(page))
        .route("/api/todos", get(list).post(create))
        .route("/api/todos/{id}", patch(toggle).delete(remove))
        .with_state(state);
    // ... bind & serve
}

async fn list(State(state): State<AppState>) -> Json<Vec<Todo>> {
    Json(state.todos.read().await.clone())
}

async fn create(State(state): State<AppState>, Json(body): Json<NewTodo>)
    -> Result<(StatusCode, Json<Todo>), StatusCode>
{
    let title = body.title.trim().to_string();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut todos = state.todos.write().await;
    let id = todos.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let todo = Todo { id, title, done: false };
    todos.push(todo.clone());

    state.save(&todos).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(todo)))
}

async fn toggle(State(state): State<AppState>, Path(id): Path<u32>)
    -> Result<Json<Todo>, StatusCode>
{
    let mut todos = state.todos.write().await;
    let todo = todos.iter_mut().find(|t| t.id == id).ok_or(StatusCode::NOT_FOUND)?;
    todo.done = !todo.done;
    let updated = todo.clone();
    state.save(&todos).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(updated))
}

async fn remove(State(state): State<AppState>, Path(id): Path<u32>) -> StatusCode {
    let mut todos = state.todos.write().await;
    let before = todos.len();
    todos.retain(|t| t.id != id);
    if todos.len() == before { return StatusCode::NOT_FOUND; }
    match state.save(&todos).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
```

### 3-2. 줄별 설명

| 코드 | 설명 |
|---|---|
| `todos: Arc<RwLock<Vec<Todo>>>` | 앞의 이론 그대로. `Clone`은 `Arc`만 복사하므로 싸다 |
| `file: Arc<PathBuf>` | 저장 경로도 상태에 함께 둔다 (환경변수 `TODO_FILE`로 지정) |
| `AppState::load` | 시작 시 파일을 읽어 복구. **파일이 없으면 빈 목록**으로 시작 |
| `serde_json::from_str(&text).unwrap_or_default()` | JSON이 깨졌으면 빈 목록. ⚠️ **조용히 데이터를 버린다** — 개선 과제로 남겨 둠 (오류 노트) |
| `save`: `with_extension("json.tmp")` → `write` → `rename` | **임시 파일에 다 쓴 뒤 이름만 바꾼다.** 쓰다 죽어도 원본이 반쯤 써진 채로 남지 않는다 (rename은 원자적) |
| `Json(state.todos.read().await.clone())` | 읽기 락을 잡아 **복사**하고 락은 곧바로 풀린다. 락을 쥔 채 직렬화·전송하지 않는다 |
| `state.todos.write().await` | **쓰기 락**. 이 변수가 살아 있는 동안 다른 모든 접근이 기다린다 → 가능한 짧게 |
| `.map(\|t\| t.id).max().unwrap_or(0) + 1` | 새 id = 현재 최대 id + 1. 비어 있으면 1. (⚠️ 마지막 글을 지우면 id가 재사용된다 — DB의 `AUTOINCREMENT`가 필요한 이유) |
| `todos.iter_mut().find(...).ok_or(NOT_FOUND)?` | 가변 참조로 찾고, 없으면 404로 즉시 반환 |
| `todos.retain(\|t\| t.id != id)` | 조건에 맞는 것만 남긴다. 길이가 안 변했으면 없던 id → 404 |
| `state.save(&todos)` | **쓰기 락을 쥔 채로 저장한다.** 저장이 끝나기 전에 다른 요청이 중간 상태를 보지 못하게 하려는 의도적 선택 (처리량은 희생) |

### 3-3. 화면 바인딩 — 같은 파일의 HTML/JS

```html
<form id="form"><input id="title" autocomplete="off"><button>추가</button></form>
<ul id="list"></ul>
<script>
const input = document.getElementById("title");

// ① 로컬 저장소: 쓰다 만 글이 새로고침해도 남는다 (서버 저장과는 별개의 "임시 저장")
input.value = localStorage.getItem("draft") ?? "";
input.addEventListener("input", () => localStorage.setItem("draft", input.value));

// ② 바인딩: 서버 데이터 → DOM
async function render() {
  const todos = await (await fetch("/api/todos")).json();
  document.getElementById("list").replaceChildren(...todos.map((t) => {
    const li = document.createElement("li");
    const span = document.createElement("span");
    span.textContent = t.title;     // innerHTML이 아니라 textContent → XSS 방지
    span.onclick = async () => { await fetch(`/api/todos/${t.id}`, { method: "PATCH" }); render(); };
    li.append(span);
    return li;
  }));
}

// ③ 이벤트: 화면 → 서버
document.getElementById("form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const res = await fetch("/api/todos", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title: input.value }),
  });
  if (res.ok) { input.value = ""; localStorage.removeItem("draft"); render(); }
});
render();
</script>
```

- **`textContent`로 넣는다.** `innerHTML`에 제목을 넣으면 `<img onerror=...>` 같은 입력이 실행된다(XSS).
- **서버가 성공(`res.ok`)을 답한 뒤에만** 입력창을 비우고 다시 그린다. 낙관적으로 먼저 비우면 실패했을 때 사용자가 쓴 글을 잃는다.

### 3-4. 실행 결과 (서버 재시작 검증)

```
$ curl -X POST http://127.0.0.1:3003/api/todos -H 'content-type: application/json' -d '{"title":"learn axum"}'
{"id":1,"title":"learn axum","done":false}

$ curl -X POST ... -d '{"title":"   "}'      → 400  (공백만 있는 제목은 거절)

$ curl -X PATCH http://127.0.0.1:3003/api/todos/1
{"id":1,"title":"learn axum","done":true}

$ cat todos.json
[ { "id": 1, "title": "learn axum", "done": true } ]

# ── 서버를 껐다가 다시 켠다 ──
$ curl http://127.0.0.1:3003/api/todos
[{"id":1,"title":"learn axum","done":true}]          ← 유지됨
```

---

## ④ 🔗 연동 포인트

### 데이터를 화면과 묶는 흐름 정리

```
브라우저 JS                      서버(Axum)                      디스크
────────────                     ───────────                    ───────
render()  ── GET /api/todos ───▶ read 락 → clone → JSON
submit    ── POST /api/todos ──▶ write 락 → Vec에 push ──────▶ todos.json.tmp → rename
          ◀── 201 + 새 Todo ───
render()  (다시 GET)
```

### 로컬 저장소 vs 서버 파일

| | `localStorage` | 서버 `todos.json` |
|---|---|---|
| 어디에 | 사용자 브라우저 | 서버 디스크 |
| 용도 | 임시 초안, UI 설정(다크모드), **민감하지 않은** 값 | 모두가 공유해야 하는 **진짜 데이터** |
| 용량 | 약 5MB | 디스크 한도 |
| 주의 | **JS가 읽을 수 있다**(XSS에 취약) → 토큰·비밀 저장 금지 (study7) | 동시 접근 → 락, 여러 서버 인스턴스에는 부적합 |

### 파일 저장의 한계 → DB로

- 데이터가 커질수록 **매번 전체를 다시 쓰는** 비용이 커진다.
- 서버를 **둘 이상** 띄우면 각자 자기 파일을 갖는다.
- 조건 검색·정렬·관계(글-작성자)를 직접 구현해야 한다.

그래서 [study6](study6.md)에서 DB로 넘어간다. 여기서 만든 `State`와 모델 타입은 그대로 쓰고 **저장 방식만** 바꾼다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 컴파일 에러: `future cannot be sent between threads safely` … `MutexGuard ... held across an await point` | `std::sync::Mutex`/`RwLock`의 guard를 `.await` 너머로 들고 있음 | 락을 잡은 채 `.await`가 필요하면 `tokio::sync::RwLock`. 아니면 guard가 `.await` 전에 drop되도록 블록 `{ }`으로 범위를 좁힌다 |
| 요청이 **영원히 멈춤** (데드락) | 같은 태스크에서 `write()`를 잡은 채 다시 `read()`/`write()`를 호출 | 락 범위를 짧게. 한 핸들러 안에서 같은 락을 두 번 잡지 않는다 |
| 서버를 껐다 켰더니 데이터가 사라짐 | 메모리 상태만 쓰고 파일에 저장하지 않음 | `save()` 호출 확인. 파일 경로가 **실행 위치 기준 상대경로**라서 다른 폴더에서 실행하면 다른 파일이 생긴다 |
| `todos.json`을 손으로 고쳤다가 깨졌더니 **목록이 비어 있고, 다음 저장 때 원본이 덮어써짐** | `from_str(...).unwrap_or_default()`가 깨진 JSON을 빈 목록으로 **조용히** 처리 | 깨졌으면 시작을 실패시키거나, 원본을 `.bak`으로 옮긴 뒤 시작하도록 고친다 (직접 해보기 3) |
| 마지막 항목을 지우고 새로 추가하면 **지운 것과 같은 id**가 생김 | id를 `max+1`로 계산 | 별도 카운터를 저장하거나 UUID, 또는 DB의 자동 증가 |
| 한글 제목이 `???`로 저장/표시 | 터미널에서 한글 JSON을 `-d`로 보냄 (코드페이지) | UTF-8 파일로 저장해 `curl --data-binary @file.json` |
| 제목에 `<script>`를 넣었더니 실행됨 | `innerHTML`로 그림 | `textContent`로 넣는다 |
| 동시에 많이 요청하면 느려짐 | 쓰기 락을 쥔 채 파일 쓰기 (직렬화) | 학습용으로는 허용. 쓰기가 많다면 DB나 별도 쓰기 태스크(채널)로 분리 |

---

## ⑥ 직접 해보기

1. **필드 추가**: `Todo`에 `createdAt`(문자열 시각)을 추가하고, 이전 버전의 `todos.json`(필드 없음)이 있어도 읽히도록 `#[serde(default)]`를 쓴다. 어떤 JSON이 읽히고 어떤 것이 깨지는지 실험한다.
2. **필터**: `GET /api/todos?done=true`로 완료/미완료만 돌려주도록 `Query<Filter>`를 추가한다 (study4의 `Query` 복습).
3. **깨진 파일을 조용히 버리지 않기**: `load`가 JSON 파싱에 실패하면 파일을 `todos.json.broken`으로 옮기고 경고 로그를 남긴 뒤 빈 목록으로 시작하도록 고친다.

---

## ⑦ ☐ 체크리스트

- ☐ 데이터가 사는 곳(요청 지역 / 서버 메모리 / 파일 / DB / 브라우저)의 차이를 표 없이 설명할 수 있다
- ☐ `Arc<RwLock<Vec<T>>>`의 각 겹이 하는 일을 설명할 수 있다
- ☐ `read()`와 `write()`의 차이, 쓰기 락을 오래 쥐면 안 되는 이유를 안다
- ☐ tokio의 `RwLock`을 쓰는 이유(`.await` 너머로 guard를 들고 있기 위해)를 설명할 수 있다
- ☐ `#[serde(rename_all = "camelCase")]`로 JSON 이름을 바꿨다
- ☐ 임시 파일 → `rename`으로 저장하는 이유를 설명할 수 있다
- ☐ 서버를 재시작해도 데이터가 남는 것을 직접 확인했다
- ☐ `innerHTML` 대신 `textContent`를 쓰는 이유를 안다
- ☐ `localStorage`에 넣으면 안 되는 것을 안다

---

## ⑧ 다음 단계 연결

데이터가 생겼으니 [study4](study4.md)에서는 그 데이터를 **URL로 가리키는 법**을 다룬다: 동적 경로·쿼리·리다이렉트·404 처리, 그리고 프론트와 백엔드의 경로를 어긋나지 않게 맞추는 방법.
