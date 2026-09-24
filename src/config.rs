//! 配置解析（config.json + 环境变量覆盖）
//!
//! TryingOpen2API 是「完全匿名」网关：上游 tryingopen.com 的所有对话端点
//! 不要求 Cookie/登录，全站按「每 IP 每小时约 20 次」限流。因此本网关没有
//! 账号/凭证池，只需要：监听地址、上游地址、代理池配置、模型目录开关。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// 每 IP 每小时限流（tryingopen 站点约束，用于代理选择/冷却语义）
    #[serde(default = "default_hourly_per_ip")]
    pub hourly_per_ip: usize,
    /// 单请求最大出口尝试轮数（超出后直连兜底）
    #[serde(default = "default_max_attempts")]
    pub max_attempts: usize,
    /// 代理池全局并发请求上限（同时打出去的不同出口数）
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
    /// 免费代理并发预检窗口
    #[serde(default = "default_precheck_concurrency")]
    pub precheck_concurrency: usize,
    /// 递增冷却秒数映射（逗号分隔；第 N 次使用后等待 X 秒）
    #[serde(default = "default_cooldown_map")]
    pub cooldown_map: String,
    /// 直连兜底（代理全部失败后最后试一次本机出口；上游每分钟 20 次本机配额）
    #[serde(default = "default_true")]
    pub direct_fallback: bool,
    #[serde(default = "default_sqlite")]
    pub sqlite_path: String,
    #[serde(default = "default_proxies_data")]
    pub proxies_path: String,
    #[serde(default = "default_telemetry")]
    pub telemetry_path: String,
    /// 跳过上游健康检查
    #[serde(default = "default_true")]
    pub skip_upstream_check: bool,
    /// 日志脱敏
    #[serde(default = "default_true")]
    pub redact_logs: bool,
    /// Web 面板访问密码（空=不鉴权；设置后 / 和 /ui 需 Basic Auth）
    #[serde(default)]
    pub ui_password: String,
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
fn default_precheck_concurrency() -> usize {
    50
}
fn default_cooldown_map() -> String {
    "0,15,60,120,300".into()
}
fn default_true() -> bool {
    true
}
fn default_sqlite() -> String {
    "data/tryingopen2api.sqlite".into()
}
fn default_proxies_data() -> String {
    "data/proxies.txt".into()
}
fn default_telemetry() -> String {
    "data/telemetry.sqlite".into()
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
            precheck_concurrency: default_precheck_concurrency(),
            cooldown_map: default_cooldown_map(),
            direct_fallback: default_true(),
            sqlite_path: default_sqlite(),
            proxies_path: default_proxies_data(),
            telemetry_path: default_telemetry(),
            skip_upstream_check: default_true(),
            ui_password: String::new(),
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
        if let Ok(v) = std::env::var("PRECHECK_CONCURRENCY") {
            cfg.precheck_concurrency = v.parse().unwrap_or(cfg.precheck_concurrency);
        }
        if let Ok(v) = std::env::var("DIRECT_FALLBACK") {
            cfg.direct_fallback = matches!(v.trim().to_lowercase().as_str(), "1" | "true");
            if let Ok(v) = std::env::var("UI_PASSWORD") {
                cfg.ui_password = v;
            }
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
