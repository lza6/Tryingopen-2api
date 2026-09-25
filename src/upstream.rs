//! 上游 TryingOpen HTTP 客户端
//!
//! 逆向自抓包数据包（www.tryingopen.com.har）+ 站点 JS chunk：
//! - POST /api/open                匿名对话 SSE（无 Cookie；单 IP 每 24h UTC 日约 20 次）
//! - GET  /                        首页 HTML（含 /_next/static/chunks/*.js 引用）
//! - GET  /_next/static/chunks/*.js 模型目录（{id,name,context,supportsTools,supportsImages,pricePerMTok,...}）
//!
//! 认证：不需要。必须带 origin/referer/user-agent（站点校验浏览器语义）。
//! 单次请求体：{id, trigger:"submit-message", messageId, model, effort, messages, stream:true}
//! SSE 事件：start / start-step / reasoning-start / reasoning-delta / reasoning-end /
//!            text-start / text-delta / text-end / finish-step / finish / error

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DESKTOP_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "mediaType", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamMessage {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub parts: Vec<MessagePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRequest {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub msg_type: Option<String>,
    pub id: String,
    pub trigger: String,
    #[serde(rename = "messageId")]
    pub message_id: String,
    pub model: String,
    pub effort: String,
    pub messages: Vec<UpstreamMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct UpstreamClient {
    pub base_url: String,
    pub http: reqwest::Client,
}

impl UpstreamClient {
    pub fn new(base_url: &str, proxy: Option<&str>, timeout: Duration) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(timeout)
            .pool_idle_timeout(Duration::from_secs(90))
            .cookie_store(false)
            .user_agent(DESKTOP_UA)
            .default_headers(default_headers(base_url));
        if let Some(p) = proxy {
            builder = builder.proxy(reqwest::Proxy::all(p)?);
        }
        let http = builder.build()?;
        let base_url = base_url.trim_end_matches('/').to_string();
        Ok(Self { base_url, http })
    }

    pub async fn check_health(&self) -> Result<()> {
        let url = format!("{}/", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("上游健康检查失败")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(anyhow!("上游健康检查 HTTP {}", resp.status()))
        }
    }

    /// 对话流：返回 SSE 响应体（由上层逐行转换）。proxy=None 表示直连。
    pub async fn stream(
        &self,
        req: &StreamRequest,
        proxy: Option<&str>,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/api/open", self.base_url);
        let mut client = &self.http;
        let owned = if proxy.is_some() {
            let b = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(120))
                .user_agent(DESKTOP_UA)
                .default_headers(default_headers(&self.base_url));
            let b = if let Some(p) = proxy {
                match reqwest::Proxy::all(p) {
                    Ok(pr) => b.proxy(pr),
                    Err(_) => b,
                }
            } else {
                b
            };
            Some(b.build()?)
        } else {
            None
        };
        if let Some(c) = &owned {
            client = c;
        }
        let resp = client
            .post(&url)
            .json(req)
            .send()
            .await
            .context("上游对话流请求失败")?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("upstream-429: {}", truncate(&text, 300)));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "上游对话流失败 HTTP {status}: {}",
                truncate(&text, 400)
            ));
        }
        Ok(resp)
    }

    /// 抓取首页 + 模型目录 chunk，返回全部模型记录（供 ModelRegistry 替换）
    pub async fn fetch_catalog(&self) -> Result<Vec<crate::models::ModelMeta>> {
        let home = self
            .http
            .get(format!("{}/", self.base_url))
            .send()
            .await
            .context("拉取首页失败")?;
        if !home.status().is_success() {
            return Err(anyhow!("首页 HTTP {}", home.status()));
        }
        let html = home.text().await.unwrap_or_default();
        // 提取 chunk 路径
        let chunk_re = regex::Regex::new(r#"/_next/static/chunks/[A-Za-z0-9._~\-]+\.js"#).unwrap();
        let mut paths = Vec::new();
        for cap in chunk_re.find_iter(&html) {
            let p = cap.as_str().to_string();
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        if paths.is_empty() {
            return Err(anyhow!("首页未发现 chunk 路径"));
        }
        let mut all: Vec<crate::models::ModelMeta> = Vec::new();
        for p in paths {
            let url = format!("{}{}", self.base_url, p);
            let resp = self.http.get(&url).send().await?;
            if !resp.status().is_success() {
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            if !text.contains("supportsTools") {
                continue;
            }
            let parsed = parse_catalog_chunk(&text);
            if !parsed.is_empty() {
                all.extend(parsed);
            }
        }
        // 去重
        let mut seen = std::collections::HashSet::new();
        all.retain(|m| seen.insert(m.id.clone()));
        if all.is_empty() {
            return Err(anyhow!("未解析到模型目录"));
        }
        Ok(all)
    }
}

/// 用与 providers/tryingopen 相同的正则从 JS chunk 提取模型目录
pub fn parse_catalog_chunk(chunk: &str) -> Vec<crate::models::ModelMeta> {
    let re_price = regex::Regex::new(r#"pricePerMTok:([0-9.]+)"#).unwrap();
    let re = regex::Regex::new(
        r#"(?s)\{id:"([a-z0-9][a-z0-9.\-]*/[a-z0-9][a-z0-9.\-]*)",name:"([^"]+)".*?\}"#,
    )
    .unwrap();
    let mut out = Vec::new();
    for cap in re.captures_iter(chunk) {
        let id = cap[1].to_string();
        let name = cap[2].to_string();
        // 用平衡括号切出这条记录
        let Some(seg) = balanced_segment(chunk, cap.get(0).unwrap().start()) else {
            continue;
        };
        let field = |k: &str| -> Option<String> {
            let r = regex::Regex::new(&format!(r#""{}":"([^"]*)""#, regex::escape(k))).ok()?;
            Some(r.captures(seg)?.get(1)?.as_str().to_string())
        };
        let flag = |k: &str| seg.contains(&format!("{k}:!0")) || seg.contains(&format!("{k}:true"));
        let price = re_price
            .captures(seg)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());
        let ctx = field("context").unwrap_or_else(|| "128k".into());
        let price_f = price.and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        out.push(crate::models::ModelMeta {
            id,
            label: name,
            family: field("maker").unwrap_or_else(|| "tryingopen".into()),
            context: ctx.clone(),
            context_window: crate::models::parse_ctx(&ctx),
            price_per_mtok: price_f,
            tools: flag("supportsTools"),
            vision: flag("supportsImages"),
            source: "dynamic".into(),
        });
    }
    out
}

/// 平衡括号切出 { ... } 记录
fn balanced_segment(text: &str, start: usize) -> Option<&str> {
    // 从 start 处 '{' 开始配对
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut end = start;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if end > start {
        Some(&text[start..=end])
    } else {
        None
    }
}

fn default_headers(base_url: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE};
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        "accept-language",
        HeaderValue::from_static("zh-CN,zh;q=0.9"),
    );
    h.insert(ACCEPT, HeaderValue::from_static("*/*"));
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    // 浏览器语义：origin/referer 必须是站点本身
    if let Ok(v) = HeaderValue::from_str(base_url) {
        h.insert("origin", v);
    }
    if let Ok(v) = HeaderValue::from_str(&format!("{base_url}/")) {
        h.insert("referer", v);
    }
    h
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        // 按字符边界截断（避免 &s[..n] 在 UTF-8 中文字符中间 panic）
        let mut end = n;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// 判断错误是否「模型不存在」：
/// - 优先匹配「HTTP 4xx + 模型语义词 + 不存在词」（覆盖 tryingopen 真实报错）
/// - 也直接匹配「模型 + 不存在」纯语义（上游可能不带 HTTP 前缀）
pub fn is_model_not_found_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    let model_kw = lower.contains("model") || lower.contains("模型");
    if !model_kw {
        return false;
    }
    let not_found_kw = [
        "not found",
        "不存在",
        "invalid",
        "无效",
        "unknown",
        "not_found",
        // tryingopen.com 真实返回：That model isn't on this page.
        "isn't on this page",
        "not on this page",
        "does not exist",
        "no such model",
        "not available",
        "not supported",
        "not_exist",
    ]
    .iter()
    .any(|k| lower.contains(k));
    if !not_found_kw {
        return false;
    }
    // 有明确 5xx 状态码 → 不是模型问题（服务端故障）
    let http_5xx =
        lower.contains("http 5") || lower.contains("upstream-5") || lower.contains("status 5");
    if http_5xx {
        return false;
    }
    // 命中「模型 + 不存在」：允许 4xx 前缀或无前缀（上游中文错误可能无 HTTP 前缀）
    true
}

/// 判断错误是否「模型暂停 / 容量不足」（临时不可用，非永久下线）
/// tryingopen 真实返回：GLM 5.2 is paused while we bring up more capacity. Pick another model to keep going.
/// Chat too long / HTTP 413: retrying other exits cannot help.
pub fn is_chat_too_long_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("payload too large")
        || lower.contains("http 413")
        || lower.contains(" 413 ")
        || lower.contains("too much text")
        || lower.contains("start a new one")
        || lower.contains("对话太长")
        || lower.contains("内容过长")
}

pub fn is_model_paused_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    let paused_kw = [
        "is paused",
        "paused while",
        "bring up more capacity",
        "at capacity",
        "overloaded",
        "temporarily unavailable",
        "too much traffic",
        "容量不足",
        "暂时不可用",
        "已暂停",
    ]
    .iter()
    .any(|k| lower.contains(k));
    paused_kw
}

