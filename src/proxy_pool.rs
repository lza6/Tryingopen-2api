//! 代理池：住宅代理文件 + 免费代理抓取 双源，按 IP 冷却/故障轮换/健康分路由。
//!
//! 适配 tryingopen 上游「单 IP 限流 20 次/h → 代理池自动故障轮换」：
//! - 优先选「24h 窗口内从未用过」且 latency 最低 / 健康分最高的出口 IP；
//! - 每出口 inflight 计数：并发高峰优先选 inflight=0 的出口，避免打爆同一出口；
//! - 全局并发上限（max_concurrent_requests）用 tokio Semaphore 门控；
//! - 429/网络错误 → 冷却该出口并按指数退避换下一个；
//! - 健康分（EWMA）低的降序排底，不硬剔除（给恢复机会）；
//! - 全部可用出口用过一轮后选冷却最早结束的；
//! - 无代理/全部冷却时返回 None，由上层直连兜底（本机 IP 也有每 24h UTC 日配额）。
//!
//! 数据面只暴露 host:port（住宅代理 user:pass 凭据脱敏）。

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, Semaphore};

const DAY: u64 = 24 * 3600;
/// 默认全局并发上限（无 config 时的兜底值）
const DEFAULT_MAX_CONCURRENT: usize = 64;
/// 默认并发预检/注入窗口（测试/兜底）
const DEFAULT_PRECHECK_CONCURRENCY: usize = 50;
/// 单出口并发软上限：inflight 超过该值不再新分配同一出口
const PER_PROXY_MAX_INFLIGHT: u32 = 2;

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxySnapshot {
    pub host_port: String,
    pub source: String,
    pub daily_uses: u32,
    pub cooling: bool,
    pub cooldown_seconds: i64,
    pub fails: u32,
    pub health_score: f64,
    /// 最近一次 HTTP 延迟测量（毫秒；0=未知）
    pub latency_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProxyEntry {
    pub url: String,
    pub source: String,
    pub added_at: f64,
    pub last_used_at: f64,
    pub daily_uses: u32,
    pub day_key: i64,
    pub cooldown_until: f64,
    pub consecutive_fails: u32,
    pub use_count: u32,
    pub health_score: f64,
    pub last_success_ts: f64,
    /// HTTP 延迟测量（毫秒；0=未知/未测）
    pub latency_ms: u64,
}

impl ProxyEntry {
    fn new(url: String, source: &str) -> Self {
        Self {
            url,
            source: source.to_string(),
            added_at: now(),
            last_used_at: 0.0,
            daily_uses: 0,
            day_key: (now() / DAY as f64) as i64,
            cooldown_until: 0.0,
            consecutive_fails: 0,
            use_count: 0,
            health_score: 1.0,
            last_success_ts: 0.0,
            latency_ms: 0,
        }
    }

    fn available(&self, t: f64, hourly_per_ip: usize) -> bool {
        if t < self.cooldown_until {
            return false;
        }
        let day = (t / DAY as f64) as i64;
        if day != self.day_key {
            return true; // 新的一天重置
        }
        if hourly_per_ip > 0 && self.use_count >= hourly_per_ip as u32 {
            return false;
        }
        true
    }

    fn snapshot(&self) -> ProxySnapshot {
        let t = now();
        let host_port = safe_host_port(&self.url);
        let c = if t < self.cooldown_until {
            self.cooldown_until - t
        } else {
            0.0
        };
        ProxySnapshot {
            host_port,
            source: self.source.clone(),
            daily_uses: if (t / DAY as f64) as i64 == self.day_key {
                self.daily_uses
            } else {
                0
            },
            cooling: t < self.cooldown_until,
            cooldown_seconds: c as i64,
            fails: self.consecutive_fails,
            health_score: (self.health_score * 1000.0).round() / 1000.0,
            latency_ms: self.latency_ms,
        }
    }
}

/// 脱敏：只暴露 host:port，不泄漏 user:pass
pub fn safe_host_port(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let rest = if let Some(at) = rest.rfind('@') {
        &rest[at + 1..]
    } else {
        rest
    };
    rest.to_string()
}

/// 统一代理 URL 前缀：解析出的纯 ip:port → http:// 前缀；已有 scheme 保留
pub fn normalize_proxy_url(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    }
}

