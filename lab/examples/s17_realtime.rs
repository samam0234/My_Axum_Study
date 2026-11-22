//! study17 — 실시간·확장: WebSocket 채팅 · SSE 알림 · 대용량 파일 스트리밍 업로드.
//!
//! 실행: `cargo run --example s17_realtime`  →  http://127.0.0.1:3017
//!
//! 하나의 `broadcast` 채널이 WebSocket과 SSE 양쪽에 같은 메시지를 흘려 준다.
//!   POST /notify ──┐
//!   WS 채팅 입력 ──┴─▶ broadcast::Sender ──┬─▶ WebSocket 구독자들
//!                                          └─▶ SSE(/events) 구독자들
//!
//! WebSocket은 **양방향**(채팅), SSE는 **서버→클라이언트 단방향**(알림)이다.
//! 단방향이면 SSE가 더 단순하다: 일반 HTTP라서 프록시·인증·재연결(EventSource)이 공짜다.

use std::{convert::Infallible, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Multipart, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::{SinkExt, Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{io::AsyncWriteExt, sync::broadcast};
use tokio_stream::wrappers::BroadcastStream;

/// 업로드 한도 (20MB). 이걸 넘기는 순간 스트림을 끊는다.
const MAX_UPLOAD: usize = 20 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<String>,
    upload_dir: Arc<PathBuf>,
}

#[tokio::main]
async fn main() {
    // 용량 64: 느린 구독자가 64개 넘게 밀리면 그 구독자만 `Lagged`를 받고 일부를 건너뛴다.
    // (한 명이 느리다고 서버 메모리가 무한히 늘거나 다른 구독자가 막히지 않는다)
    let (tx, _rx) = broadcast::channel(64);
    let upload_dir = PathBuf::from(std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".into()));
    tokio::fs::create_dir_all(&upload_dir).await.unwrap();

    let state = AppState { tx, upload_dir: Arc::new(upload_dir) };

    let app = Router::new()
        .route("/", get(|| async { Html(PAGE) }))
        .route("/ws", get(ws_handler))
        .route("/events", get(sse_handler))
        .route("/notify", post(notify))
        // 이 라우트만 본문 한도를 크게 연다. (기본 2MB)
        .route("/upload", post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD + 1024 * 1024)))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3017").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ── WebSocket ────────────────────────────────────────────────────────────────

/// 일반 HTTP 요청(`Upgrade: websocket`)을 WebSocket 연결로 "승격"한다.
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| chat(socket, state))
}

async fn chat(socket: WebSocket, state: AppState) {
    // 한 소켓을 "보내기"와 "받기"로 쪼개 각각 다른 태스크에서 다룬다.
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // 태스크 A: 채널에 올라온 메시지를 이 클라이언트에게 보낸다.
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(text) => {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break; // 클라이언트가 떠남
                    }
                }
                // 너무 느려서 일부를 놓침. 끊지 않고 계속 간다.
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    eprintln!("[ws] {skipped}개 메시지를 건너뜀");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 태스크 B: 이 클라이언트가 보낸 메시지를 채널에 올린다.
    let tx = state.tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // 길이 제한: 클라이언트가 보낸 값은 믿지 않는다.
                    let text: String = text.as_str().chars().take(500).collect();
                    let _ = tx.send(json!({ "type": "chat", "text": text }).to_string());
                }
                Message::Close(_) => break,
                _ => {} // Ping/Pong은 axum이 알아서 처리한다
            }
        }
    });

    // 둘 중 하나가 끝나면 나머지도 정리한다. (안 하면 연결이 끊긴 뒤에도 태스크가 남는다)
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

// ── SSE ─────────────────────────────────────────────────────────────────────

/// 브라우저의 `new EventSource("/events")`가 연결하는 곳. 끊기면 브라우저가 알아서 다시 붙는다.
async fn sse_handler(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.tx.subscribe()).filter_map(|msg| async move {
        // Lagged 에러는 건너뛰고 정상 메시지만 이벤트로 바꾼다.
        msg.ok().map(|text| Ok(Event::default().event("notice").data(text)))
    });

    // 프록시가 "아무 데이터도 없는 연결"을 끊지 않도록 주기적으로 주석(`:`)을 보낸다.
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Deserialize)]
struct Notice {
    message: String,
}

