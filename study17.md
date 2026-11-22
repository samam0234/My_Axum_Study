# study17 — 실시간 · 확장

> 🎯 **목표**: 요청-응답을 넘어 **서버가 먼저 말하는** 기능을 붙인다: WebSocket 채팅(양방향), SSE 알림(서버→클라이언트), 대용량 파일 스트리밍 업로드. 하나의 `broadcast` 채널이 이 모두를 잇는 구조와, 연결이 오래 유지되는 기능이 서버 운영에 미치는 영향을 이해한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s17_realtime.rs](lab/examples/s17_realtime.rs) (포트 3017) |
| 검증 | 실제 서버 + **Node 24의 내장 WebSocket 클라이언트**(두 연결 간 메시지 전달) + `curl -N`(SSE) + `curl -F`(업로드, sha256 대조, 22MB 초과 거절) |
| 사용 기능 | axum `ws`·`multipart` feature, `tokio::sync::broadcast`, `tokio-stream`(`BroadcastStream`), `futures-util` |

---

## ① 목표

1. HTTP 업그레이드로 WebSocket이 시작되는 과정과 `split()`으로 보내기/받기를 분리하는 이유를 안다.
2. SSE로 단방향 알림을 만들고, WebSocket과 **언제 무엇을 쓸지** 고른다.
3. `broadcast` 채널로 여러 구독자에게 같은 메시지를 보낸다 (느린 구독자 처리 포함).
4. 큰 파일을 **메모리에 올리지 않고** 디스크로 스트리밍하며 크기 제한·정리를 한다.
5. 장시간 연결이 종료·타임아웃·프록시와 만나는 지점을 안다.

---

## ② 📖 이론

### 세 가지 실시간 방식

| | **폴링** | **SSE** (Server-Sent Events) | **WebSocket** |
|---|---|---|---|
| 방향 | 클라이언트가 계속 묻는다 | **서버 → 클라이언트** 단방향 | **양방향** |
| 프로토콜 | 일반 HTTP | 일반 HTTP (`text/event-stream`) | HTTP로 시작해 **업그레이드** → 별도 프레임 프로토콜 |
| 재연결 | 자연스럽다 | 브라우저(`EventSource`)가 **자동** | 직접 구현 |
| 프록시·인증 | 쉬움 | 쉬움 (일반 HTTP, 쿠키/헤더) | 설정 필요(업그레이드 허용, 타임아웃) |
| 어울리는 곳 | 가끔 확인 (작업 상태, study15) | **알림, 진행률, 로그 스트림, AI 토큰 스트림(study18)** | **채팅, 협업 편집, 게임** |

**고르는 기준**: 클라이언트가 **서버로 자주 말하지 않는다면 SSE**가 더 단순하다. 양방향이 정말 필요할 때만 WebSocket.

### 하나의 채널, 여러 구독자 — `broadcast`

```
POST /notify ────┐
WS 채팅 입력 ─────┴─▶  broadcast::Sender<String>  ─┬─▶ WebSocket 구독자 A, B, C …
                       (용량 64)                    └─▶ SSE 구독자 X, Y …
```

- `tx.send(msg)`는 **구독자 모두**에게 복제되어 간다. 구독자가 0명이면 `Err`이지만 오류가 아니다(아무도 안 들었을 뿐).
- 채널에는 **용량**이 있다(64). 한 구독자가 느려서 64개 넘게 밀리면 **그 구독자만** `Lagged(n)`을 받고 일부를 건너뛴다 → 서버 메모리가 무한히 늘거나 **다른 구독자가 막히지 않는다.**

### WebSocket 연결이 시작되는 방법

```
클라이언트: GET /ws  Upgrade: websocket  Connection: Upgrade  Sec-WebSocket-Key: ...
서버:       101 Switching Protocols     ← 이 순간부터 같은 TCP 연결에서 WebSocket 프레임이 오간다
```

axum에서는 `WebSocketUpgrade` extractor가 위 협상을 맡고, `ws.on_upgrade(|socket| ...)`에 넘긴 비동기 함수가 **연결이 끊길 때까지** 실행된다. 연결 하나가 태스크 하나(보통 둘)다.

---

## ③ 🛠️ 실기

### 3-1. 상태와 라우터