#[cfg(test)]
mod tests {
    use super::{is_chat_too_long_error, is_model_not_found_error, is_model_paused_error};

    #[test]
    fn model_not_found_tryingopen_that_model_isnt_on_page() {
        // 生产真实返回（anthropic/claude-sonnet-5 已下线）
        let err =
            "上游对话流失败 HTTP 400 Bad Request: {\"error\":\"That model isn't on this page.\"}";
        assert!(is_model_not_found_error(err), "应识别为模型不存在: {err}");
    }

    #[test]
    fn model_not_found_common_phrases() {
        assert!(is_model_not_found_error("HTTP 404 model not found"));
        assert!(is_model_not_found_error("模型不存在"));
    }

    #[test]
    fn model_not_found_does_not_match_unrelated_errors() {
        assert!(!is_model_not_found_error("upstream timeout"));
        assert!(!is_model_not_found_error("HTTP 500 internal error"));
        assert!(!is_model_not_found_error("upstream-429 rate limited"));
    }

    #[test]
    fn model_paused_tryingopen_glm() {
        // 生产真实返回：GLM 5.2 is paused while we bring up more capacity
        let err = "上游对话流失败 HTTP 400 Bad Request: {\"error\":\"GLM 5.2 is paused while we bring up more capacity. Pick another model to keep going.\"}";
        assert!(is_model_paused_error(err), "应识别为模型暂停: {err}");
        // 非暂停错误不误判
        assert!(!is_model_paused_error("That model isn't on this page."));
        assert!(!is_model_paused_error("upstream timeout"));
    }

    #[test]
    fn chat_too_long_413() {
        let err = "HTTP 413 Payload Too Large: too much text in this chat now. Start a new one";
        assert!(is_chat_too_long_error(err));
        assert!(!is_chat_too_long_error("upstream timeout"));
    }
}
