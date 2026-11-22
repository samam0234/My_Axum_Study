//! study15 — 성능: 캐시 · 백그라운드 작업 · 블로킹 격리 · 측정.
//!
//! 실행: `cargo run --example s15_cache_jobs`  →  http://127.0.0.1:3015
//!
//! 1. TTL 캐시:      느린 집계 쿼리(300ms)를 10초 동안 재사용 → 히트/미스와 소요 시간 확인
//! 2. 백그라운드 작업: 요청은 즉시 202로 끝내고, 무거운 일은 워커가 처리 → 상태 조회
//! 3. 블로킹 격리:    CPU를 오래 쓰는 일은 `spawn_blocking`으로 → 다른 요청이 안 막힌다
//! 4. 측정:          응답에 소요 시간을 싣고, `/stats`로 캐시 적중률을 본다

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{RwLock, mpsc};

// ── 1. TTL 캐시 ──────────────────────────────────────────────────────────────

/// 값과 만료 시각을 함께 들고 있는 단순 캐시.
/// 실무에서는 `moka`(동시성·용량 제한·TTL 내장) 또는 Redis를 쓴다. 원리를 보려고 직접 만든다.
struct TtlCache<V> {
    map: RwLock<HashMap<String, (Instant, V)>>,
    ttl: Duration,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<V: Clone> TtlCache<V> {
    fn new(ttl: Duration) -> Self {
        Self { map: RwLock::new(HashMap::new()), ttl, hits: 0.into(), misses: 0.into() }
    }

    async fn get(&self, key: &str) -> Option<V> {
        let map = self.map.read().await;
        match map.get(key) {
            Some((expires, value)) if *expires > Instant::now() => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(value.clone())
            }
            _ => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    async fn put(&self, key: &str, value: V) {
        self.map.write().await.insert(key.to_string(), (Instant::now() + self.ttl, value));
    }

    /// 쓰기가 일어나면 관련 캐시를 지운다. "캐시 무효화는 컴퓨터 과학에서 가장 어려운 두 가지 중 하나"다.
    async fn invalidate(&self, key: &str) {
        self.map.write().await.remove(key);
    }
}

// ── 2. 작업 큐 ───────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobStatus {
    state: &'static str, // queued | running | done
    result: Option<String>,
}

#[derive(Clone)]
struct AppState {
    cache: Arc<TtlCache<Value>>,
    jobs: Arc<RwLock<HashMap<u32, JobStatus>>>,
    queue: mpsc::Sender<(u32, String)>,
    next_job_id: Arc<AtomicU64>,
}

#[tokio::main]
async fn main() {
    // 용량이 있는 채널: 워커가 못 따라가면 `send`가 기다리게 되어 **메모리가 무한히 늘지 않는다.**
    let (tx, mut rx) = mpsc::channel::<(u32, String)>(100);

    let state = AppState {
        cache: Arc::new(TtlCache::new(Duration::from_secs(10))),
        jobs: Default::default(),
        queue: tx,
        next_job_id: Arc::new(AtomicU64::new(1)),
    };

    // 워커: 서버가 떠 있는 동안 큐에서 하나씩 꺼내 처리한다.
    let jobs = state.jobs.clone();
    tokio::spawn(async move {
        while let Some((id, payload)) = rx.recv().await {
            jobs.write().await.insert(id, JobStatus { state: "running", result: None });
            tokio::time::sleep(Duration::from_secs(2)).await; // 무거운 일(리포트 생성, 메일 대량 발송 등)을 흉내
            jobs.write().await.insert(id, JobStatus { state: "done", result: Some(format!("처리 완료: {payload}")) });
        }
    });

    let app = Router::new()
        .route("/report", get(report))
        .route("/report/refresh", post(refresh_report))
        .route("/jobs", post(enqueue))
        .route("/jobs/{id}", get(job_status))
        .route("/cpu/offloaded", get(cpu_offloaded))
        .route("/cpu/blocking", get(cpu_blocking))
        .route("/health", get(|| async { "ok" }))
        .route("/stats", get(stats))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3015").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// ── 1. 캐시 적용 ─────────────────────────────────────────────────────────────

/// 느린 집계. (실제로는 `SELECT ... GROUP BY ...`가 수백 ms 걸리는 상황)
async fn slow_query() -> Value {
    tokio::time::sleep(Duration::from_millis(300)).await;
    json!({ "posts": 1234, "comments": 5678 })
}

async fn report(State(state): State<AppState>) -> Json<Value> {
    let started = Instant::now();

    // 캐시 조회 → 없으면 계산 → 저장. ("cache-aside" 패턴)
    let (value, cache) = match state.cache.get("report").await {
        Some(v) => (v, "hit"),
        None => {
            let v = slow_query().await;
            state.cache.put("report", v.clone()).await;
            (v, "miss")
        }
    };

    Json(json!({ "data": value, "cache": cache, "elapsedMs": started.elapsed().as_millis() }))
}

/// 원본이 바뀌었다고 가정하고 캐시를 지운다.
async fn refresh_report(State(state): State<AppState>) -> StatusCode {
    state.cache.invalidate("report").await;
    StatusCode::NO_CONTENT
}

// ── 2. 백그라운드 작업 ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NewJob {
    payload: String,
}

async fn enqueue(State(state): State<AppState>, Json(body): Json<NewJob>) -> (StatusCode, Json<Value>) {
    let id = state.next_job_id.fetch_add(1, Ordering::Relaxed) as u32;
    state.jobs.write().await.insert(id, JobStatus { state: "queued", result: None });

    if state.queue.send((id, body.payload)).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": { "code": "INTERNAL", "message": "워커가 멈췄습니다." } })));
    }
    // 202 Accepted + 상태를 볼 수 있는 위치. 클라이언트는 이 주소를 폴링한다.
    (StatusCode::ACCEPTED, Json(json!({ "data": { "id": id, "statusUrl": format!("/jobs/{id}") } })))
}

