//! 生产保护组件：限流器（per-API-key）+ 熔断器（上游整体）+ Prometheus 指标
//!
//! 公网部署后必需：防滥用（限流）、防上游雪崩（熔断）、可观测（metrics）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

// ── RateLimiter：每 API Key 固定窗口限流 ────────────────────────────

#[derive(Debug, Clone)]
struct RateWindow {
    start: Instant,
    count: u64,
}

#[derive(Debug)]
pub struct RateLimiter {
    enabled: bool,
    requests: u64,
    /// 每个时间窗口内的计数阈值；count 达到该值后拒绝
    window: Duration,
    /// 限流 map 最大条目数（防唯一 key 制造无界内存）
    max_keys: usize,
    inner: Mutex<HashMap<String, RateWindow>>,
    /// 每 N 次 check 触发一次全量过期清理（防无界增长）
    sweep_counter: AtomicU64,
}

impl RateLimiter {
    pub fn new(enabled: bool, requests: u64, window_sec: u64) -> Self {
        Self::with_max_keys(enabled, requests, window_sec, 4096)
    }

    pub fn with_max_keys(enabled: bool, requests: u64, window_sec: u64, max_keys: usize) -> Self {
        Self {
            enabled,
            requests: requests.max(1),
            window: Duration::from_secs(window_sec.max(1)),
            max_keys: max_keys.max(1),
            inner: Mutex::new(HashMap::new()),
            sweep_counter: AtomicU64::new(0),
        }
    }

    /// 检查并计数：超限返回 Err(重试秒数)
    pub fn check(&self, key: &str) -> Result<(), u64> {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> Result<(), u64> {
        if !self.enabled {
            return Ok(());
        }
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // 周期性全量清理过期条目
        let n = self.sweep_counter.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(256) {
            map.retain(|_, w| now < w.start + self.window);
        }
        // 限流 map 有界：超过 max_keys 时先清过期；仍满则拒绝新 key（保留既有 key 计数）
        if !map.contains_key(key) && !map.is_empty() && map.len() >= self.max_keys {
            return Err(1);
        }
        let window_end = match map.get(key) {
            Some(w) if now < w.start + self.window => w.start + self.window,
            _ => {
                map.insert(
                    key.to_string(),
                    RateWindow {
                        start: now,
                        count: 0,
                    },
                );
                now + self.window
            }
        };
        // 尾部判定：达到阈值后不再计数（避免第 N+1 次后继续递增制造假峰值）
        let entry = map.get_mut(key).unwrap();
        if entry.count >= self.requests {
            let retry = window_end.saturating_duration_since(now).as_secs().max(1);
            return Err(retry);
        }
        entry.count += 1;
        Ok(())
    }

    /// 当前记录条数（测试/观测）
    pub fn len(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// 是否为空（clippy len_without_is_empty）
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ── CircuitBreaker：上游整体熔断状态机 ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CbState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug)]
struct CbInner {
    state: CbState,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    enabled: bool,
    threshold: u32,
    timeout: Duration,
    inner: RwLock<CbInner>,
}

impl CircuitBreaker {
    pub fn new(enabled: bool, threshold: u32, timeout_sec: u64) -> Self {
        Self {
            enabled,
            threshold: threshold.max(1),
            timeout: Duration::from_secs(timeout_sec.max(1)),
            inner: RwLock::new(CbInner {
                state: CbState::Closed,
                consecutive_failures: 0,
                opened_at: None,
            }),
        }
    }

    /// 请求是否允许发往上游
    pub fn allow(&self) -> bool {
        self.allow_at(Instant::now())
    }

    fn allow_at(&self, now: Instant) -> bool {
        if !self.enabled {
            return true;
        }
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        match inner.state {
            CbState::Closed => true,
            CbState::Open => {
                if let Some(opened) = inner.opened_at {
                    if now.saturating_duration_since(opened) >= self.timeout {
                        // 进入半开，放行一个探测请求
                        inner.state = CbState::HalfOpen;
                        return true;
                    }
                }
                false
            }
            CbState::HalfOpen => false, // 探测请求在途，其它拒绝
        }
    }

    pub fn record_success(&self) {
        if !self.enabled {
            return;
        }
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        match inner.state {
            CbState::Closed => {
                inner.consecutive_failures = 0;
            }
            CbState::HalfOpen => {
                inner.state = CbState::Closed;
                inner.consecutive_failures = 0;
                inner.opened_at = None;
            }
            CbState::Open => {}
        }
    }

    pub fn record_failure(&self) {
        if !self.enabled {
            return;
        }
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        match inner.state {
            CbState::Closed => {
                inner.consecutive_failures += 1;
                if inner.consecutive_failures >= self.threshold {
                    inner.state = CbState::Open;
                    inner.opened_at = Some(Instant::now());
                }
            }
            CbState::HalfOpen => {
                // 探测失败 → 重新 OPEN
                inner.state = CbState::Open;
                inner.opened_at = Some(Instant::now());
            }
            CbState::Open => {}
        }
    }

