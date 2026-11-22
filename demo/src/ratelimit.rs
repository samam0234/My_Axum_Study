//! 호출 제한 (429 `RATE_LIMITED`). **공통 API 스펙 v1 §5**
//!
//! 고정 창(fixed window) 방식: IP마다 "창 시작 시각 + 횟수"만 기억한다.
//! 창이 지나면 횟수를 0으로 되돌린다. 단순하고 메모리를 거의 안 쓰지만,
//! 창 경계에서 최대 2배까지 몰릴 수 있다. (더 정밀한 토큰 버킷은 `tower_governor`가 해 준다)
//!
//! ⚠️ 프로세스 메모리에만 있어서 **서버가 여러 대면 대수만큼 허용량이 늘어난다.**
//!    그때는 Redis 같은 공유 저장소로 옮긴다. (study15)

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::AppError;
use crate::state::AppState;

/// 한 번에 기억하는 IP 수의 상한. 넘으면 만료된 항목부터 정리한다. (메모리 고갈 방지)
const MAX_TRACKED_KEYS: usize = 10_000;

#[derive(Debug)]
pub struct RateLimiter {
    /// 창당 허용 횟수. `0`이면 제한하지 않는다.
    limit: u32,
    window: Duration,
    hits: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// 제한 없음. (기본값, 테스트)
    pub fn disabled() -> Self {
        Self::new(0, Duration::from_secs(60))
    }

    /// 허용되면 `Ok`, 초과면 `Err(다시 시도할 때까지 남은 초)`.
    pub fn check(&self, key: &str) -> Result<(), u64> {
        if self.limit == 0 {
            return Ok(());
        }

        let now = Instant::now();
        // 락을 잡은 채로 await하지 않으므로 std Mutex로 충분하다.
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());

        if hits.len() >= MAX_TRACKED_KEYS {
            hits.retain(|_, (started, _)| now.duration_since(*started) < self.window);
        }

        let entry = hits.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0); // 새 창
        }

        if entry.1 >= self.limit {
            let retry_after = self.window.saturating_sub(now.duration_since(entry.0));
            return Err(retry_after.as_secs().max(1));
        }

        entry.1 += 1;
        Ok(())
    }
}

/// `/api` 아래 모든 요청에 거는 미들웨어. `from_fn_with_state`로 붙인다.
///
/// 키는 **연결의 원격 IP**다. `X-Forwarded-For`는 클라이언트가 마음대로 적을 수 있어서
/// 신뢰할 수 있는 프록시 뒤가 아니면 쓰지 않는다. (프록시 뒤라면 프록시가 넣는 헤더를 읽도록 바꾼다)
pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let key = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip().to_string())
        // 서버를 띄우지 않는 테스트(oneshot)에는 연결 정보가 없다.
        .unwrap_or_else(|| "local".to_string());

    match state.rate_limiter.check(&key) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => AppError::RateLimited { retry_after }.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 한도를_넘으면_거부하고_남은_초를_알려준다() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("a").is_ok());
        let retry = limiter.check("a").unwrap_err();
        assert!((1..=60).contains(&retry));
    }

    #[test]
    fn ip마다_따로_센다() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("b").is_ok());
        assert!(limiter.check("a").is_err());
    }

    #[test]
    fn 창이_지나면_다시_허용한다() {
        let limiter = RateLimiter::new(1, Duration::from_millis(20));
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("a").is_err());
        std::thread::sleep(Duration::from_millis(30));
        assert!(limiter.check("a").is_ok());
    }

    #[test]
    fn 한도_0은_제한_없음이다() {
        let limiter = RateLimiter::disabled();
        for _ in 0..1000 {
            assert!(limiter.check("a").is_ok());
        }
    }
}
