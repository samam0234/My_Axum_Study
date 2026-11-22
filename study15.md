# study15 — 성능 · 캐시

> 🎯 **목표**: 서버를 **빠르게** 만드는 네 가지 도구 — 캐시, 비동기 처리, 백그라운드 작업, 블로킹 격리 — 를 예제로 만들고, **측정으로 효과를 확인**하는 습관을 들인다. "빨라졌을 것 같다"가 아니라 숫자로 말한다.

| 항목 | 내용 |
|---|---|
| 관련 코드 | [lab/examples/s15_cache_jobs.rs](lab/examples/s15_cache_jobs.rs) (포트 3015), 측정용 미들웨어 [demo/src/middleware.rs](demo/src/middleware.rs) (`response_time`), 비밀번호 해싱 격리 [demo/src/auth/password.rs](demo/src/auth/password.rs) |
| 검증 | 아래 수치는 모두 실제로 측정한 값 (디버그 빌드, 이 개발 PC 기준 — **절대값이 아니라 비율**을 본다) |

---

## ① 목표

1. 병목을 **추측하지 않고 측정**한다 (소요 시간, 적중률).
2. TTL 캐시(cache-aside)를 만들고 **무효화**까지 다룬다.
3. 오래 걸리는 일을 `202 Accepted` + 작업 상태 조회로 분리한다.
4. async 서버에서 **블로킹 호출이 왜 위험한지** 숫자로 확인하고 `spawn_blocking`으로 격리한다.
5. 응답 압축 같은 값싼 최적화의 효과를 안다.

---

## ② 📖 이론

### 먼저 측정, 그다음 최적화

```
측정 → 병목 찾기 → 한 가지만 고치기 → 다시 측정 → (반복)
```

추측으로 고치면 빨라지지 않거나, 복잡도만 늘린다. Rust라고 해서 모든 것이 자동으로 빠르지 않다 — **느린 DB 쿼리 하나, 막힌 워커 스레드 하나**가 언어의 이점을 지운다.

### async 서버의 속도는 "워커 스레드를 얼마나 안 막는가"

tokio의 워커 스레드(기본: CPU 코어 수)는 **수천 개의 요청을 번갈아** 처리한다. `await` 지점에서 다른 요청으로 넘어가는 것이 핵심이다. 그런데 `await`가 없는 **긴 동기 계산**(해시, 이미지 변환, 압축)을 돌리면 그 스레드는 **다른 모든 요청을 처리하지 못한다.**

```
워커 스레드 2개, 요청 4개가 각각 1초짜리 CPU 계산을 async 안에서 직접 수행
  스레드1: [요청A 1s][요청C 1s]
  스레드2: [요청B 1s][요청D 1s]      ← 이 동안 /health 같은 가벼운 요청도 기다린다
```

→ 무거운 동기 일은 `tokio::task::spawn_blocking`(별도 스레드 풀)로 보낸다.

### 캐시: 무엇을, 얼마나, 언제 지우나

| 질문 | 기준 |
|---|---|
| **무엇을** | 읽기가 많고, 계산이 비싸고, 조금 낡아도 되는 것 (집계, 목록 첫 페이지) |
| **얼마나**(TTL) | 낡아도 되는 시간. 짧을수록 정확, 길수록 빠름 |
| **언제 지우나** | **원본이 바뀔 때** (쓰기 핸들러에서 `invalidate`) — 캐시 무효화는 가장 어려운 문제다 |
| **어디에** | 프로세스 메모리(가장 빠름, 서버마다 따로) / Redis(여러 서버 공유) / HTTP 캐시(브라우저·CDN, study14의 ETag) |

**cache-aside 패턴**: 읽을 때 `캐시 조회 → 있으면 반환 → 없으면 계산 → 캐시에 저장 → 반환`.

### 백그라운드 작업: 응답과 일을 분리

요청 안에서 5초짜리 일을 기다리면 사용자는 5초 동안 멈춰 있고, 타임아웃(5초)에 걸린다. 대신:

```
POST /jobs  → 큐에 넣고 즉시 202 + { statusUrl: "/jobs/1" }
워커        → 큐에서 꺼내 처리 (queued → running → done)
GET /jobs/1 → 클라이언트가 폴링해서 상태/결과 확인
```

---

## ③ 🛠️ 실기