/// 校验代理 URL 的 host 是否为公网可路由地址（防住宅代理文件被污染 → SSRF/内网注入）
/// 返回 (是否合法, 规范化 URL)
pub fn sanitize_proxy_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('#') {
        return None;
    }
    let norm = normalize_proxy_url(raw);
    let rest = norm.split("://").nth(1).unwrap_or(&norm);
    let host = if let Some(at) = rest.rfind('@') {
        &rest[at + 1..]
    } else {
        rest
    };
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_end_matches('/');
    // 拒绝非 IP 的 hostname（住宅文件也要求 IP:port）
    if host.parse::<std::net::IpAddr>().is_err() {
        return None;
    }
    let ip: std::net::IpAddr = host.parse().ok()?;
    let bad = match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_multicast() || v6.is_unspecified(),
    };
    if bad {
        return None;
    }
    Some(norm)
}

/// host:port 去重键（忽略 scheme 与 user:pass）
pub fn host_port_key(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let rest = if let Some(at) = rest.rfind('@') {
        &rest[at + 1..]
    } else {
        rest
    };
    // 去掉可能多余的路径/斜杠
    let rest = rest.trim_end_matches('/');
    rest.to_string()
}

/// 递增冷却：第 N 次使用后等待 map[N-1] 秒（超出取最后值）
pub fn cooldown_seconds(count: u32, map: &[u32]) -> u64 {
    if map.is_empty() {
        return 30;
    }
    let idx = (count as usize).saturating_sub(1);
    if idx < map.len() {
        map[idx] as u64
    } else {
        *map.last().unwrap() as u64
    }
}

pub fn parse_cooldown_map(s: &str) -> Vec<u32> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<u32>().ok())
        .collect()
}

