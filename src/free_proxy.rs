//! 免费代理池抓取器：从公开免费源抓取/解析/TCP 预检/注入共享 ProxyPool。
//!
//! 移植自 imagefree-2ai `api/free_proxy_fetcher.py`（13 个免费源 + 公网 IP 白名单过滤
//! + TCP 连通性预检）。用途：tryingopen「单 IP 20 次/h」限流下轮换出口。
//!
//! 安全边界：免费代理无凭据、明文 http 有数据泄露风险；仅用于低敏感度对话请求，
//! 默认关闭（IF_FREE_PROXY=0 / config free_proxy_enabled:false）。

use crate::proxy_pool::ProxyPool;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

const FETCH_TIMEOUT: u64 = 15;
const PRECHECK_TIMEOUT: u64 = 3;

struct Source {
    name: &'static str,
    url: &'static str,
    fmt: &'static str,
}

const FREE_PROXY_SOURCES: &[Source] = &[
    Source { name: "proxyscrape-v3-http", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=http&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport" },
    Source { name: "proxyscrape-v3-ssl", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=https&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport" },
    Source { name: "proxyscrape-v3-socks4", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=socks4&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport" },
    Source { name: "proxyscrape-v3-socks5", url: "https://api.proxyscrape.com/v3/free-proxy-list/get?request=displayproxies&protocol=socks5&proxy_format=ipport&format=text&timeout=5000&country=all", fmt: "ipport" },
    Source { name: "proxyscrape-v2", url: "https://api.proxyscrape.com/v2/?request=getproxies&protocol=http&timeout=10000&country=all&ssl=all&anonymity=all", fmt: "ipport" },
    Source { name: "geonode", url: "https://proxylist.geonode.com/api/proxy-list?limit=500&page=1&sort_by=lastChecked&sort_type=desc&protocols=http", fmt: "json" },
    Source { name: "proxifly-github", url: "https://raw.githubusercontent.com/proxifly/free-proxy-list/main/proxies/all/data.txt", fmt: "ipport" },
    Source { name: "thespeedx-http", url: "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt", fmt: "ipport" },
    Source { name: "ercindedeoglu-http", url: "https://raw.githubusercontent.com/ErcinDedeoglu/proxies/main/proxies/http.txt", fmt: "ipport" },
    Source { name: "proxy4parsing-http", url: "https://raw.githubusercontent.com/proxy4parsing/proxy-list/main/http.txt", fmt: "ipport" },
    Source { name: "monosans-http", url: "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/http.txt", fmt: "ipport" },
    Source { name: "thespeedx-socks5", url: "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks5.txt", fmt: "ipport" },
    Source { name: "thespeedx-socks4", url: "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/socks4.txt", fmt: "ipport" },
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
pub fn parse_ipport_text(text: &str) -> Vec<String> {
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
        let url = format!("http://{host}:{port}");
        if seen.insert(url.clone()) {
            out.push(url);
        }
    }
    out
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

fn parse_source(payload: &str, fmt: &str) -> Vec<String> {
    match fmt {
        "ipport" => parse_ipport_text(payload),
        "json" => parse_geonode_json(payload),
        _ => Vec::new(),
    }
}

/// TCP 连通性预检：能连上代理端口即可（不做真实转发验证）
async fn precheck(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (host, port) = rest.rsplit_once(':').unwrap_or((rest, "80"));
    let port: u16 = port.parse().unwrap_or(80);
    tokio::time::timeout(
        std::time::Duration::from_secs(PRECHECK_TIMEOUT),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

/// 异步后台循环：抓取 → 解析 → TCP 预检 → 注入 → 周期刷新 + 剔除
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
                let parsed = parse_source(&text, src.fmt);
                fetched_all.extend(parsed);
            }
            None => tracing::debug!("免费代理源 {} 抓取失败", src.name),
        }
    }
    // 去重后 TCP 预检
    let mut seen = std::collections::HashSet::new();
    fetched_all.retain(|u| seen.insert(u.clone()));
    let mut injected = 0usize;
    for url in fetched_all.iter().cloned() {
        if precheck(&url).await {
            injected += pool.add_free(vec![url]).await;
        }
    }
    let mut stats = pool_stats(pool).await;
    stats.sources_ok = ok;
    stats.fetched = fetched_all.len() as u32;
    stats.injected = injected as u32;
    stats.last_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    tracing::debug!(
        "免费代理一轮: 源OK={ok} 解析={} 注入={}",
        stats.fetched,
        stats.injected
    );
    stats.injected as usize
}

async fn fetch_text(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT))
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