### 3-1. TTL 캐시 + cache-aside — [lab/examples/s15_cache_jobs.rs](lab/examples/s15_cache_jobs.rs)

```rust
struct TtlCache<V> {
    map: RwLock<HashMap<String, (Instant, V)>>,     // 키 → (만료 시각, 값)
    ttl: Duration,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<V: Clone> TtlCache<V> {
    async fn get(&self, key: &str) -> Option<V> {
        let map = self.map.read().await;
        match map.get(key) {
            Some((expires, value)) if *expires > Instant::now() => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(value.clone())
            }
            _ => { self.misses.fetch_add(1, Ordering::Relaxed); None }       // 없거나 만료
        }
    }
    async fn put(&self, key: &str, value: V) {
        self.map.write().await.insert(key.to_string(), (Instant::now() + self.ttl, value));
    }
    async fn invalidate(&self, key: &str) { self.map.write().await.remove(key); }
}

async fn report(State(state): State<AppState>) -> Json<Value> {
    let started = Instant::now();
    let (value, cache) = match state.cache.get("report").await {
        Some(v) => (v, "hit"),
        None => { let v = slow_query().await; state.cache.put("report", v.clone()).await; (v, "miss") }
    };
    Json(json!({ "data": value, "cache": cache, "elapsedMs": started.elapsed().as_millis() }))
}
```

| 코드 | 설명 |
|---|---|
| `(Instant, V)` | 값과 **만료 시각**을 함께 저장. 읽을 때 만료를 검사한다(지연 삭제) |
| `RwLock` | 읽기가 압도적으로 많으므로 동시에 읽을 수 있게 |
| `AtomicU64` + `Relaxed` | 적중/미스 카운터. 락 없이 증가 |
| `invalidate` | 원본이 바뀌면 지운다. `POST /report/refresh`로 시험 |
| 응답의 `cache`, `elapsedMs` | **측정이 응답에 들어 있다.** 눈으로 효과를 확인 |

> 이 캐시는 원리를 보려고 직접 만들었다. 실무에서는 용량 제한·동시성·TTL이 내장된 `moka` 같은 크레이트나 Redis를 쓴다. **한계**: ① 용량 상한이 없다(키가 무한히 늘 수 있다) ② 만료된 항목을 능동적으로 지우지 않는다 ③ **캐시 스탬피드** — TTL이 끝나는 순간 동시 요청이 모두 miss가 되어 느린 쿼리를 **동시에 N번** 실행한다(해결: 한 요청만 계산하고 나머지는 기다리게).

실측:

```
GET /report            → {"cache":"miss", ..., "elapsedMs":311}      ← 느린 쿼리(300ms)를 실제로 수행
GET /report            → {"cache":"hit",  ..., "elapsedMs":0}        ← 메모리에서 바로
POST /report/refresh   → 204                                          ← 원본이 바뀌었다고 가정하고 무효화
GET /report            → {"cache":"miss", ..., "elapsedMs":310}      ← 다시 계산
GET /stats             → {"cacheHits":1,"cacheMisses":2,"hitRate":0.333}
```

**311ms → 0ms.** (적중률은 요청 패턴에 따라 달라진다. 읽기 100번에 쓰기 1번이면 적중률은 99%에 가깝다.)

### 3-2. 백그라운드 작업 — 큐 + 워커

```rust
// 용량이 있는 채널: 워커가 못 따라가면 send가 기다리게 되어 메모리가 무한히 늘지 않는다
let (tx, mut rx) = mpsc::channel::<(u32, String)>(100);

// 워커: 서버가 떠 있는 동안 큐에서 하나씩 꺼내 처리한다
tokio::spawn(async move {
    while let Some((id, payload)) = rx.recv().await {
        jobs.write().await.insert(id, JobStatus { state: "running", result: None });
        tokio::time::sleep(Duration::from_secs(2)).await;                 // 무거운 일을 흉내
        jobs.write().await.insert(id, JobStatus { state: "done", result: Some(format!("처리 완료: {payload}")) });
    }
});

async fn enqueue(State(state): State<AppState>, Json(body): Json<NewJob>) -> (StatusCode, Json<Value>) {
    let id = state.next_job_id.fetch_add(1, Ordering::Relaxed) as u32;
    state.jobs.write().await.insert(id, JobStatus { state: "queued", result: None });
    if state.queue.send((id, body.payload)).await.is_err() { /* 503 */ }
    (StatusCode::ACCEPTED, Json(json!({ "data": { "id": id, "statusUrl": format!("/jobs/{id}") } })))
}
```