```rust
#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<String>,
    upload_dir: Arc<PathBuf>,
}

let (tx, _rx) = broadcast::channel(64);          // 용량 64

let app = Router::new()
    .route("/", get(|| async { Html(PAGE) }))
    .route("/ws", get(ws_handler))
    .route("/events", get(sse_handler))
    .route("/notify", post(notify))
    // 이 라우트만 본문 한도를 크게 연다. (기본 2MB)
    .route("/upload", post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD + 1024 * 1024)))
    .with_state(state);
```

### 3-2. WebSocket 채팅

```rust
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| chat(socket, state))
}

async fn chat(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();       // 보내기 / 받기 분리
    let mut rx = state.tx.subscribe();

    // 태스크 A: 채널의 메시지를 이 클라이언트에게 보낸다
    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(text) => { if sender.send(Message::Text(text.into())).await.is_err() { break; } }   // 클라이언트가 떠남
                Err(broadcast::error::RecvError::Lagged(skipped)) => eprintln!("[ws] {skipped}개 메시지를 건너뜀"),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 태스크 B: 이 클라이언트가 보낸 메시지를 채널에 올린다
    let tx = state.tx.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    let text: String = text.as_str().chars().take(500).collect();      // 길이 제한: 클라이언트 값은 믿지 않는다
                    let _ = tx.send(json!({ "type": "chat", "text": text }).to_string());
                }
                Message::Close(_) => break,
                _ => {}                                                                 // Ping/Pong은 axum이 처리
            }
        }
    });

    // 둘 중 하나가 끝나면 나머지도 정리한다
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}
```

| 코드 | 설명 |
|---|---|
| `WebSocketUpgrade` | 업그레이드 협상 extractor. 실패하면(일반 HTTP로 접근) 400대 응답 |
| `socket.split()` | `(Sink, Stream)`으로 분리. **한 태스크가 읽으면서 동시에 보내려면** 둘이 분리돼야 한다 (`futures_util::{SinkExt, StreamExt}`) |
| 태스크 두 개 | A는 "채널→클라이언트", B는 "클라이언트→채널". 서로 기다리지 않는다 |
| `Lagged(skipped)` | 느린 구독자. **끊지 않고 계속 간다** (로그만) |
| `chars().take(500)` | 한 메시지 길이 제한. 상한 없이 받으면 한 연결이 메모리를 먹을 수 있다 |
| `Message::Text(text.into())` | axum 0.8의 `Message::Text`는 `String`이 아니라 `Utf8Bytes` — `.into()`로 변환 |
| `tokio::select!` + `abort()` | 한쪽이 끝나면 다른 쪽 태스크도 **정리**한다. 안 하면 연결이 끊긴 뒤에도 태스크가 남는다(누수) |

### 3-3. SSE 알림

```rust
async fn sse_handler(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.tx.subscribe()).filter_map(|msg| async move {
        msg.ok().map(|text| Ok(Event::default().event("notice").data(text)))        // Lagged 에러는 건너뜀
    });
    Sse::new(stream).keep_alive(KeepAlive::default())       // 프록시가 "아무 데이터도 없는 연결"을 끊지 않도록 주기적으로 주석(:)
}

async fn notify(State(state): State<AppState>, Json(body): Json<Notice>) -> StatusCode {
    let _ = state.tx.send(json!({ "type": "notice", "text": body.message }).to_string());
    StatusCode::ACCEPTED
}
```

브라우저:

```js
const es = new EventSource("/events");                       // 끊기면 자동 재연결
es.addEventListener("notice", (e) => add("sse", JSON.parse(e.data).text));
```

| 코드 | 설명 |
|---|---|
| `Sse<impl Stream<...>>` | 반환 타입이 스트림이면 연결이 열린 채로 이벤트가 계속 흘러간다 (`Content-Type: text/event-stream`) |
| `Event::default().event("notice").data(...)` | 이벤트 이름 + 데이터. 브라우저가 이름으로 구독 |
| `KeepAlive::default()` | 주기적 `:` 주석 줄. 프록시·로드밸런서의 유휴 연결 끊김 방지 |
| `notify`가 `202` | 알림을 큐에 넣었다는 뜻 (구독자가 없어도 오류 아님) |

### 3-4. 대용량 파일 — 디스크로 스트리밍

