//! 免费代理池抓取器：从公开免费源抓取/解析/并发 HTTP 延迟预检/注入共享 ProxyPool。
//!
//! 移植自 imagefree-2ai `api/free_proxy_fetcher.py` 并深度扩展：
//! - 36 个真实可达源（GitHub 知名代理列表项目 + proxyscrape/geonode，均经真实请求验证）；
//! - 解析统一规范化（http:// 前缀 + host:port 去重，不同 user:pass 同出口算一个）；
//! - 预检从 TCP 连通性升级为「并发真实 HTTP 延迟测量」（如 generate_204），
//!   tokio::sync::Semaphore 限流 50 并发，测出 latency_ms 供 acquire 低延迟优先；
//! - 注入失败（超时/连接错误）的代理不进入池子；连续失败/超阈值的代理降权剔除。
//!
//! 安全边界：免费代理无凭据、明文 http 有数据泄露风险；仅用于低敏感度对话请求，
//! 默认开启（config free_proxy_enabled:true），可用环境变量 FREE_PROXY_ENABLED=0 关闭。

use crate::proxy_pool::host_port_key;
use crate::proxy_pool::ProxyPool;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;

const FETCH_TIMEOUT_SEC: u64 = 25;
const PRECHECK_TIMEOUT_SEC: u64 = 4;
/// 并发预检/注入窗口
const PRECHECK_CONCURRENCY: usize = 50;
/// 每源最多注入数量（超大源截断，避免一轮抓取被占满）
const MAX_PER_SOURCE: usize = 1200;
/// 一轮总预检上限
const MAX_PRECHECK_TOTAL: usize = 4500;
/// 连续失败超过该次数 → 降权
const MAX_FAILS: u32 = 6;
/// 延迟超过该阈值(ms) → 降权
const MAX_LATENCY_MS: u64 = 5000;

struct Source {
    name: &'static str,
    url: &'static str,
    /// 文本格式解析方式
    fmt: &'static str,
    /// 代理协议前缀（http:// https:// socks4:// socks5://）
    scheme: &'static str,
}