| 코드 | 설명 |
|---|---|
| `mpsc::channel(100)` | **유한 버퍼.** 무한 큐는 처리가 밀리면 메모리를 다 쓴다 (역압력) |
| 워커 `tokio::spawn` | 서버 시작 시 한 번. 요청과 무관하게 계속 돈다 |
| `202 Accepted` + `statusUrl` | "접수했다. 여기서 진행 상황을 봐라" |
| 상태 `queued → running → done` | 클라이언트는 `GET /jobs/{id}`를 폴링 (또는 SSE/WebSocket, study17) |

실측:

```
POST /jobs {"payload":"report-1"}  → 202 {"data":{"id":1,"statusUrl":"/jobs/1"}}
GET  /jobs/1                        → {"state":"running","result":null}
(3초 뒤) GET /jobs/1                → {"state":"done","result":"처리 완료: report-1"}
```

> ⚠️ **한계**: 큐가 메모리에 있어서 **서버가 재시작하면 대기 중이던 작업이 사라진다.** 중요한 작업은 DB에 "할 일"을 먼저 저장(outbox)하고 워커가 처리·재시도하거나, 별도 큐(Redis 등)를 쓴다. 이 예제의 `jobs` 맵도 재시작하면 사라진다.

### 3-3. ⭐ 블로킹 격리 — 숫자로 확인

```rust
fn heavy_cpu(rounds: u32) -> String {        // SHA-256을 반복 — 비밀번호 해싱 같은 동기 CPU 작업
    let mut data = b"seed".to_vec();
    for _ in 0..rounds { data = Sha256::digest(&data).to_vec(); }
    hex::encode(&data)[..16].to_string()
}

// ✅ 별도 스레드 풀에서 계산. async 워커 스레드는 다른 요청을 계속 처리한다
async fn cpu_offloaded() -> Json<Value> {
    let digest = tokio::task::spawn_blocking(|| heavy_cpu(400_000)).await.unwrap();
    Json(json!({ "digest": digest }))
}

// ❌ async 안에서 그냥 계산. 이 동안 이 워커 스레드는 아무 요청도 처리하지 못한다
async fn cpu_blocking() -> Json<Value> {
    Json(json!({ "digest": heavy_cpu(400_000) }))
}
```

**실험**: 워커 스레드를 2개로 제한(`TOKIO_WORKER_THREADS=2`)하고 CPU 요청 4개를 동시에 보낸 뒤, 그 사이 가벼운 `/health` 응답 시간을 쟀다.

```
유휴 상태:                       /health 0.0012 s
/cpu/blocking  4개 동시 진행 중:   /health 1.2645 s    ← 워커 2개가 모두 막혀서 /health가 기다림
/cpu/offloaded 4개 동시 진행 중:   /health 0.0011 s    ← 거의 변화 없음
```

**1.26초 vs 0.001초.** 같은 CPU 일을 하는데 `spawn_blocking`을 쓰냐 마느냐로 **다른 요청의 지연이 천 배 이상** 달라졌다. (코어가 많은 PC에서 워커 스레드를 줄이지 않고 요청 수가 적으면 이 차이가 안 보인다 — 운영에서 부하가 몰릴 때 나타난다.)

이 저장소에서는 [demo/src/auth/password.rs](demo/src/auth/password.rs)의 Argon2 해싱·검증이 같은 이유로 `spawn_blocking` 안에 있다. 동시 로그인 몇 건에 서버가 통째로 멈추는 것을 막는다.

격리 대상: 비밀번호 해싱, 이미지 리사이즈, 큰 JSON 파싱, 압축/암호화, **동기 방식 라이브러리**(블로킹 DB 드라이버, 파일 시스템 동기 API), `std::thread::sleep`.

### 3-4. 측정 방법