```rust
async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    while let Ok(Some(mut field)) = multipart.next_field().await {
        if field.name() != Some("file") { continue; }

        // ⚠️ 클라이언트가 준 파일명은 저장 경로에 쓰지 않는다 → 서버가 만든 이름으로
        let stored = format!("{}.bin", uuid::Uuid::new_v4());
        let path = state.upload_dir.join(&stored);
        let mut file = tokio::fs::File::create(&path).await?;

        let mut total = 0usize;
        let mut hasher = Sha256::new();

        // field.chunk()는 네트워크에서 도착하는 만큼만 꺼낸다. 파일 전체가 메모리에 올라가지 않는다.
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    total += chunk.len();
                    if total > MAX_UPLOAD {
                        drop(file);
                        let _ = tokio::fs::remove_file(&path).await;            // 반쯤 받은 파일을 지운다
                        return (StatusCode::PAYLOAD_TOO_LARGE, Json(json!({ "error": { "code": "PAYLOAD_TOO_LARGE", ... } }))).into_response();
                    }
                    hasher.update(&chunk);
                    file.write_all(&chunk).await?;
                }
                Ok(None) => break,
                Err(_) => { let _ = tokio::fs::remove_file(&path).await; return StatusCode::BAD_REQUEST.into_response(); }
            }
        }
        let digest = hex::encode(hasher.finalize());
        let _ = state.tx.send(json!({ "type": "notice", "text": format!("파일 업로드 완료 ({total} bytes)") }).to_string());
        return (StatusCode::CREATED, Json(json!({ "data": { "storedAs": stored, "bytes": total, "sha256": digest } }))).into_response();
    }
    /* `file` 필드 없음 → 400 */
}
```

| 코드 | 설명 |
|---|---|
| `DefaultBodyLimit::max(MAX_UPLOAD + 1MB)` | 이 라우트만 20MB(+멀티파트 오버헤드). 나머지는 기본 2MB |
| `field.chunk()` | **스트리밍**. `field.bytes()`는 전체를 메모리에 올린다 (demo의 썸네일 5MB는 그래도 되지만 큰 파일은 안 된다) |
| `total > MAX_UPLOAD` → 413 + 파일 삭제 | 선언된 `Content-Length`를 믿지 않고 **실제로 받은 바이트**를 센다. 중단 시 반쯤 쓴 파일을 지운다 |
| 해시를 흘리면서 계산 | 한 번 읽을 때 무결성 지문까지 (`sha256`) |
| 업로드 완료를 `tx.send`로 알림 | 파일 처리와 실시간 알림이 **같은 채널**로 이어진다 |

### 3-5. 실행 결과

```
# SSE 구독자를 열어 두고 알림을 쏜다 + WebSocket 두 연결(A, B)에서 A가 채팅을 보낸다
$ curl -X POST http://127.0.0.1:3017/notify -d '{"message":"hello sse"}'        → 202

WebSocket B가 받은 것:  [ '{"text":"안녕 from A","type":"chat"}' ]               ← A의 메시지가 B에게 전달됨

SSE 스트림이 받은 것:
event: notice
data: {"text":"hello sse","type":"notice"}

event: notice
data: {"text":"안녕 from A","type":"chat"}                                      ← 같은 채널이라 WS 채팅도 SSE에 도달

# 업로드
$ curl -F "file=@3MB.bin;filename=../../evil.txt" http://127.0.0.1:3017/upload
{"data":{"bytes":3000000,"sha256":"900ee6aa313c32c0d16d093546d386850870bcf0aad6da43952f39ac800226c9","storedAs":"702c12f8-be14-4bf0-85e8-9c396a5119f1.bin"}}
$ sha256sum 3MB.bin   → 900ee6aa313c32c0d16d093546d386850870bcf0aad6da43952f39ac800226c9      ← 일치
(디스크에는 UUID 이름 파일 1개만. `../../evil.txt`는 무시됨)

$ curl -F "file=@22MB.bin" ...     → 413 {"error":{"code":"PAYLOAD_TOO_LARGE","message":"20MB 이하만 올릴 수 있습니다."}}
(업로드 폴더에 남은 파일 수: 변화 없음 — 반쯤 받은 파일이 삭제됨)
$ curl -F "other=@3MB.bin" ...    → 400 {"error":{"code":"BAD_REQUEST","message":"`file` 필드가 필요합니다."}}
```

---

## ④ 🔗 연동 포인트