    pub fn state(&self) -> CbState {
        self.inner
            .read()
            .map(|i| i.state)
            .unwrap_or(CbState::Closed)
    }
}

// ── Metrics：Prometheus 文本指标 ───────────────────────────────────

#[derive(Debug, Default)]
pub struct Metrics {
    /// key: "endpoint|provider|status_class" -> count
    requests: Mutex<HashMap<String, u64>>,
    /// key: error type -> count
    upstream_errors: Mutex<HashMap<String, u64>>,
    duration_micros: AtomicU64,
    duration_count: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_request(&self, endpoint: &str, provider: &str, status_class: &str) {
        let key = format!("{endpoint}|{provider}|{status_class}");
        let mut m = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        *m.entry(key).or_insert(0) += 1;
    }

    pub fn record_upstream_error(&self, kind: &str) {
        let mut m = self
            .upstream_errors
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *m.entry(kind.to_string()).or_insert(0) += 1;
    }

    pub fn observe_duration(&self, secs: f64) {
        self.duration_micros
            .fetch_add((secs * 1_000_000.0) as u64, Ordering::Relaxed);
        self.duration_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn render(
        &self,
        proxy_pool_size: usize,
        proxy_pool_available: usize,
        active_sessions: usize,
    ) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        // requests
        let _ = writeln!(
            out,
            "# HELP tryingopen_requests_total 请求计数（endpoint/provider/status_class）"
        );
        let _ = writeln!(out, "# TYPE tryingopen_requests_total counter");
        let mut reqs: Vec<_> = self
            .requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .into_iter()
            .collect();
        reqs.sort();
        for (key, v) in reqs {
            let parts: Vec<&str> = key.split('|').collect();
            if parts.len() == 3 {
                let _ = writeln!(out, "tryingopen_requests_total{{endpoint=\"{}\",provider=\"{}\",status=\"{}\"}} {v}", parts[0], parts[1], parts[2]);
            }
        }
        // duration
        let _ = writeln!(
            out,
            "# HELP tryingopen_request_duration_seconds 请求耗时累计（秒）"
        );
        let _ = writeln!(out, "# TYPE tryingopen_request_duration_seconds summary");
        let micros = self.duration_micros.load(Ordering::Relaxed);
        let count = self.duration_count.load(Ordering::Relaxed);
        let _ = writeln!(
            out,
            "tryingopen_request_duration_seconds_sum {:.6}",
            micros as f64 / 1_000_000.0
        );
        let _ = writeln!(out, "tryingopen_request_duration_seconds_count {count}");
        // upstream errors
        let _ = writeln!(
            out,
            "# HELP tryingopen_upstream_errors_total 上游错误计数（按类型）"
        );
        let _ = writeln!(out, "# TYPE tryingopen_upstream_errors_total counter");
        let mut errs: Vec<_> = self
            .upstream_errors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .into_iter()
            .collect();
        errs.sort();
        for (k, v) in errs {
            let _ = writeln!(out, "tryingopen_upstream_errors_total{{type=\"{k}\"}} {v}");
        }
        // gauges
        let _ = writeln!(out, "# HELP tryingopen_proxy_pool_size 代理池大小");
        let _ = writeln!(out, "# TYPE tryingopen_proxy_pool_size gauge");
        let _ = writeln!(out, "tryingopen_proxy_pool_size {proxy_pool_size}");
        let _ = writeln!(out, "# HELP tryingopen_proxy_pool_available 可用代理数");
        let _ = writeln!(out, "# TYPE tryingopen_proxy_pool_available gauge");
        let _ = writeln!(
            out,
            "tryingopen_proxy_pool_available {proxy_pool_available}"
        );
        let _ = writeln!(out, "# HELP tryingopen_active_sessions 活跃会话数");
        let _ = writeln!(out, "# TYPE tryingopen_active_sessions gauge");
        let _ = writeln!(out, "tryingopen_active_sessions {active_sessions}");
        out
    }
}

pub type SharedRateLimiter = Arc<RateLimiter>;
pub type SharedCircuitBreaker = Arc<CircuitBreaker>;
pub type SharedMetrics = Arc<Metrics>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rate_limiter_allows_within_window() {
        let rl = RateLimiter::new(true, 3, 3600);
        let t0 = Instant::now();
        assert!(rl.check_at("k", t0).is_ok());
        assert!(rl.check_at("k", t0 + Duration::from_secs(1)).is_ok());
        assert!(rl.check_at("k", t0 + Duration::from_secs(2)).is_ok());
        // 第 4 次（超过阈值 3）→ 429
        assert!(rl.check_at("k", t0 + Duration::from_secs(3)).is_err());
        assert_eq!(rl.len(), 1);
    }