| 도구 | 무엇을 | 사용 |
|---|---|---|
| **응답에 시간 싣기** | 요청 하나의 서버 처리 시간 | `x-response-time-ms` 헤더 ([demo/src/middleware.rs](demo/src/middleware.rs)), 응답의 `elapsedMs` |
| **`curl -w`** | 클라이언트 입장의 총 시간 | `curl -s -o /dev/null -w "%{time_total}\n" URL` |
| **부하 도구** | 동시 요청에서의 처리량·지연 분포 | `oha -n 1000 -c 50 URL`, `wrk -t4 -c50 -d10s URL` (설치 필요, 이 저장소에서 실행하지 않음) |
| **`tracing` 스팬** | 요청 안에서 어느 단계가 느린지 | `#[tracing::instrument]`, `TraceLayer`의 지연 로그 |
| **`EXPLAIN ANALYZE`** | DB 쿼리의 실제 계획과 시간 | study11 (`Seq Scan` → `Index Scan`) |
| **`tokio-console`** | 태스크가 오래 `poll`되거나 멈춘 것 | 블로킹 의심 시 |
| **릴리스 빌드로 측정** | 디버그 빌드는 10배 이상 느릴 수 있다 | `cargo run --release` — **성능 숫자는 반드시 릴리스로** |

> 이 단계의 숫자는 디버그 빌드다. 비율(311→0, 1.26→0.001)은 의미가 있지만, **절대값**(초당 처리량 등)은 릴리스 빌드로 다시 재야 한다.

### 3-5. 값싼 최적화 — 응답 압축 · 릴리스 빌드

| 최적화 | 효과 (이 저장소 실측) | 비용 |
|---|---|---|
| `CompressionLayer` (gzip) | 반복적인 JSON 응답 **4,246 bytes → 94 bytes** | CPU 약간 |
| `lto="thin"` + `codegen-units=1` + `strip` | 바이너리 19.6MB → 18.5MB, 빌드 58s → 89s | **빌드 시간** |
| `panic = "abort"` | (바이너리가 더 작아진다) **하지만 켜면 안 된다** — `CatchPanicLayer`가 무력화되어 패닉 하나에 프로세스가 죽는다 | 안정성 |

### 3-6. 비동기 병렬 — 서로 독립인 일은 동시에

```rust
// 순차: 300ms + 300ms = 600ms          // 병렬: max(300, 300) = 300ms
let (a, b) = tokio::join!(fetch_posts_count(&db), fetch_comments_count(&db));
```

서로 의존하지 않는 DB 조회·외부 API 호출은 `tokio::join!`(둘 다 기다림) 또는 `try_join!`(하나라도 실패하면 즉시 오류)로 겹쳐 실행한다. 단, DB **풀 크기**를 넘는 병렬은 연결을 기다리게 된다.

---

## ④ 🔗 연동 포인트