| 연동 | 어떻게 |
|---|---|
| **인증 (study7)** | WebSocket은 `Authorization` 헤더를 브라우저 JS가 마음대로 못 붙인다. ① **쿠키**(같은 도메인, `HttpOnly`) ② 연결 URL의 **짧은 수명 일회용 토큰**(`/ws?ticket=...`, 서버가 발급·검증·즉시 폐기). 긴 JWT를 쿼리스트링에 넣으면 로그에 남는다. `AuthUser`는 `WebSocketUpgrade`와 **같은 핸들러 인자**로 적어 업그레이드 전에 401로 거절한다 |
| **작업 큐 (study15)** | 작업 워커가 완료 시 `tx.send(...)` → 폴링 대신 **SSE로 진행률/완료 푸시** |
| **DB (study6, 11)** | 글/댓글이 생성되면 `tx.send`로 "새 댓글" 알림. 단, 서버가 **여러 대**면 메모리 `broadcast`는 같은 프로세스에만 닿는다 → Redis pub/sub, PostgreSQL `LISTEN/NOTIFY` 같은 **공유 메시지 버스** |
| **프론트 (study12)** | `EventSource`/`WebSocket` 연결을 한 곳(훅/스토어)에서 관리, 서버 상태 캐시를 알림으로 **무효화** |
| **AI (study18)** | 모델의 토큰 스트림을 SSE로 중계하는 것이 같은 기술. 클라이언트가 떠나면 **업스트림도 취소**하는 원리가 똑같다 |
| **리버스 프록시 (study10)** | `Upgrade`/`Connection` 헤더 전달, 읽기 타임아웃 연장(기본 60초면 조용한 WebSocket이 끊긴다), SSE는 **응답 버퍼링 끄기**(`proxy_buffering off` 류) |
| **파일 처리 (study13)** | 업로드 완료 알림 → 후처리(썸네일 생성은 `spawn_blocking`, study15) → 완료 알림 |

### 장시간 연결이 운영에 미치는 영향

| 항목 | 영향 | 대응 |
|---|---|---|
| **정상 종료 (study10)** | `with_graceful_shutdown`은 **열려 있는 연결이 끝나기를 기다린다.** SSE/WebSocket은 안 끝난다 → `docker stop`이 10초 후 SIGKILL | 종료 신호를 받으면 구독자에게 **종료 이벤트를 보내고 연결을 닫는다** (`broadcast`에 종료 메시지, 또는 `CancellationToken`) |
| **전역 타임아웃 (study8)** | `TimeoutLayer`(5초)가 걸려 있으면 스트림 라우트가 끊긴다 | 스트리밍 라우트는 **타임아웃 계층 밖**에 둔다 |
| **연결 수** | 연결마다 메모리·파일 디스크립터. 수만 개면 한도 | 연결 수 상한, OS `ulimit -n`, 유휴 연결 정리 |
| **압축 (CompressionLayer)** | SSE가 **버퍼링**되어 이벤트가 늦게 도착할 수 있다 | `text/event-stream`은 압축에서 제외 (tower-http 기본 설정은 이를 처리하지만 프록시 쪽을 확인) |
| **Slowloris류 부하** | 연결을 열어만 두는 공격 | 인증 후에만 구독, 연결 수 제한, 프록시의 유휴 타임아웃 |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 컴파일 에러: `ws` 모듈이 없음 | axum의 **`ws` feature가 꺼져 있음** | `cargo add axum --features ws` (업로드는 `multipart`) |
| `Message::Text(text)`에 `String`을 넣으면 타입 에러 | axum 0.8은 `Utf8Bytes` | `Message::Text(text.into())`, 받을 때는 `text.as_str()` |
| 보내기와 받기를 한 루프에서 하려니 막힘 | `WebSocket` 하나로는 동시에 읽고 쓰기 어렵다 | `socket.split()`로 분리해 태스크 둘 |
| 브라우저가 한 메시지 후 연결이 끊김 | 수신 태스크가 **패닉/에러**로 끝나 `select!`가 전체를 정리 | 로그로 원인 확인, 수신 루프에서 오류를 `?`로 던지지 말고 처리 |
| 연결이 끊긴 뒤에도 서버에 태스크가 남음 | 한쪽 태스크 종료 시 반대쪽을 `abort`하지 않음 | `select!` + `abort()` |
| 한 사용자가 느려서 **전체가 느려짐** | 무한 버퍼 / 느린 구독자를 기다림 | `broadcast`는 **용량 + `Lagged`** (느린 쪽만 놓침) |
| SSE가 이벤트 뭉치로 한꺼번에 도착 | 프록시/압축의 **응답 버퍼링** | 프록시 버퍼링 끄기, SSE는 압축 제외, `KeepAlive` |
| SSE가 60초마다 끊김 | 프록시의 유휴 타임아웃 | `KeepAlive`로 주기적 전송, 프록시 `read_timeout` 연장 |
| `/notify`는 202인데 아무도 못 받음 | 구독자가 0명 (또는 서버가 여러 대라 다른 프로세스에 구독자가 있음) | 메시지 버스(Redis 등) |
| `ws://`로 접속했는데 HTTPS 페이지에서 차단 | **혼합 콘텐츠**: HTTPS 페이지는 `wss://`만 허용 | `${location.protocol === "https:" ? "wss" : "ws"}://...` |
| 22MB 업로드가 서버 도달 전에 막힘 | 전역 본문 제한(2MB) 또는 **프록시의 본문 제한**(예: nginx 기본 1MB) | 라우트별 `DefaultBodyLimit`, 프록시 `client_max_body_size` |
| 업로드 중 연결이 끊긴 뒤 **쓰레기 파일**이 남음 | 중단 시 정리 코드가 없음 | 에러/한도 초과 경로마다 `remove_file` |
| `docker stop`이 10초 걸림 (SSE/WS 연결이 열려 있을 때) | graceful shutdown이 열린 스트림을 기다림 | 종료 시 스트림을 닫는 신호를 보낸다 |