    #[test]
    fn rate_limiter_retry_after_positive() {
        let rl = RateLimiter::new(true, 1, 60);
        let t0 = Instant::now();
        assert!(rl.check_at("k", t0).is_ok());
        let err = rl.check_at("k", t0 + Duration::from_secs(5));
        assert!(err.is_err());
        let retry = err.unwrap_err();
        assert!(retry >= 1, "retry_after={retry}");
        assert!(retry <= 60);
    }

    #[test]
    fn rate_limiter_disabled_passes() {
        let rl = RateLimiter::new(false, 1, 60);
        let t0 = Instant::now();
        for i in 0..100u64 {
            assert!(rl.check_at("k", t0 + Duration::from_secs(i)).is_ok());
        }
    }

    #[test]
    fn rate_limiter_window_expiry_resets() {
        let rl = RateLimiter::new(true, 2, 10);
        let t0 = Instant::now();
        assert!(rl.check_at("k", t0).is_ok());
        assert!(rl.check_at("k", t0 + Duration::from_secs(1)).is_ok());
        assert!(rl.check_at("k", t0 + Duration::from_secs(2)).is_err());
        // 窗口过期后重新计数
        assert!(rl.check_at("k", t0 + Duration::from_secs(11)).is_ok());
    }

    #[test]
    fn circuit_breaker_closed_open_halfopen_closed() {
        let cb = CircuitBreaker::new(true, 2, 30);
        assert_eq!(cb.state(), CbState::Closed);
        assert!(cb.allow());
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CbState::Open);
        assert!(!cb.allow(), "OPEN 时拒绝");
        // 未到 timeout 仍拒绝
        assert!(!cb.allow());
        // 超过 timeout → HALF_OPEN 放行一个探测
        let t = Instant::now() + Duration::from_secs(31);
        assert!(cb.allow_at(t), "HALF_OPEN 探测放行");
        // HALF_OPEN 时其它请求拒绝
        assert!(!cb.allow_at(t + Duration::from_secs(1)));
        // 探测成功 → CLOSED
        cb.record_success();
        assert_eq!(cb.state(), CbState::Closed);
        assert!(cb.allow());
    }

    #[test]
    fn circuit_breaker_halfopen_failure_reopens() {
        let cb = CircuitBreaker::new(true, 1, 30);
        cb.record_failure();
        assert_eq!(cb.state(), CbState::Open);
        let t = Instant::now() + Duration::from_secs(31);
        assert!(cb.allow_at(t));
        cb.record_failure();
        assert_eq!(cb.state(), CbState::Open);
    }

    #[test]
    fn circuit_breaker_disabled_passes() {
        let cb = CircuitBreaker::new(false, 1, 30);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow());
        assert_eq!(cb.state(), CbState::Closed);
    }

    #[test]
    fn rate_limiter_max_keys_bounded() {
        let rl = RateLimiter::new(true, 1, 3600);
        assert!(rl.len() <= 4096);
        // 大量唯一 key 不超过上限，且超限后新 key 被拒
        for i in 0..4500u32 {
            let _ = rl.check(&format!("key-{i}"));
        }
        assert_eq!(rl.len(), 4096);
    }

    #[test]
    fn rate_limiter_tail_semantics() {
        let rl = RateLimiter::new(true, 2, 60);
        let t0 = Instant::now();
        assert!(rl.check_at("k", t0).is_ok());
        assert!(rl.check_at("k", t0 + Duration::from_secs(1)).is_ok());
        // 第 3 次起拒绝，且 count 不再增长（窗口剩余不会被耗尽）
        assert!(rl.check_at("k", t0 + Duration::from_secs(2)).is_err());
        assert!(rl.check_at("k", t0 + Duration::from_secs(3)).is_err());
    }

    #[test]
    fn metrics_render_contains_key_series() {
        let m = Metrics::new();
        m.record_request("v1_chat_completions", "openai", "2xx");
        m.record_request("v1_chat_completions", "openai", "429");
        m.record_upstream_error("timeout");
        m.observe_duration(1.25);
        let out = m.render(42, 17, 3);
        assert!(out.contains("tryingopen_requests_total{endpoint=\"v1_chat_completions\",provider=\"openai\",status=\"2xx\"} 1"));
        assert!(out.contains("tryingopen_upstream_errors_total{type=\"timeout\"} 1"));
        assert!(out.contains("tryingopen_proxy_pool_size 42"));
        assert!(out.contains("tryingopen_proxy_pool_available 17"));
        assert!(out.contains("tryingopen_active_sessions 3"));
        assert!(out.contains("tryingopen_request_duration_seconds_sum 1.250000"));
        assert!(out.contains("tryingopen_request_duration_seconds_count 1"));
    }
}