/// 서버 코드 어디서든 `tx.send(...)` 한 줄이면 모든 구독자에게 알림이 간다.
async fn notify(State(state): State<AppState>, Json(body): Json<Notice>) -> StatusCode {
    // 구독자가 0명이면 send가 Err를 돌려준다. 알림이 아무에게도 안 갔을 뿐 오류는 아니다.
    let _ = state.tx.send(json!({ "type": "notice", "text": body.message }).to_string());
    StatusCode::ACCEPTED
}

// ── 대용량 파일: 메모리에 올리지 않고 디스크로 흘려 보내기 ─────────────────────

async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    while let Ok(Some(mut field)) = multipart.next_field().await {
        if field.name() != Some("file") {
            continue;
        }

        // ⚠️ 클라이언트가 준 파일명은 저장 경로에 쓰지 않는다 (`../../etc/passwd` 같은 값이 올 수 있다).
        //    서버가 만든 이름으로 저장한다.
        let stored = format!("{}.bin", uuid::Uuid::new_v4());
        let path = state.upload_dir.join(&stored);
        let mut file = match tokio::fs::File::create(&path).await {
            Ok(f) => f,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

        let mut total = 0usize;
        let mut hasher = Sha256::new();

        // `field.chunk()`는 네트워크에서 도착하는 만큼만 꺼낸다. 파일 전체가 메모리에 올라가지 않는다.
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    total += chunk.len();
                    if total > MAX_UPLOAD {
                        drop(file);
                        let _ = tokio::fs::remove_file(&path).await; // 반쯤 받은 파일을 지운다
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            Json(json!({ "error": { "code": "PAYLOAD_TOO_LARGE", "message": "20MB 이하만 올릴 수 있습니다." } })),
                        )
                            .into_response();
                    }
                    hasher.update(&chunk);
                    if file.write_all(&chunk).await.is_err() {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    let _ = tokio::fs::remove_file(&path).await;
                    return StatusCode::BAD_REQUEST.into_response();
                }
            }
        }

        let digest = hex::encode(hasher.finalize());
        // 업로드가 끝났음을 모두에게 알린다. (알림 기능과 파일 처리가 이어지는 지점)
        let _ = state.tx.send(json!({ "type": "notice", "text": format!("파일 업로드 완료 ({total} bytes)") }).to_string());

        return (
            StatusCode::CREATED,
            Json(json!({ "data": { "storedAs": stored, "bytes": total, "sha256": digest } })),
        )
            .into_response();
    }

    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": { "code": "BAD_REQUEST", "message": "`file` 필드가 필요합니다." } })),
    )
        .into_response()
}

const PAGE: &str = r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><title>study17 실시간</title>
<style>body{font-family:sans-serif;max-width:560px;margin:2rem auto} #log{background:#f4f4f4;padding:.6rem;height:200px;overflow:auto}</style></head>
<body>
<h1>실시간 확장</h1>
<div id="log"></div>
<form id="chat"><input id="msg" placeholder="채팅 (WebSocket)" autocomplete="off"><button>보내기</button></form>
<form id="up"><input type="file" name="file"><button>업로드</button></form>
<script>
const log = document.getElementById("log");
const add = (label, text) => { const p = document.createElement("div"); p.textContent = `[${label}] ${text}`; log.append(p); log.scrollTop = log.scrollHeight; };

// WebSocket: 양방향. 페이지가 https면 wss, http면 ws
const ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`);
ws.onmessage = (e) => add("ws", JSON.parse(e.data).text);
ws.onclose = () => add("ws", "연결 종료");
document.getElementById("chat").onsubmit = (e) => { e.preventDefault(); const i = document.getElementById("msg"); ws.send(i.value); i.value = ""; };

// SSE: 서버 → 클라이언트. 끊기면 브라우저가 자동 재연결한다.
const es = new EventSource("/events");
es.addEventListener("notice", (e) => add("sse", JSON.parse(e.data).text));

document.getElementById("up").onsubmit = async (e) => {
  e.preventDefault();
  const res = await fetch("/upload", { method: "POST", body: new FormData(e.target) });
  add("upload", `${res.status} ${await res.text()}`);
};
</script></body></html>"#;