async fn job_status(State(state): State<AppState>, Path(id): Path<u32>) -> Result<Json<JobStatus>, StatusCode> {
    state.jobs.read().await.get(&id).cloned().map(Json).ok_or(StatusCode::NOT_FOUND)
}

// ── 3. 블로킹 격리 ───────────────────────────────────────────────────────────

/// CPU를 오래 쓰는 동기 계산. (비밀번호 해싱·이미지 변환·압축이 이런 종류다)
fn heavy_cpu(rounds: u32) -> String {
    let mut data = b"seed".to_vec();
    for _ in 0..rounds {
        data = Sha256::digest(&data).to_vec();
    }
    hex::encode(&data)[..16].to_string()
}

/// ✅ 별도 스레드 풀(`spawn_blocking`)에서 계산한다. async 워커 스레드는 계속 다른 요청을 처리한다.
async fn cpu_offloaded() -> Json<Value> {
    let started = Instant::now();
    let digest = tokio::task::spawn_blocking(|| heavy_cpu(400_000)).await.unwrap();
    Json(json!({ "digest": digest, "elapsedMs": started.elapsed().as_millis() }))
}

/// ❌ async 함수 안에서 그냥 계산한다. 이 동안 이 워커 스레드는 **아무 요청도 처리하지 못한다.**
/// 워커 스레드 수(= CPU 코어 수)만큼 동시에 호출되면 `/health`조차 응답하지 못한다.
async fn cpu_blocking() -> Json<Value> {
    let started = Instant::now();
    let digest = heavy_cpu(400_000);
    Json(json!({ "digest": digest, "elapsedMs": started.elapsed().as_millis() }))
}

// ── 4. 측정 ──────────────────────────────────────────────────────────────────

async fn stats(State(state): State<AppState>) -> Json<Value> {
    let hits = state.cache.hits.load(Ordering::Relaxed);
    let misses = state.cache.misses.load(Ordering::Relaxed);
    let total = hits + misses;
    Json(json!({
        "cacheHits": hits,
        "cacheMisses": misses,
        "hitRate": if total == 0 { 0.0 } else { hits as f64 / total as f64 },
    }))
}