---

## ⑥ 직접 해보기

1. **방(room) 나누기**: `broadcast` 하나 대신 `HashMap<String, broadcast::Sender<String>>`을 두고 `/ws?room=general`처럼 방별 채널을 만든다. 방이 비면 맵에서 지우는 것까지.
2. **로그인한 사용자만 채팅**: study7의 `AuthUser`(쿠키 또는 일회용 ticket)를 `ws_handler`에 붙이고, 메시지에 `from: 닉네임`을 서버가 채운다. 클라이언트가 보낸 `from`은 무시한다.
3. **종료 이벤트**: `Ctrl+C`/SIGTERM 시 구독자에게 `{"type":"shutdown"}`을 보내고 스트림을 끝내, 열린 SSE 연결이 있어도 `docker stop`이 빨리 끝나게 한다.

---

## ⑦ ☐ 체크리스트

- ☐ 폴링 / SSE / WebSocket을 방향·프로토콜·재연결·프록시 관점에서 비교하고 상황에 맞게 고를 수 있다
- ☐ WebSocket의 업그레이드(`101`)와 `split()`으로 보내기/받기를 분리하는 이유를 설명할 수 있다
- ☐ `broadcast`의 용량과 `Lagged`가 느린 구독자를 어떻게 다루는지 설명할 수 있다
- ☐ 한쪽 태스크가 끝나면 다른 쪽을 `abort`해야 하는 이유(누수)를 안다
- ☐ `EventSource`로 SSE를 받고 `KeepAlive`가 왜 필요한지 안다
- ☐ 대용량 업로드를 `chunk()`로 스트리밍하고, **실제 받은 바이트**로 한도를 검사하며, 실패 시 파일을 지우는 코드를 설명할 수 있다
- ☐ 클라이언트가 준 파일명을 경로에 쓰지 않는 이유를 안다 (UUID 저장 + sha256 대조를 확인했다)
- ☐ WebSocket 인증 방법(쿠키 / 일회용 ticket)과 쿼리스트링에 긴 토큰을 넣으면 안 되는 이유를 안다
- ☐ 장시간 연결이 graceful shutdown·타임아웃·프록시·압축에 주는 영향을 말할 수 있다

---

## ⑧ 다음 단계 연결

서버가 먼저 말하는 방법(SSE 스트림)을 익혔으니 [study18](study18.md)에서는 그것을 **AI 모델의 토큰 스트림**에 그대로 적용한다. AI 서버를 호출하고, 프롬프트를 설계하고, 스트리밍 응답을 화면에 흘리고, MCP 서버·클라이언트의 기초까지 — 노션의 AI 미니 프로젝트(DocuForge)의 오케스트레이터 자리에 Axum을 놓아 본다.