| 이 단계의 것 | 이어지는 곳 |
|---|---|
| **TTL 캐시** | 쓰기 핸들러(study6)가 `cache.invalidate("posts:list")`를 호출. 여러 서버로 늘리면 프로세스 메모리 대신 **Redis**로 (study16). HTTP 쪽 캐시는 study14의 `ETag`/`Cache-Control` |
| **작업 큐** | 메일 전송(study13), AI 요약 같은 느린 일(study18)을 요청에서 분리. 결과를 알리는 방법은 폴링 → SSE/WebSocket(study17) |
| **`spawn_blocking`** | 비밀번호 해싱(study7), 이미지 처리(study13의 업로드 후처리) |
| **측정 값** | `x-response-time-ms`/`requestId`를 로그로 → 모니터링(study16). 점검표에 "p95 응답 시간"을 추가 |
| **DB** | 캐시로 감춘 느린 쿼리는 근본 원인(인덱스)을 고쳐야 한다 → `EXPLAIN` (study11). 커서 페이지네이션(study14)도 성능 설계 |
| **속도 제한(429)** | 스펙 v1의 `RATE_LIMITED`. 이 저장소는 **미구현**. 도입한다면 tower 계층(IP/토큰별 토큰 버킷)으로 — 캐시·큐와 같은 "서버를 지키는" 도구 |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| 동시 요청이 몰리면 **`/health`까지** 느려지거나 멈춤 | 워커 스레드를 막는 **동기 계산/블로킹 호출** | `spawn_blocking`. 실측 1.26초 → 0.001초 |
| `std::thread::sleep`을 `async fn`에서 호출 | 그 워커 스레드가 통째로 잠듦 | `tokio::time::sleep(...).await` |
| 컴파일 에러 `future cannot be sent between threads safely` (`MutexGuard`) | `std::sync::Mutex` guard를 `.await` 너머로 들고 있음 | 락 범위를 `.await` 전에 끝내거나 `tokio::sync` 락 |
| 캐시를 켰더니 **수정이 반영되지 않음** | 무효화 누락 (TTL이 끝날 때까지 낡은 값) | 쓰기 핸들러에서 `invalidate`, 또는 TTL을 짧게 |
| TTL 만료 순간 DB 부하가 **순간적으로 폭주** | 캐시 스탬피드 (동시 요청이 모두 miss) | 한 요청만 계산하게(싱글 플라이트), TTL에 무작위 오차 |
| 메모리가 계속 늘어남 | 용량 상한 없는 캐시 / 무한 큐 | 용량 제한이 있는 캐시(`moka`), 유한 `mpsc::channel` |
| 서버 재시작 후 작업이 사라짐 | 큐·상태가 메모리 | DB/외부 큐에 영속화(outbox), 시작 시 미처리 작업 복구 |
| 작업이 한 번 실패하면 영영 안 됨 | 재시도 없음 | 시도 횟수·백오프·최종 실패 상태를 기록 |
| 디버그 빌드로 측정해서 "Rust가 느리다" | 디버그 빌드는 최적화가 꺼져 있다 | **`--release`로 측정** |
| 부하 테스트 클라이언트가 병목 | 같은 PC에서 부하 도구와 서버가 CPU를 나눠 씀 | 가능하면 분리, 최소한 코어 수를 의식 |
| `spawn_blocking` 안에서 `.await` 사용 불가 | 그 클로저는 동기 컨텍스트 | 필요한 비동기 값은 **밖에서 미리** 준비해 넘긴다 |
| 압축을 켰는데 응답이 안 줄어듦 | 이미 압축된 파일(이미지/zip)이거나 아주 작은 응답 | JSON/텍스트에만 효과가 크다. 작은 응답은 `min size` |

---

## ⑥ 직접 해보기

1. **TTL 효과 곡선**: `ttl`을 1초/10초/60초로 바꾸고, 초당 5회씩 30초 동안 `/report`를 호출해 `/stats`의 `hitRate`가 어떻게 달라지는지 표로 만든다. (정확성과 속도의 교환)
2. **스탬피드 재현**: `ttl`을 1초로 두고 20개의 `curl`을 **동시에** 보내 `slow_query`가 몇 번 실행되는지(로그에 카운터를 찍어) 센다. 그다음 "계산 중인 키는 기다리게" 하는 방어를 넣어 1번이 되는지 확인한다.
3. **릴리스로 다시 재기**: `cargo run --release --example s15_cache_jobs`로 `/cpu/blocking` 지연과 `x-response-time-ms`를 다시 재고, 디버그 빌드와 비교해 비율이 얼마나 달라지는지 기록한다.

---

## ⑦ ☐ 체크리스트

- ☐ "측정 → 병목 → 한 가지 수정 → 재측정" 순서를 지킨다
- ☐ cache-aside 패턴과 TTL, **무효화**가 왜 어려운지 설명할 수 있다
- ☐ 캐시의 한계 세 가지(용량, 만료 정리, 스탬피드)를 말할 수 있다
- ☐ `202 Accepted` + 상태 조회 URL로 오래 걸리는 일을 분리했다
- ☐ 큐를 **유한**하게 만드는 이유(역압력)와 메모리 큐의 한계(재시작 시 유실)를 안다
- ☐ 블로킹 호출이 워커 스레드를 막아 **다른 요청까지 느리게** 만드는 것을 숫자로 확인했다
- ☐ 어떤 작업을 `spawn_blocking`으로 보내야 하는지 목록을 댈 수 있다
- ☐ 성능 숫자는 **릴리스 빌드**로 재야 한다는 것을 안다
- ☐ 서로 독립인 호출을 `tokio::join!`로 겹치는 방법을 안다

---

## ⑧ 다음 단계 연결

빠른 서버를 만들었으니 [study16](study16.md)에서는 그것을 **계속 안전하게 굴리는** 법을 다룬다: Docker/compose 구성, GitHub Actions로 테스트와 이미지 빌드 자동화, 로그·모니터링, 백업을 체크리스트 중심으로 정리한다.
