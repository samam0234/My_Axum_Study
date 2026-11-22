//! study3 — 데이터 다루기: 상태(메모리) · 모델 · 파일 저장 · 화면 바인딩.
//!
//! 실행: `cargo run --example s03_store`  →  http://127.0.0.1:3003
//!
//! - 서버 상태:   `Arc<RwLock<Vec<Todo>>>`
//! - 파일 저장:   변경할 때마다 `todos.json`에 쓰고, 시작할 때 읽어서 복구한다.
//! - 화면 바인딩: `/` 페이지의 JS가 JSON을 받아 DOM에 그린다. 입력 중인 글은 localStorage에 임시 저장한다.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::Html,
    routing::{get, patch},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// 모델. DB 행·JSON 파일·API 응답이 같은 모양이라 한 타입으로 쓴다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Todo {
    id: u32,
    title: String,
    done: bool,
}

#[derive(Debug, Deserialize)]
struct NewTodo {
    title: String,
}

#[derive(Clone)]
struct AppState {
    /// 읽기는 동시에 여러 개, 쓰기는 하나만. `std::sync::RwLock`이 아니라 tokio의 것을 쓰는 이유:
    /// 락을 잡은 채로 `.await`(파일 쓰기)를 해야 하기 때문이다.
    todos: Arc<RwLock<Vec<Todo>>>,
    file: Arc<PathBuf>,
}

impl AppState {
    /// 시작할 때 파일에서 복구한다. 파일이 없으면 빈 목록으로 시작한다.
    async fn load(file: PathBuf) -> Self {
        let todos = match tokio::fs::read_to_string(&file).await {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Self {
            todos: Arc::new(RwLock::new(todos)),
            file: Arc::new(file),
        }
    }

    /// 임시 파일에 쓰고 rename한다. 쓰는 도중에 죽어도 **반쯤 써진 파일이 남지 않는다.**
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3003").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn list(State(state): State<AppState>) -> Json<Vec<Todo>> {
    // read()는 읽기 락. 다른 읽기와 동시에 잡을 수 있다.
    Json(state.todos.read().await.clone())
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<NewTodo>,
) -> Result<(StatusCode, Json<Todo>), StatusCode> {
    let title = body.title.trim().to_string();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // write()는 쓰기 락. 잡는 동안 다른 읽기·쓰기가 기다린다 → 짧게 잡는다.
    let mut todos = state.todos.write().await;
    let id = todos.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let todo = Todo { id, title, done: false };
    todos.push(todo.clone());

    state
        .save(&todos)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(todo)))
}

async fn toggle(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Json<Todo>, StatusCode> {
    let mut todos = state.todos.write().await;
    let todo = todos
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    todo.done = !todo.done;
    let updated = todo.clone();

    state
        .save(&todos)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(updated))
}

async fn remove(State(state): State<AppState>, Path(id): Path<u32>) -> StatusCode {
    let mut todos = state.todos.write().await;
    let before = todos.len();
    todos.retain(|t| t.id != id);

    if todos.len() == before {
        return StatusCode::NOT_FOUND;
    }
    match state.save(&todos).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// 화면. JS가 `/api/todos`를 받아 DOM에 묶는다(바인딩).
/// `textContent`로 넣기 때문에 제목에 `<script>`가 들어 있어도 글자 그대로 보인다.
async fn page() -> Html<&'static str> {
    Html(PAGE)
}

const PAGE: &str = r#"<!doctype html>
<html lang="ko">
<head><meta charset="utf-8"><title>Todo (study3)</title>
<style>body{font-family:sans-serif;max-width:480px;margin:2rem auto}li.done span{text-decoration:line-through;color:#888}</style>
</head>
<body>
  <h1>Todo</h1>
  <form id="form"><input id="title" placeholder="할 일" autocomplete="off"><button>추가</button></form>
  <ul id="list"></ul>
<script>
const $ = (id) => document.getElementById(id);
const input = $("title");

// ① 로컬 저장소: 쓰다 만 글이 새로고침해도 남는다. (서버 저장과는 별개의 "임시 저장")
input.value = localStorage.getItem("draft") ?? "";
input.addEventListener("input", () => localStorage.setItem("draft", input.value));

// ② 바인딩: 서버 데이터 → DOM
async function render() {
  const todos = await (await fetch("/api/todos")).json();
  const ul = $("list");
  ul.replaceChildren(...todos.map((t) => {
    const li = document.createElement("li");
    if (t.done) li.className = "done";
    const span = document.createElement("span");
    span.textContent = t.title;             // innerHTML이 아니라 textContent → XSS 방지
    span.onclick = async () => { await fetch(`/api/todos/${t.id}`, { method: "PATCH" }); render(); };
    const del = document.createElement("button");
    del.textContent = "삭제";
    del.onclick = async () => { await fetch(`/api/todos/${t.id}`, { method: "DELETE" }); render(); };
    li.append(span, " ", del);
    return li;
  }));
}

// ③ 이벤트: 화면 → 서버
$("form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const res = await fetch("/api/todos", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title: input.value }),
  });
  if (res.ok) { input.value = ""; localStorage.removeItem("draft"); render(); }
  else alert("추가 실패: " + res.status);
});

render();
</script>
</body></html>"#;
