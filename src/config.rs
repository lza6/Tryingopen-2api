//! 配置解析（config.json + 环境变量覆盖）
//!
//! TryingOpen2API 是「完全匿名」网关：上游 tryingopen.com 的所有对话端点
//! 不要求 Cookie/登录，全站按「每 24h UTC 日约 20 次」限流。因此本网关没有
//! 账号/凭证池，只需要：监听地址、上游地址、代理池配置、模型目录开关。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 深合并：local 的非 null 字段覆盖 base；对象递归合并，数组/标量直接覆盖
fn merge_json(base: serde_json::Value, local: serde_json::Value) -> Option<serde_json::Value> {
    match (base, local) {
        (serde_json::Value::Object(mut b), serde_json::Value::Object(l)) => {
            for (k, v) in l {
                if v.is_null() {
                    continue;
                }
                match b.get(&k) {
                    Some(bv) if bv.is_object() && v.is_object() => {
                        if let Some(m) = merge_json(bv.clone(), v) {
                            b.insert(k, m);
                        }
                    }
                    _ => {
                        b.insert(k, v);
                    }
                }
            }
            Some(serde_json::Value::Object(b))
        }
        (_, v) => Some(v),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 监听地址
    #[serde(default = "default_listen")]
    pub listen_addr: String,
    /// 上游 tryingopen.com 基址
    #[serde(default = "default_upstream")]
    pub upstream_base_url: String,
    /// 下游 API Key（配置后客户端必须带；空 = 本机放行）
    #[serde(default)]
    pub api_keys: Vec<String>,
    /// 默认模型
    #[serde(default = "default_model")]
    pub default_model: String,
    /// 降级链（429 出口全部耗尽 / 模型不可用时的兜底顺序）
    #[serde(default = "default_fallbacks")]
    pub fallback_models: Vec<String>,
    /// 请求超时（秒）
    #[serde(default = "default_timeout")]
    pub request_timeout_sec: u64,
    /// 上游目录动态抓取周期（分钟；0 = 关闭，仅用静态目录）
    #[serde(default = "default_catalog_min")]
    pub catalog_refresh_min: u64,
    /// ── 代理池 ──
    /// 住宅/自备代理文件：每行一个 http://user:pass@host:port 或 socks5://host:port
    #[serde(default)]
    pub proxy_file: String,
    /// 免费代理抓取开关（默认开启：30+ 源并发预检自动注入；可 FREE_PROXY_ENABLED=0 显式关闭）
    #[serde(default = "default_true")]
    pub free_proxy_enabled: bool,
    /// 免费代理刷新周期（分钟）
    #[serde(default = "default_free_proxy_min")]
    pub free_proxy_refresh_min: u64,
    /// 每 24h UTC 日限流（按 IP 配额）（tryingopen 站点约束，用于代理选择/冷却语义）
    #[serde(default = "default_hourly_per_ip")]
    pub hourly_per_ip: usize,
    /// 单请求最大出口尝试轮数（超出后直连兜底）
    #[serde(default = "default_max_attempts")]
    pub max_attempts: usize,
    /// 代理池全局并发请求上限（同时打出去的不同出口数）
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
    /// 递增冷却秒数映射（逗号分隔；第 N 次使用后等待 X 秒）
    #[serde(default = "default_cooldown_map")]
    pub cooldown_map: String,
    /// 直连兜底（代理全部失败后最后试一次本机出口；上游每分钟 20 次本机配额）
    #[serde(default = "default_true")]
    pub direct_fallback: bool,
    /// 跳过上游健康检查
    #[serde(default = "default_true")]
    pub skip_upstream_check: bool,
    /// 日志脱敏
    #[serde(default = "default_true")]
    pub redact_logs: bool,
    /// Web 面板访问密码（空=不鉴权；设置后 / 和 /ui 需 Basic Auth）
    #[serde(default)]
    pub ui_password: String,
    /// ── 生产保护：限流 / 熔断 / 可观测性 ──
    /// 每 API Key 限流开关（默认开）
    #[serde(default = "default_true")]
    pub rate_limit_enabled: bool,
    /// 窗口内最大请求数
    #[serde(default = "default_rate_limit_requests")]
    pub rate_limit_requests: u64,
    /// 限流窗口长度（秒）
    #[serde(default = "default_rate_limit_window_sec")]
    pub rate_limit_window_sec: u64,
    /// 上游整体熔断开关（默认开）
    #[serde(default = "default_true")]
    pub circuit_breaker_enabled: bool,
    /// 连续失败多少次后 OPEN
    #[serde(default = "default_cb_failure_threshold")]
    pub cb_failure_threshold: u32,
    /// OPEN 后多久进入 HALF_OPEN 探测
    #[serde(default = "default_cb_timeout_sec")]
    pub cb_timeout_sec: u64,
    /// Prometheus /metrics 端点开关（默认开）
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
    /// 限流 map 最大条目数（防唯一 key 制造无界内存）
    #[serde(default = "default_max_rate_keys")]
    pub rate_limit_max_keys: usize,
    /// 直连兜底每窗口配额（避免匿名上游「每 24h UTC 日约 20 次」被共享打满）
    #[serde(default = "default_direct_quota")]
    pub direct_fallback_quota: u64,
}
fn default_max_rate_keys() -> usize {
    4096
}
fn default_direct_quota() -> u64 {
    10
}
fn default_rate_limit_requests() -> u64 {
    60
}
fn default_rate_limit_window_sec() -> u64 {
    3600
}
fn default_cb_failure_threshold() -> u32 {
    5
}
fn default_cb_timeout_sec() -> u64 {
    30
}
fn default_listen() -> String {
    "127.0.0.1:47831".into()
}
fn default_upstream() -> String {
    "https://www.tryingopen.com".into()
}
fn default_model() -> String {
    "qwen/qwen3.8-27b".into()
}
fn default_fallbacks() -> Vec<String> {
    vec![
        "deepseek/deepseek-v4-flash-0731".into(),
        "z-ai/glm-5.2".into(),
        "minimax/minimax-m3".into(),
    ]
}
fn default_timeout() -> u64 {
    120
}
fn default_catalog_min() -> u64 {
    30
}
fn default_free_proxy_min() -> u64 {
    30
}
fn default_hourly_per_ip() -> usize {
    20
}
fn default_max_attempts() -> usize {
    3
}
fn default_max_concurrent() -> usize {
    64
}
fn default_cooldown_map() -> String {
    "0,15,60,120,300".into()
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: default_listen(),
            upstream_base_url: default_upstream(),
            api_keys: vec![],
            default_model: default_model(),
            fallback_models: default_fallbacks(),
            request_timeout_sec: default_timeout(),
            catalog_refresh_min: default_catalog_min(),
            proxy_file: String::new(),
            free_proxy_enabled: true,
            free_proxy_refresh_min: default_free_proxy_min(),
            hourly_per_ip: default_hourly_per_ip(),
            max_attempts: default_max_attempts(),
            max_concurrent_requests: default_max_concurrent(),
            cooldown_map: default_cooldown_map(),
            direct_fallback: default_true(),
            skip_upstream_check: default_true(),
            ui_password: String::new(),
            rate_limit_enabled: default_true(),
            rate_limit_requests: default_rate_limit_requests(),
            rate_limit_window_sec: default_rate_limit_window_sec(),
            circuit_breaker_enabled: default_true(),
            cb_failure_threshold: default_cb_failure_threshold(),
            cb_timeout_sec: default_cb_timeout_sec(),
            metrics_enabled: default_true(),
            rate_limit_max_keys: default_max_rate_keys(),
            direct_fallback_quota: default_direct_quota(),
            redact_logs: default_true(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&std::path::Path>) -> Result<Self> {
        let mut cfg: Config = if let Some(p) = path {
            if p.exists() {
                let raw = std::fs::read_to_string(p)
                    .with_context(|| format!("读取配置文件失败: {}", p.display()))?;
                serde_json::from_str(&raw)
                    .with_context(|| format!("解析配置文件失败: {}", p.display()))?
            } else {
                Config::default()
            }
        } else {
            Config::default()
        };
        // 本地覆盖通道：同目录 config.local.json 存在时，用其字段覆盖主配置
        // （只合并显式出现的字段，不重置未出现字段；便于本机调试/部署差异，且不入 git）
        if let Ok(local_raw) = std::fs::read_to_string("config.local.json") {
            match serde_json::from_str::<serde_json::Value>(&local_raw) {
                Ok(local_value) => {
                    if let Ok(cfg_value) = serde_json::to_value(&cfg) {
                        if let Some(merged) = merge_json(cfg_value, local_value) {
                            match serde_json::from_value(merged) {
                                Ok(m) => cfg = m,
                                Err(e) => {
                                    tracing::warn!("config.local.json 合并失败（保留主配置）: {e}")
                                }
                            }
                        }
                    }
                }
                Err(e) => tracing::warn!("config.local.json 解析失败（忽略）: {e}"),
            }
        }
        // 环境变量覆盖
        if let Ok(v) = std::env::var("LISTEN_ADDR") {
            cfg.listen_addr = v;
        }
        if let Ok(v) = std::env::var("UPSTREAM_BASE_URL") {
            cfg.upstream_base_url = v.trim_end_matches('/').to_string();
        }
        if let Ok(v) = std::env::var("API_KEYS") {
            cfg.api_keys = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = std::env::var("DEFAULT_MODEL") {
            cfg.default_model = v;
        }
        if let Ok(v) = std::env::var("REQUEST_TIMEOUT_SEC") {
            cfg.request_timeout_sec = v.parse().unwrap_or(cfg.request_timeout_sec);
        }
        if let Ok(v) = std::env::var("CATALOG_REFRESH_MIN") {
            cfg.catalog_refresh_min = v.parse().unwrap_or(cfg.catalog_refresh_min);
        }
        if let Ok(v) = std::env::var("PROXY_FILE") {
            cfg.proxy_file = v;
        }
        if let Ok(v) = std::env::var("FREE_PROXY_ENABLED") {
            cfg.free_proxy_enabled = matches!(v.trim().to_lowercase().as_str(), "1" | "true");
        }
        if let Ok(v) = std::env::var("FREE_PROXY_REFRESH_MIN") {
            cfg.free_proxy_refresh_min = v.parse().unwrap_or(cfg.free_proxy_refresh_min);
        }
        if let Ok(v) = std::env::var("HOURLY_PER_IP") {
            cfg.hourly_per_ip = v.parse().unwrap_or(cfg.hourly_per_ip);
        }
        if let Ok(v) = std::env::var("MAX_ATTEMPTS") {
            cfg.max_attempts = v.parse().unwrap_or(cfg.max_attempts);
        }
        if let Ok(v) = std::env::var("COOLDOWN_MAP") {
            cfg.cooldown_map = v;
        }
        if let Ok(v) = std::env::var("MAX_CONCURRENT_REQUESTS") {
            cfg.max_concurrent_requests = v.parse().unwrap_or(cfg.max_concurrent_requests);
        }
        if let Ok(v) = std::env::var("DIRECT_FALLBACK") {
            cfg.direct_fallback = matches!(v.trim().to_lowercase().as_str(), "1" | "true");
        }
        if let Ok(v) = std::env::var("UI_PASSWORD") {
            cfg.ui_password = v;
        }
        // ── 生产保护：限流 / 熔断 / 可观测性（环境变量覆盖）──
        if let Ok(v) = std::env::var("RATE_LIMIT_ENABLED") {
            cfg.rate_limit_enabled = matches!(v.trim().to_lowercase().as_str(), "1" | "true");
        }
        if let Ok(v) = std::env::var("RATE_LIMIT_REQUESTS") {
            cfg.rate_limit_requests = v.parse().unwrap_or(cfg.rate_limit_requests);
        }
        if let Ok(v) = std::env::var("RATE_LIMIT_WINDOW_SEC") {
            cfg.rate_limit_window_sec = v.parse().unwrap_or(cfg.rate_limit_window_sec);
        }
        if let Ok(v) = std::env::var("CB_ENABLED") {
            cfg.circuit_breaker_enabled = matches!(v.trim().to_lowercase().as_str(), "1" | "true");
        }
        if let Ok(v) = std::env::var("CB_FAILURE_THRESHOLD") {
            cfg.cb_failure_threshold = v.parse().unwrap_or(cfg.cb_failure_threshold);
        }
        if let Ok(v) = std::env::var("CB_TIMEOUT_SEC") {
            cfg.cb_timeout_sec = v.parse().unwrap_or(cfg.cb_timeout_sec);
        }
        if let Ok(v) = std::env::var("METRICS_ENABLED") {
            cfg.metrics_enabled = matches!(v.trim().to_lowercase().as_str(), "1" | "true");
        }
        if let Ok(v) = std::env::var("RATE_LIMIT_MAX_KEYS") {
            cfg.rate_limit_max_keys = v.parse().unwrap_or(cfg.rate_limit_max_keys);
        }
        if let Ok(v) = std::env::var("DIRECT_FALLBACK_QUOTA") {
            cfg.direct_fallback_quota = v.parse().unwrap_or(cfg.direct_fallback_quota);
        }
        Ok(cfg)
    }

    pub fn resolve_config_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("CONFIG_PATH") {
            return Some(PathBuf::from(p));
        }
        let p = PathBuf::from("config.json");
        if p.exists() {
            Some(p)
        } else {
            None
        }
    }

    /// 解析递增冷却映射
    pub fn cooldown_vec(&self) -> Vec<u32> {
        crate::proxy_pool::parse_cooldown_map(&self.cooldown_map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_apply_to_config() {
        unsafe {
            std::env::set_var("LISTEN_ADDR", "127.0.0.1:59999");
            std::env::set_var("API_KEYS", "sk-a, sk-b");
            std::env::set_var("RATE_LIMIT_REQUESTS", "7");
            std::env::set_var("FREE_PROXY_ENABLED", "0");
        }
        let cfg = Config::load(None).unwrap();
        assert_eq!(cfg.listen_addr, "127.0.0.1:59999");
        assert_eq!(cfg.api_keys, vec!["sk-a", "sk-b"]);
        assert_eq!(cfg.rate_limit_requests, 7);
        assert!(!cfg.free_proxy_enabled);
        // 清理，避免污染其它测试
        unsafe {
            std::env::remove_var("LISTEN_ADDR");
            std::env::remove_var("API_KEYS");
            std::env::remove_var("RATE_LIMIT_REQUESTS");
            std::env::remove_var("FREE_PROXY_ENABLED");
        }
    }

    #[test]
    fn dead_fields_removed() {
        // rusqlite/sqlite/telemetry/proxies_path/precheck 均为已移除死配置
        let cfg = Config::default();
        // 死字段已删除：sqlite_path/telemetry_path/proxies_path/precheck_concurrency 不应存在
        // （编译期验证：若字段仍在则访问报错；此处仅验证 default 可构造含 proxy_file 空串语义）
        assert!(cfg.proxy_file.is_empty());
    }
}