#[derive(Debug, Default)]
struct PoolData {
    entries: Vec<ProxyEntry>,
    sticky: HashMap<String, (String, f64)>,
    /// host:port → 当前 inflight 请求数
    inflight: HashMap<String, u32>,
    /// host:port → 全局并发 gate 许可（请求完成时释放）
    permits: HashMap<String, Vec<Arc<tokio::sync::OwnedSemaphorePermit>>>,
    /// host:port 去重索引
    keys: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct ProxyPool {
    inner: Arc<RwLock<PoolData>>,
    /// 全局并发上限门控
    gate: Arc<RwLock<Arc<Semaphore>>>,
}

impl Default for ProxyPool {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyPool {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PoolData::default())),
            gate: Arc::new(RwLock::new(Arc::new(Semaphore::new(
                DEFAULT_MAX_CONCURRENT,
            )))),
        }
    }

    /// 设置全局并发上限（main.rs 启动时调用；不传则保留默认 64）
    pub async fn set_limits(&self, max_concurrent: Option<usize>) {
        let n = max_concurrent.unwrap_or(DEFAULT_MAX_CONCURRENT).max(1);
        let mut g = self.gate.write().await;
        *g = Arc::new(Semaphore::new(n));
    }

    /// 载入住宅/自备代理文件（每行一个 url，支持 # 注释）
    pub async fn load_file(&self, path: &str) -> usize {
        let mut added = 0;
        match tokio::fs::read_to_string(path).await {
            Ok(text) => {
                let mut data = self.inner.write().await;
                let mut fresh: Vec<String> = Vec::new();
                for line in text.lines() {
                    let u = line.trim();
                    if u.is_empty() || u.starts_with('#') {
                        continue;
                    }
                    // 住宅代理文件也做公网地址校验（防污染 → SSRF/内网注入）
                    let Some(norm) = sanitize_proxy_url(u) else {
                        continue;
                    };
                    let key = host_port_key(&norm);
                    if data.keys.contains(&key) || fresh.iter().any(|f| host_port_key(f) == key) {
                        continue;
                    }
                    fresh.push(norm);
                }
                for norm in fresh {
                    let key = host_port_key(&norm);
                    data.keys.insert(key);
                    data.entries.push(ProxyEntry::new(norm, "residential"));
                    added += 1;
                }
            }
            Err(e) => tracing::warn!("代理文件不可读 {}: {e}", path),
        }
        added
    }

    /// 批量注入免费代理（按 host:port 高效去重，忽略 user:pass/scheme 差异）
    pub async fn add_free(&self, urls: Vec<String>) -> usize {
        self.add_free_with_latency(urls.into_iter().map(|u| (u, 0)).collect())
            .await
    }

    /// 批量注入免费代理并附带延迟测量（并发预检产物）
    pub async fn add_free_with_latency(&self, urls: Vec<(String, u64)>) -> usize {
        if urls.is_empty() {
            return 0;
        }
        let mut added = 0;
        let mut data = self.inner.write().await;
        let mut fresh: Vec<(String, u64)> = Vec::new();
        let mut fresh_keys: HashSet<String> = HashSet::new();
        for (u, latency) in urls {
            // 防御纵深：免费代理同样做公网地址校验
            let Some(norm) = sanitize_proxy_url(&u) else {
                continue;
            };
            let key = host_port_key(&norm);
            if data.keys.contains(&key) || !fresh_keys.insert(key) {
                continue;
            }
            fresh.push((norm, latency));
        }
        for (u, latency) in fresh {
            let key = host_port_key(&u);
            data.keys.insert(key);
            let mut e = ProxyEntry::new(u, "free");
            e.latency_ms = latency;
            data.entries.push(e);
            added += 1;
        }
        added
    }

    /// 剔除「注入超 3h 且最近 30 分钟未用」的免费代理
    pub async fn reap_free(&self) -> usize {
        let t = now();
        let mut data = self.inner.write().await;
        let before = data.entries.len();
        data.entries.retain(|e| {
            !(e.source == "free" && t - e.added_at > 10800.0 && t - e.last_used_at > 1800.0)
        });
        rebuild_keys(&mut data);
        before - data.entries.len()
    }

    /// 连续失败/延迟超阈值降权（免费代理保留策略不变）
    pub async fn demote_bad(&self, max_fails: u32, max_latency_ms: u64) -> usize {
        let mut data = self.inner.write().await;
        let mut demoted = 0;
        for e in data.entries.iter_mut() {
            let bad = e.consecutive_fails >= max_fails.max(1)
                || (e.latency_ms > 0 && e.latency_ms > max_latency_ms && e.latency_ms != u64::MAX);
            if bad {
                e.health_score *= 0.5;
                demoted += 1;
            }
        }
        demoted
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.entries.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.entries.is_empty()
    }

    pub async fn count_free(&self) -> usize {
        self.inner
            .read()
            .await
            .entries
            .iter()
            .filter(|e| e.source == "free")
            .count()
    }

    /// 选择排序：latency 升序（0=未知排后）+ health 降序
    fn rank(a: &ProxyEntry, b: &ProxyEntry) -> std::cmp::Ordering {
        let la = if a.latency_ms == 0 {
            u64::MAX
        } else {
            a.latency_ms
        };
        let lb = if b.latency_ms == 0 {
            u64::MAX
        } else {
            b.latency_ms
        };
        la.cmp(&lb)
            .then(
                b.health_score
                    .partial_cmp(&a.health_score)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                a.cooldown_until
                    .partial_cmp(&b.cooldown_until)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    }

    /// 分配一个可用出口代理：
    /// 1) inflight=0 且 24h 未用过 → latency 升序 + 健康分降序
    /// 2) 全用过 → 同上排序（仍优先 inflight=0）
    /// 3) 全在冷却 → 冷却最早结束（权宜）
    ///
    /// 全局并发经 gate 信号量门控；出口 inflight 计数避免打爆同一出口。
    pub async fn acquire(
        &self,
        prefer_source: Option<&str>,
        hourly_per_ip: usize,
        cooldown_map: &[u32],
    ) -> Option<String> {
        // 全局并发上限：先取许可（阻塞等待，天然限流）
        let gate = self.gate.read().await.clone();
        let permit = gate.acquire_owned().await.expect("semaphore closed");
        let mut data = self.inner.write().await;
        if data.entries.is_empty() {
            return None;
        }
        let t = now();
        let mut idxs: Vec<usize> = (0..data.entries.len())
            .filter(|&i| data.entries[i].available(t, hourly_per_ip))
            .collect();
        if let Some(pref) = prefer_source {
            let p: Vec<usize> = idxs
                .iter()
                .cloned()
                .filter(|&i| data.entries[i].source == pref)
                .collect();
            if !p.is_empty() {
                idxs = p;
            }
        }
        // 优先 inflight=0；其次 inflight 未超软上限
        idxs.retain(|&i| {
            let inf = data
                .inflight
                .get(&host_port_key(&data.entries[i].url))
                .copied()
                .unwrap_or(0);
            inf < PER_PROXY_MAX_INFLIGHT
        });
        let pick = if !idxs.is_empty() {
            let unused: Vec<usize> = idxs
                .iter()
                .cloned()
                .filter(|&i| data.entries[i].use_count == 0)
                .collect();
            let zero_inflight: Vec<usize> = idxs
                .iter()
                .cloned()
                .filter(|&i| {
                    data.inflight
                        .get(&host_port_key(&data.entries[i].url))
                        .copied()
                        .unwrap_or(0)
                        == 0
                })
                .collect();
            if !unused.is_empty() {
                unused
                    .into_iter()
                    .min_by(|&a, &b| Self::rank(&data.entries[a], &data.entries[b]))
                    .unwrap()
            } else if !zero_inflight.is_empty() {
                zero_inflight
                    .into_iter()
                    .min_by(|&a, &b| Self::rank(&data.entries[a], &data.entries[b]))
                    .unwrap()
            } else {
                idxs.into_iter()
                    .min_by(|&a, &b| Self::rank(&data.entries[a], &data.entries[b]))
                    .unwrap()
            }
        } else {
            // 全部出口在冷却/超配额：返回 None（尊重每 IP 每 24h UTC 日限流语义，
            // 由上层直连兜底；不强行使用冷却中的代理引发上游 429）
            return None;
        };
        let key = host_port_key(&data.entries[pick].url);
        {
            let e = &mut data.entries[pick];
            e.last_used_at = t;
            e.use_count += 1;
            e.daily_uses += 1;
            e.cooldown_until = t + cooldown_seconds(e.use_count, cooldown_map) as f64;
        }
        *data.inflight.entry(key.clone()).or_insert(0) += 1;
        data.permits
            .entry(key.clone())
            .or_default()
            .push(Arc::new(permit));
        Some(data.entries[pick].url.clone())
    }

    /// 请求失败：EWMA 下调健康分；429 用递增冷却，其它 30s 冷却；释放并发槽
    pub async fn mark_failure(&self, url: &str, rate_limited: bool, cooldown_map: &[u32]) {
        let mut data = self.inner.write().await;
        let t = now();
        let mut found = false;
        for e in data.entries.iter_mut() {
            if e.url == url {
                e.consecutive_fails += 1;
                e.health_score *= 0.7;
                e.cooldown_until = if rate_limited {
                    t + cooldown_seconds(e.use_count + 1, cooldown_map) as f64
                } else {
                    t + 30.0
                };
                found = true;
                break;
            }
        }
        if found {
            self.release_slot(&mut data, url);
        }
    }

    pub async fn mark_success(&self, url: &str) {
        let mut data = self.inner.write().await;
        let mut found = false;
        for e in data.entries.iter_mut() {
            if e.url == url {
                e.consecutive_fails = 0;
                e.health_score = 0.7 * e.health_score + 0.3;
                e.last_success_ts = now();
                found = true;
                break;
            }
        }
        if found {
            self.release_slot(&mut data, url);
        }
    }

    /// 释放 inflight 计数 + 全局并发许可（幂等：未登记则忽略）
    fn release_slot(&self, data: &mut PoolData, url: &str) {
        let key = host_port_key(url);
        if let Some(n) = data.inflight.get_mut(&key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                data.inflight.remove(&key);
            }
        }
        data.permits.remove(&key);
    }

    /// 同会话出口粘滞（避免上游 IP 跳变风控；窗口默认 300s）
    pub async fn get_sticky(
        &self,
        session_id: &str,
        sticky_window: u64,
        hourly_per_ip: usize,
        cooldown_map: &[u32],
    ) -> Option<String> {
        let mut data = self.inner.write().await;
        let t = now();
        if let Some((url, ts)) = data.sticky.get(session_id).cloned() {
            if sticky_window > 0 && t - ts < sticky_window as f64 {
                if let Some(idx) = data.entries.iter().position(|e| e.url == url) {
                    if data.entries[idx].available(t, hourly_per_ip) {
                        let key = host_port_key(url.as_str());
                        if data.inflight.get(&key).copied().unwrap_or(0) < PER_PROXY_MAX_INFLIGHT {
                            let e = &mut data.entries[idx];
                            e.last_used_at = t;
                            e.use_count += 1;
                            e.daily_uses += 1;
                            e.cooldown_until =
                                t + cooldown_seconds(e.use_count, cooldown_map) as f64;
                            // 全局并发许可（阻塞等待）
                            drop(data);
                            let gate = self.gate.read().await.clone();
                            let permit = gate.acquire_owned().await.expect("semaphore closed");
                            let mut data = self.inner.write().await;
                            *data.inflight.entry(key.clone()).or_insert(0) += 1;
                            data.permits
                                .entry(key.clone())
                                .or_default()
                                .push(Arc::new(permit));
                            data.sticky.insert(session_id.to_string(), (url.clone(), t));
                            return Some(url);
                        }
                    }
                }
            }
        }
        // 未命中/过期 → 先释放锁走 acquire
        drop(data);
        let url = self.acquire(None, hourly_per_ip, cooldown_map).await;
        if let Some(u) = &url {
            let mut data = self.inner.write().await;
            data.sticky.insert(session_id.to_string(), (u.clone(), t));
            if data.sticky.len() > 1000 {
                data.sticky
                    .retain(|_, (_, ts)| t - *ts < sticky_window as f64);
            }
        }
        url
    }

    /// 快照（面板/调试）：含 capacity 容量计算（可用代理数 × hourly_per_ip − 当日已用）
    pub async fn snapshot(&self, hourly_per_ip: usize) -> serde_json::Value {
        let data = self.inner.read().await;
        let t = now();
        let mut items: Vec<ProxySnapshot> = data.entries.iter().map(|e| e.snapshot()).collect();
        items.sort_by(|a, b| {
            b.health_score
                .partial_cmp(&a.health_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let available = data
            .entries
            .iter()
            .filter(|e| e.available(t, hourly_per_ip))
            .count();
        // 容量口径：总容量 = 可用代理数 × hourly_per_ip；
        // 剩余 = Σ 每个可用代理剩余次数 max(0, hourly_per_ip − daily_uses)
        // （避免“全部代理用满时 used>total”的矛盾：只按可用代理统计）
        let capacity_total = available as u64 * hourly_per_ip as u64;
        let capacity_remaining: u64 = data
            .entries
            .iter()
            .filter(|e| e.available(t, hourly_per_ip))
            .map(|e| (hourly_per_ip as u64).saturating_sub(e.daily_uses as u64))
            .sum();
        let capacity_used = capacity_total.saturating_sub(capacity_remaining);
        serde_json::json!({
            "total": data.entries.len(),
            "residential": data.entries.iter().filter(|e| e.source == "residential").count(),
            "free": data.entries.iter().filter(|e| e.source == "free").count(),
            "available": available,
            "cooldown": data.entries.iter().filter(|e| t < e.cooldown_until).count(),
            "inflight": data.inflight.len(),
            "capacity": {
                "capacity_total": capacity_total,
                "capacity_used": capacity_used,
                "capacity_remaining": capacity_remaining,
            },
            "items": items,
        })
    }
}

/// 重建 host:port 去重索引（reap/剔除后调用）
fn rebuild_keys(data: &mut PoolData) {
    data.keys = data.entries.iter().map(|e| host_port_key(&e.url)).collect();
}

/// 供测试使用的并发窗口常量
pub fn default_precheck_concurrency() -> usize {
    DEFAULT_PRECHECK_CONCURRENCY
}

/// 容量计算（供测试/快照复用）：总容量 = 可用代理数 × hourly_per_ip；剩余 = 总 − 当日已用（下限 0）
pub fn capacity_calc(available: usize, hourly_per_ip: usize, used: u64) -> (u64, u64, u64) {
    let capacity_total = available as u64 * hourly_per_ip as u64;
    let capacity_used = used;
    (
        capacity_total,
        capacity_used,
        capacity_total.saturating_sub(capacity_used),
    )
}

/// 批量去重辅助（公开供测试验证 host:port 高效去重）
pub fn add_free_dedupe_helper(urls: Vec<String>) -> Vec<String> {
    let mut keys = std::collections::HashSet::new();
    let mut out = Vec::new();
    for u in urls {
        let norm = normalize_proxy_url(&u);
        if keys.insert(host_port_key(&norm)) {
            out.push(norm);
        }
    }
    out
}