// 真实可达性验证于 2026-09-24（Invoke-WebRequest，部分源经 GitHub raw 直连验证）：
// 不可达或非 ip:port 格式的源一律不收录（严禁编造）。
const FREE_PROXY_SOURCES: &[Source] = &[
    // ---- proxyscrape API（HTTP/HTTPS/SOCKS4/SOCKS5）----
    Source { name: "proxyscrape-v3-http", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=http&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport", scheme: "http://" },
    Source { name: "proxyscrape-v3-ssl", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=https&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport", scheme: "https://" },
    Source { name: "proxyscrape-v3-socks4", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=socks4&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport", scheme: "socks4://" },
    Source { name: "proxyscrape-v3-socks5", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=socks5&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport", scheme: "socks5://" },
    Source { name: "proxyscrape-v2", url: "https://api.proxyscrape.com/v2/?request=getproxies&protocol=http&timeout=10000&country=all&ssl=all&anonymity=all", fmt: "ipport", scheme: "http://" },
    // ---- geonode JSON（HTTP 500 条）----
    Source { name: "geonode", url: "https://proxylist.geonode.com/api/proxy-list?limit=500&page=1&sort_by=lastChecked&sort_type=desc&protocols=http", fmt: "json", scheme: "http://" },
    // ---- proxifly（HTTP/SOCKS5/SOCKS4 独立子文件）----
    Source { name: "proxifly-http", url: "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/http/data.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "proxifly-socks5", url: "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/socks5/data.txt", fmt: "ipport", scheme: "socks5://" },
    Source { name: "proxifly-socks4", url: "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/protocols/socks4/data.txt", fmt: "ipport", scheme: "socks4://" },
    // ---- TheSpeedX/PROXY-List ----
    Source { name: "thespeedx-http", url: "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "thespeedx-socks5", url: "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt", fmt: "ipport", scheme: "socks5://" },
    Source { name: "thespeedx-socks4", url: "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks4.txt", fmt: "ipport", scheme: "socks4://" },
    // ---- ErcinDedeoglu/proxies（HTTP/HTTPS/SOCKS4/SOCKS5）----
    Source { name: "ercindedeoglu-http", url: "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "ercindedeoglu-https", url: "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/https.txt", fmt: "ipport", scheme: "https://" },
    Source { name: "ercindedeoglu-socks4", url: "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/socks4.txt", fmt: "ipport", scheme: "socks4://" },
    Source { name: "ercindedeoglu-socks5", url: "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/socks5.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- proxy4parsing/proxy-list ----
    Source { name: "proxy4parsing-http", url: "https://raw.githubusercontent.com/proxy4parsing/proxy-list/main/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "proxy4parsing-hproxy", url: "https://raw.githubusercontent.com/proxy4parsing/proxy-list/main/hproxy.txt", fmt: "ipport", scheme: "http://" },
    // ---- monosans/proxy-list ----
    Source { name: "monosans-http", url: "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "monosans-all", url: "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/all.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "monosans-socks4", url: "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/socks4.txt", fmt: "ipport", scheme: "socks4://" },
    Source { name: "monosans-socks5", url: "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/socks5.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- hookzof/socks5_list ----
    Source { name: "hookzof-socks5", url: "https://raw.githubusercontent.com/hookzof/socks5_list/master/proxy.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- clarketm/proxy-list ----
    Source { name: "clarketm", url: "https://raw.githubusercontent.com/clarketm/proxy-list/master/proxy-list-raw.txt", fmt: "ipport", scheme: "http://" },
    // ---- roosterkid/openproxylist（RAW 纯 ip:port）----
    Source { name: "roosterkid-https", url: "https://raw.githubusercontent.com/roosterkid/openproxylist/main/HTTPS_RAW.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "roosterkid-socks4", url: "https://raw.githubusercontent.com/roosterkid/openproxylist/main/SOCKS4_RAW.txt", fmt: "ipport", scheme: "socks4://" },
    Source { name: "roosterkid-socks5", url: "https://raw.githubusercontent.com/roosterkid/openproxylist/main/SOCKS5_RAW.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- ShiftyTR/Proxy-List ----
    Source { name: "shiftytr-http", url: "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "shiftytr-socks4", url: "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/socks4.txt", fmt: "ipport", scheme: "socks4://" },
    Source { name: "shiftytr-socks5", url: "https://raw.githubusercontent.com/ShiftyTR/Proxy-List/master/socks5.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- mmpx12/proxy-list ----
    Source { name: "mmpx12-http", url: "https://raw.githubusercontent.com/mmpx12/proxy-list/master/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "mmpx12-socks5", url: "https://raw.githubusercontent.com/mmpx12/proxy-list/master/socks5.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- mishakorzik/Free-Proxy ----
    Source { name: "mishakorzik", url: "https://raw.githubusercontent.com/mishakorzik/Free-Proxy/main/proxy.txt", fmt: "ipport", scheme: "http://" },
    // ---- ProxyScrape free-proxy-list（独立仓库 data.txt）----
    Source { name: "proxyscrape-gh-http", url: "https://raw.githubusercontent.com/ProxyScrape/free-proxy-list/main/proxies/protocols/http/data.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "proxyscrape-gh-socks5", url: "https://raw.githubusercontent.com/ProxyScrape/free-proxy-list/main/proxies/protocols/socks5/data.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- MuRongPIG/Proxy-Master ----
    Source { name: "murongpig-http", url: "https://raw.githubusercontent.com/MuRongPIG/Proxy-Master/main/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "murongpig-socks4", url: "https://raw.githubusercontent.com/MuRongPIG/Proxy-Master/main/socks4.txt", fmt: "ipport", scheme: "socks4://" },
    Source { name: "murongpig-socks5", url: "https://raw.githubusercontent.com/MuRongPIG/Proxy-Master/main/socks5.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- r00tee/Proxy-List ----
    Source { name: "r00tee-https", url: "https://raw.githubusercontent.com/r00tee/Proxy-List/master/Https.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "r00tee-socks4", url: "https://raw.githubusercontent.com/r00tee/Proxy-List/master/Socks4.txt", fmt: "ipport", scheme: "socks4://" },
    Source { name: "r00tee-socks5", url: "https://raw.githubusercontent.com/r00tee/Proxy-List/master/Socks5.txt", fmt: "ipport", scheme: "socks5://" },
    // ---- Vann-Dev/proxy-list ----
    Source { name: "vanndev-http", url: "https://raw.githubusercontent.com/Vann-Dev/proxy-list/master/proxies/http.txt", fmt: "ipport", scheme: "http://" },
    Source { name: "vanndev-socks5", url: "https://raw.githubusercontent.com/Vann-Dev/proxy-list/master/proxies/socks5.txt", fmt: "ipport", scheme: "socks5://" },
];

#[derive(Debug, Default)]
pub struct FetcherStats {
    pub sources_ok: u32,
    pub fetched: u32,
    pub injected: u32,
    pub last_at: u64,
}
/// 只保留合法公网 IP（拒绝内网/回环/链路本地/保留/组播/未指定）
pub fn is_valid_public_ip(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(addr) => match addr {
            IpAddr::V4(v4) => {
                !(v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_multicast()
                    || v4.is_unspecified()
                    || v4.is_broadcast())
            }
            IpAddr::V6(v6) => !(v6.is_loopback() || v6.is_multicast() || v6.is_unspecified()),
        },
        Err(_) => false,
    }
}

/// 解析纯 ip:port 文本（兼容已有前缀/注释/CRLF；按指定 scheme 输出）
pub fn parse_ipport_text_scheme(text: &str, scheme: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = strip_scheme(line);
        if line.matches(':').count() != 1 {
            continue;
        }
        let (host, port) = line.rsplit_once(':').unwrap();
        if host.is_empty() || port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !is_valid_public_ip(host) {
            continue;
        }
        let url = format!("{}{host}:{port}", scheme);
        if seen.insert(url.clone()) {
            out.push(url);
        }
    }
    out
}

/// 解析纯 ip:port 文本（默认 http:// 前缀；兼容测试与无协议源）
pub fn parse_ipport_text(text: &str) -> Vec<String> {
    parse_ipport_text_scheme(text, "http://")
}

fn strip_scheme(line: &str) -> &str {
    for p in ["http://", "https://", "socks5://", "socks4://", "socks://"] {
        if let Some(rest) = line.strip_prefix(p) {
            return rest;
        }
    }
    line
}

/// 解析 geonode JSON（data[].ip/port）
pub fn parse_geonode_json(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let data = v.get("data").and_then(|d| d.as_array());
    let Some(data) = data else { return out };
    for item in data {
        let ip = item.get("ip").and_then(|x| x.as_str()).unwrap_or("");
        let port = item.get("port").and_then(|x| x.as_str()).unwrap_or("");
        if ip.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !is_valid_public_ip(ip) {
            continue;
        }
        out.push(format!("http://{ip}:{port}"));
    }
    out
}

fn parse_source(payload: &str, fmt: &str, scheme: &str) -> Vec<String> {
    match fmt {
        "ipport" => parse_ipport_text_scheme(payload, scheme),
        "json" => parse_geonode_json(payload),
        _ => Vec::new(),
    }
}

/// 真实 HTTP 延迟测量：经代理发一个快速无害请求（generate_204），返回毫秒延迟。
/// 失败（连接错误/超时/非 2xx）返回 None —— 该代理不进入池子。
async fn measure_latency(url: &str) -> Option<u64> {
    let proxy = reqwest::Proxy::all(url).ok()?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(PRECHECK_TIMEOUT_SEC))
        .connect_timeout(Duration::from_secs(PRECHECK_TIMEOUT_SEC))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36")
        .no_proxy()
        .build()
        .ok()?;
    let start = Instant::now();
    let resp = client
        .get("http://www.gstatic.com/generate_204")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    Some(start.elapsed().as_millis() as u64)
}

/// 并发延迟预检（供测试注入可替换的 checker）
async fn check_all_concurrent<F, Fut>(
    urls: Vec<String>,
    checker: Arc<F>,
    limit: usize,
) -> Vec<(String, u64)>
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Option<u64>> + Send + 'static,
{
    let sem = Arc::new(Semaphore::new(limit.max(1)));
    let mut handles = Vec::with_capacity(urls.len());
    for u in urls.into_iter() {
        let sem = sem.clone();
        let checker = checker.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let ms = checker(u.clone()).await?;
            Some((u, ms))
        }));
    }
    let mut out = Vec::new();
    for h in handles {
        if let Ok(Some(pair)) = h.await {
            out.push(pair);
        }
    }
    out
}

/// 异步后台循环：抓取 → 解析 → 并发 HTTP 延迟预检 → 注入 → 周期刷新 + 剔除 + 降权
pub async fn run_loop(
    pool: std::sync::Arc<ProxyPool>,
    refresh_min: u64,
    stop: tokio::sync::watch::Receiver<bool>,
) {
    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(refresh_min.max(1) * 60));
    interval.tick().await; // 第一次立即 tick
    let has_stop = *stop.borrow();
    if has_stop {
        return;
    }
    let _ = refresh_once(pool.as_ref()).await;
    let mut recv = stop;
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let _injected = refresh_once(pool.as_ref()).await;
                if _injected > 0 {
                    tracing::info!("免费代理刷新注入 {_injected} 个（池总数 {}）", pool.len().await);
                }
                let reaped = pool.reap_free().await;
                if reaped > 0 {
                    tracing::info!("剔除过期免费代理 {reaped} 个");
                }
                let demoted = pool.demote_bad(MAX_FAILS, MAX_LATENCY_MS).await;
                if demoted > 0 {
                    tracing::info!("连续失败/高延迟免费代理降权 {demoted} 个");
                }
            }
            changed = recv.changed() => {
                if changed.is_err() || *recv.borrow() { break; }
            }
        }
    }
}

pub async fn refresh_once(pool: &ProxyPool) -> usize {
    let mut fetched_all: Vec<String> = Vec::new();
    let mut ok = 0u32;
    for src in FREE_PROXY_SOURCES {
        match fetch_text(src.url).await {
            Some(text) => {
                ok += 1;
                let parsed = parse_source(&text, src.fmt, src.scheme);
                // 超大源截断，避免一轮被占满
                let parsed: Vec<String> = parsed.into_iter().take(MAX_PER_SOURCE).collect();
                fetched_all.extend(parsed);
            }
            None => tracing::debug!("免费代理源 {} 抓取失败", src.name),
        }
    }
    // host:port 去重（忽略 scheme/user:pass），再截断预检规模
    let mut seen = std::collections::HashSet::new();
    fetched_all.retain(|u| seen.insert(host_port_key(u)));
    fetched_all.truncate(MAX_PRECHECK_TOTAL);
    let parsed_count = fetched_all.len();
    // 并发 HTTP 延迟预检（50 并发）
    let measured = check_all_concurrent(
        fetched_all,
        Arc::new(measure_latency_wrapper),
        PRECHECK_CONCURRENCY,
    )
    .await;
    let mut latency_map = std::collections::HashMap::new();
    for (u, ms) in &measured {
        latency_map.insert(u.clone(), *ms);
    }
    // 只注入预检成功的代理
    let mut injected = 0usize;
    let mut batch: Vec<(String, u64)> = Vec::with_capacity(measured.len());
    for (u, ms) in measured {
        batch.push((u, ms));
        if batch.len() >= 500 {
            injected += pool.add_free_with_latency(std::mem::take(&mut batch)).await;
        }
    }
    if !batch.is_empty() {
        injected += pool.add_free_with_latency(batch).await;
    }
    let mut stats = pool_stats(pool).await;
    stats.sources_ok = ok;
    stats.fetched = parsed_count as u32;
    stats.injected = injected as u32;
    stats.last_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    tracing::debug!(
        "免费代理一轮: 源OK={ok} 解析={} 预检通过={} 注入={}",
        stats.fetched,
        latency_map.len(),
        stats.injected
    );
    stats.injected as usize
}

async fn fetch_text(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SEC))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36")
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

async fn pool_stats(_pool: &ProxyPool) -> FetcherStats {
    FetcherStats::default()
}

/// 包装函数指针以匹配 check_all_concurrent 泛型签名
async fn measure_latency_wrapper(url: String) -> Option<u64> {
    measure_latency(&url).await
}

// ---- 测试辅助：供 tests/ 复用（不联网） ----

/// 并发预检限额（供单元测试断言伪测并发窗口）
pub fn precheck_concurrency() -> usize {
    PRECHECK_CONCURRENCY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn concurrent_precheck_respects_limit_and_collects_latency() {
        // 伪测 checker：不联网，记录并发峰值
        let urls: Vec<String> = (0..120)
            .map(|i| format!("http://10.0.0.{i}:8080"))
            .collect();
        let peak = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak2 = peak.clone();
        let active2 = active.clone();
        let out = check_all_concurrent(
            urls,
            Arc::new(move |_u| {
                let peak = peak2.clone();
                let active = active2.clone();
                async move {
                    let a = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    peak.fetch_max(a, std::sync::atomic::Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    Some(3u64) // 伪延迟
                }
            }),
            precheck_concurrency(),
        )
        .await;
        assert_eq!(out.len(), 120);
        assert!(
            peak.load(std::sync::atomic::Ordering::SeqCst) <= precheck_concurrency(),
            "并发峰值 {} 超过窗口 {}",
            peak.load(std::sync::atomic::Ordering::SeqCst),
            precheck_concurrency()
        );
    }
}

