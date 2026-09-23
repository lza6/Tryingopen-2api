//! 代理池：住宅代理文件 + 免费代理抓取 双源，按 IP 冷却/故障轮换/健康分路由。
//!
//! 适配 tryingopen 上游「单 IP 限流 20 次/h → 代理池自动故障轮换」：
//! - 优先选「24h 窗口内从未用过」的出口 IP；
//! - 429/网络错误 → 冷却该出口并按指数退避换下一个；
//! - 健康分（EWMA）低的降序排底，不硬剔除（给恢复机会）；
//! - 全部可用出口用过一轮后选冷却最早结束的；
//! - 无代理/全部冷却时返回 None，由上层直连兜底（本机 IP 也有每小时配额）。
//!
//! 数据面只暴露 host:port（住宅代理 user:pass 凭据脱敏）。

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

const DAY: u64 = 24 * 3600;

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
}

#[derive(Debug, Clone, Default)]
pub struct ProxyPool {
    inner: Arc<RwLock<PoolData>>,
}

impl ProxyPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// 载入住宅/自备代理文件（每行一个 url，支持 # 注释）
    pub async fn load_file(&self, path: &str) -> usize {
        let mut added = 0;
        match tokio::fs::read_to_string(path).await {
            Ok(text) => {
                let mut data = self.inner.write().await;
                let existing: HashSet<String> =
                    data.entries.iter().map(|e| e.url.clone()).collect();
                let mut fresh: Vec<String> = Vec::new();
                for line in text.lines() {
                    let u = line.trim();
                    if u.is_empty() || u.starts_with('#') {
                        continue;
                    }
                    let norm: String = if u.contains("://") {
                        u.to_string()
                    } else {
                        format!("http://{u}")
                    };
                    if !existing.contains(&norm) && !fresh.contains(&norm) {
                        fresh.push(norm);
                    }
                }
                for norm in fresh {
                    data.entries.push(ProxyEntry::new(norm, "residential"));
                    added += 1;
                }
            }
            Err(e) => tracing::warn!("代理文件不可读 {}: {e}", path),
        }
        added
    }
    /// 批量注入免费代理（去重）
    pub async fn add_free(&self, urls: Vec<String>) -> usize {
        if urls.is_empty() {
            return 0;
        }
        let mut added = 0;
        let mut data = self.inner.write().await;
        let existing: HashSet<String> = data.entries.iter().map(|e| e.url.clone()).collect();
        let mut fresh: Vec<String> = Vec::new();
        for u in urls {
            if !existing.contains(&u) && !fresh.contains(&u) {
                fresh.push(u);
            }
        }
        for u in fresh {
            data.entries.push(ProxyEntry::new(u, "free"));
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
        before - data.entries.len()
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

    /// 分配一个可用出口代理：
    /// 1) 24h 窗口未用过 → 健康分最高
    /// 2) 全用过 → 健康分降序 + 冷却最早结束
    /// 3) 全在冷却 → 冷却最早结束（权宜）
    pub async fn acquire(
        &self,
        prefer_source: Option<&str>,
        hourly_per_ip: usize,
        cooldown_map: &[u32],
    ) -> Option<String> {
        let mut data = self.inner.write().await;
        if data.entries.is_empty() {
            return None;
        }
        let t = now();
        let entries = &mut data.entries;
        let mut idxs: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i].available(t, hourly_per_ip))
            .collect();
        if let Some(pref) = prefer_source {
            let p: Vec<usize> = idxs
                .iter()
                .cloned()
                .filter(|&i| entries[i].source == pref)
                .collect();
            if !p.is_empty() {
                idxs = p;
            }
        }
        let pick = if !idxs.is_empty() {
            let unused: Vec<usize> = idxs
                .iter()
                .cloned()
                .filter(|&i| entries[i].use_count == 0)
                .collect();
            if !unused.is_empty() {
                unused
                    .into_iter()
                    .max_by(|&a, &b| {
                        entries[a]
                            .health_score
                            .partial_cmp(&entries[b].health_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap()
            } else {
                idxs.into_iter()
                    .max_by(|&a, &b| {
                        entries[a]
                            .health_score
                            .partial_cmp(&entries[b].health_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(
                                entries[b]
                                    .cooldown_until
                                    .partial_cmp(&entries[a].cooldown_until)
                                    .unwrap_or(std::cmp::Ordering::Equal),
                            )
                    })
                    .unwrap()
            }
        } else {
            entries
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.cooldown_until
                        .partial_cmp(&b.cooldown_until)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        let e = &mut entries[pick];
        e.last_used_at = t;
        e.use_count += 1;
        e.daily_uses += 1;
        e.cooldown_until = t + cooldown_seconds(e.use_count, cooldown_map) as f64;
        Some(e.url.clone())
    }

    /// 请求失败：EWMA 下调健康分；429 用递增冷却，其它 30s 冷却
    pub async fn mark_failure(&self, url: &str, rate_limited: bool, cooldown_map: &[u32]) {
        let mut data = self.inner.write().await;
        let t = now();
        for e in data.entries.iter_mut() {
            if e.url == url {
                e.consecutive_fails += 1;
                e.health_score *= 0.7;
                e.cooldown_until = if rate_limited {
                    t + cooldown_seconds(e.use_count + 1, cooldown_map) as f64
                } else {
                    t + 30.0
                };
                return;
            }
        }
    }

    pub async fn mark_success(&self, url: &str) {
        let mut data = self.inner.write().await;
        for e in data.entries.iter_mut() {
            if e.url == url {
                e.consecutive_fails = 0;
                e.health_score = 0.7 * e.health_score + 0.3;
                e.last_success_ts = now();
                return;
            }
        }
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
                        let e = &mut data.entries[idx];
                        e.last_used_at = t;
                        e.use_count += 1;
                        e.daily_uses += 1;
                        e.cooldown_until = t + cooldown_seconds(e.use_count, cooldown_map) as f64;
                        let u = e.url.clone();
                        data.sticky.insert(session_id.to_string(), (u.clone(), t));
                        return Some(u);
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

    /// 快照（面板/调试）
    pub async fn snapshot(&self, hourly_per_ip: usize) -> serde_json::Value {
        let data = self.inner.read().await;
        let t = now();
        let mut items: Vec<ProxySnapshot> = data.entries.iter().map(|e| e.snapshot()).collect();
        items.sort_by(|a, b| {
            b.health_score
                .partial_cmp(&a.health_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        serde_json::json!({
            "total": data.entries.len(),
            "residential": data.entries.iter().filter(|e| e.source == "residential").count(),
            "free": data.entries.iter().filter(|e| e.source == "free").count(),
            "available": data.entries.iter().filter(|e| e.available(t, hourly_per_ip)).count(),
            "cooldown": data.entries.iter().filter(|e| t < e.cooldown_until).count(),
            "items": items,
        })
    }
}
