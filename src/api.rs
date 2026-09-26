//! HTTP API：OpenAI/Anthropic 兼容端点 + TryingOpen 上游桥接 + 代理轮换 + 面板
//!
//! 重试策略（代理轮换）：
//!   1) 每轮从代理池 acquire() 取一个出口（住宅优先，免费兜底）；
//!   2) 429/网络错误 → mark_failure + 指数退避 × 下一轮换出口；
//!   3) max_attempts 轮后仍失败 → 按 config.direct_fallback 直连兜底一次；
//!   4) 全部失败 → 429 ProviderRateLimited 兼容错误。

use crate::config::Config;
use crate::errors::ApiError;
use crate::models::{self, ModelRegistry};
use crate::protocol::anthropic_sse::AnthropicSseResponse;
use crate::protocol::openai_sse::{openai_events, SseResponse};
use crate::proxy_pool::ProxyPool;
use crate::session::SessionMap;
use crate::upstream::{MessagePart, StreamRequest, UpstreamClient, UpstreamMessage};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub client: Arc<UpstreamClient>,
    pub pool: Arc<ProxyPool>,
    pub registry: Arc<ModelRegistry>,
    pub sessions: Arc<SessionMap>,
    pub api_keys: Arc<std::sync::RwLock<Vec<String>>>,
    pub limiter: Arc<crate::prod_guard::RateLimiter>,
    /// 直连兜底专用配额（匿名上游约束，全局共享，防单 key 打满）
    pub direct_quota: Arc<crate::prod_guard::RateLimiter>,
    pub breaker: Arc<crate::prod_guard::CircuitBreaker>,
    pub metrics: Arc<crate::prod_guard::Metrics>,
    /// 每 key 用量统计（内存，有界）
    pub usage: Arc<crate::prod_guard::UsageTracker>,
}

pub fn build_router(state: AppState) -> Router {
    let mut router = Router::new()
        // 生产防护：限制请求体大小（多模态 base64 图/长文上限 16MB，防内存打爆）
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .route("/", get(handle_dashboard))
        .route("/ui", get(handle_dashboard))
        .route("/healthz", get(handle_healthz))
        .route("/v1/models", get(handle_v1_models))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/messages", post(handle_claude_messages))
        .route("/v1/responses", post(handle_responses))
        .route("/api/proxies", get(handle_proxies))
        .route("/api/proxies/refresh-free", post(handle_refresh_free))
        .route("/api/catalog/refresh", post(handle_catalog_refresh))
        .route("/api/guide", get(handle_guide))
        .route("/api/config/api-key", post(handle_config_api_key))
        .route("/api/usage", get(handle_usage));
    if state.cfg.metrics_enabled {
        router = router.route("/metrics", get(handle_metrics));
    }
    router.with_state(state)
}

// ---------- 认证 ----------

fn check_api_key(
    cfg: &Config,
    api_keys: &std::sync::RwLock<Vec<String>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let keys = api_keys.read().map(|g| g.clone()).unwrap_or_default();
    if keys.is_empty() && cfg.api_keys.is_empty() {
        return Ok(());
    }
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bearer = auth.strip_prefix("Bearer ").unwrap_or("").trim();
    let x_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if keys.iter().any(|k| k == bearer || k == x_key)
        || cfg.api_keys.iter().any(|k| k == bearer || k == x_key)
    {
        return Ok(());
    }
    Err(ApiError::unauthorized(
        "无效的 API Key。请在面板生成 Key 或配置 config.json 的 api_keys",
    ))
}

// ---------- 面板 ----------

/// Web 面板（/ 与 /ui）：配置 ui_password 后需 Basic Auth
async fn handle_dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let pass = state.cfg.ui_password.clone();
    if !pass.is_empty() {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let ok = if let Some(b64) = auth.strip_prefix("Basic ") {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            STANDARD
                .decode(b64.trim())
                .ok()
                .and_then(|d| String::from_utf8(d).ok())
                .map(|s| {
                    // 用户名任意，密码匹配 ui_password
                    s.split_once(':').map(|(_, p)| p == pass).unwrap_or(false)
                })
                .unwrap_or(false)
        } else {
            false
        };
        if !ok {
            return Response::builder()
                .status(axum::http::StatusCode::UNAUTHORIZED)
                .header(
                    "www-authenticate",
                    "Basic realm=\"TryingOpen2API\", charset=\"UTF-8\"",
                )
                .body(axum::body::Body::from("unauthorized"))
                .unwrap();
        }
    }
    // 面板已通过 UI 密码鉴权：把当前 API key 注入前端 JS，
    // 让 /v1/models、/api/proxies 等受保护端点在面板内自动带 x-api-key 访问
    let mut keys = state.api_keys.read().map(|g| g.clone()).unwrap_or_default();
    for k in &state.cfg.api_keys {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    // 默认配置（api_keys 为空）下面板首次打开会给所有数据端点 401：
    // 这里生成一次性会话级 key 注入前端，配合 check_api_key 的放行语义（空=本机模式）一起自愈
    if keys.is_empty() && state.cfg.api_keys.is_empty() {
        let session_key = format!("sk-to-session-{}", uuid::Uuid::new_v4().simple());
        if let Ok(mut w) = state.api_keys.write() {
            w.push(session_key.clone());
            keys.push(session_key);
        }
    }
    // 注入数量上限：只暴露前 N 个（防 key 过多时整页 HTML 膨胀/泄漏面）
    let keys_json = serde_json::to_string(&keys.iter().take(64).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".into());
    let html = crate::web::INDEX_HTML.replace("__API_KEYS_JSON__", &keys_json);
    Html(html).into_response()
}

async fn handle_healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    let model_count = state.registry.all().await.len();
    let proxy_count = state.pool.len().await;
    Json(
        json!({ "ok": true, "app": "tryingopen2api", "version": env!("CARGO_PKG_VERSION"),
        "upstream": state.cfg.upstream_base_url, "models": model_count, "proxies": proxy_count }),
    )
}

// ---------- 生产保护 helper ----------

async fn handle_metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let pool_size = state.pool.len().await;
    let snap = state.pool.snapshot(state.cfg.hourly_per_ip).await;
    let available = snap["available"].as_u64().unwrap_or(0) as usize;
    let sessions = state.sessions.len().await;
    let body = state.metrics.render(pool_size, available, sessions);
    Response::builder()
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(axum::body::Body::from(body))
        .unwrap()
}

/// 提取已认证的 API key（Bearer 或 x-api-key）
fn request_key(headers: &HeaderMap) -> Option<String> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bearer = auth.strip_prefix("Bearer ").unwrap_or("").trim();
    if !bearer.is_empty() {
        return Some(bearer.to_string());
    }
    let x = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if !x.is_empty() {
        return Some(x.to_string());
    }
    None
}

/// key 脱敏显示：保留前 4 + 后 4，中间掩码
fn mask_key(k: &str) -> String {
    let chars: Vec<char> = k.chars().collect();
    if chars.len() <= 8 {
        "***".to_string()
    } else {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}***{tail}")
    }
}

/// 记录每 key 用量（请求计数/成败/限流/上游错误/耗时）
fn record_usage(
    state: &AppState,
    key: Option<&str>,
    ok: bool,
    is_429: bool,
    is_5xx: bool,
    is_upstream_err: bool,
    elapsed: std::time::Duration,
) {
    // 匿名模式（api_keys 为空）无 key：统一记到哨兵 key，保证用量可见
    let k = key.unwrap_or("__anonymous__");
    {
        state.usage.record(k, |u| {
            u.requests += 1;
            if ok {
                u.ok += 1;
            } else if is_429 {
                u.rate_limited += 1;
            } else if is_5xx {
                u.errors_5xx += 1;
            } else {
                u.errors_4xx += 1;
            }
            if is_upstream_err {
                u.upstream_errors += 1;
            }
            u.duration_ms_total += elapsed.as_millis() as u64;
        });
    }
}

/// 429 响应（带 Retry-After）
fn rate_limited_response(e: ApiError, retry_after: u64) -> Response {
    let mut resp = (e.status(), e.openai_json()).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&retry_after.to_string()) {
        resp.headers_mut()
            .insert(axum::http::header::RETRY_AFTER, v);
    }
    resp
}

/// 熔断 OPEN：503
fn cb_open_response() -> Response {
    (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({
        "error": { "message": "上游服务暂不可用（熔断保护中），请稍后重试", "type": "upstream_error", "code": null }
    }))).into_response()
}

/// 请求日志：脱敏 key + 结构化字段（打开黑匣子）
/// redact_logs=true（默认）→ detail 截断到 300 字符且剥离疑似密钥；
/// redact_logs=false → 完整 detail（排障用，慎开）。
#[allow(clippy::too_many_arguments)]
fn log_request(
    redact: bool,
    endpoint: &str,
    key: Option<&str>,
    model: &str,
    stream: bool,
    status: &str,
    detail: Option<&str>,
    elapsed: std::time::Duration,
) {
    // 脱敏按 char 处理（不能用字节切片：多字节 key 会 panic）
    let masked = key
        .map(|k| {
            let chars: Vec<char> = k.chars().collect();
            if chars.len() <= 8 {
                "***".to_string()
            } else {
                let head: String = chars[..4].iter().collect();
                let tail: String = chars[chars.len() - 4..].iter().collect();
                format!("{head}***{tail}")
            }
        })
        .unwrap_or_else(|| "-".to_string());
    let ms = elapsed.as_millis();
    let d = detail.map(|s| {
        if redact {
            // 截断 + 剥离常见密钥形态（sk-xxx / Bearer token）
            let cut: String = s.chars().take(300).collect();
            let re = regex::Regex::new(r"(?i)(sk-[a-z0-9]{8,}|bearer\s+[a-z0-9]{8,})")
                .unwrap_or_else(|_| regex::Regex::new("$^").unwrap());
            re.replace_all(&cut, "***").into_owned()
        } else {
            s.to_string()
        }
    });
    // 结构化 JSON 行（便于日志采集/排障；detail 已按 redact 开关处理）
    let detail_json = d.unwrap_or_default();
    tracing::info!(
        "REQ {}",
        serde_json::json!({
            "event": "request",
            "endpoint": endpoint,
            "key": masked,
            "model": model,
            "stream": stream,
            "status": status,
            "took_ms": ms,
            "detail": detail_json,
        })
    );
}

/// 上游错误分类（用于 metrics）
fn upstream_error_kind(e: &ApiError) -> &'static str {
    match e {
        ApiError::RateLimited(_) => "rate_limited",
        ApiError::Upstream(msg) => {
            let m = msg.to_lowercase();
            if m.contains("timed out") || m.contains("timeout") {
                "timeout"
            } else if m.contains("429") {
                "rate_limited"
            } else {
                "network"
            }
        }
        _ => "other",
    }
}

// ---------- /v1/models ----------

async fn handle_v1_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let list = state.registry.all().await;
    let models = models::openai_models(&list);
    Json(json!({ "object": "list", "data": models })).into_response()
}

// ---------- OpenAI /v1/chat/completions ----------

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    pub user: Option<String>,
    /// 思考程度（balanced/deep/low 等，上游决定）
    #[serde(default = "default_effort")]
    pub effort: String,
}

fn default_effort() -> String {
    "balanced".into()
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: serde_json::Value,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

fn message_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                } else if part.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                    if let Some(t) = part.get("content").and_then(|v| v.as_str()) {
                        out.push_str(t);
                    }
                }
            }
            out
        }
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn render_tool_calls(calls: &serde_json::Value) -> String {
    let Some(arr) = calls.as_array() else {
        return String::new();
    };
    let mut out = String::new();
    for c in arr {
        let name = c
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|v| v.as_str())
            .or_else(|| c.get("name").and_then(|v| v.as_str()))
            .unwrap_or("tool");
        let args = c
            .get("function")
            .and_then(|f| f.get("arguments"))
            .or_else(|| c.get("arguments"))
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_else(|| "{}".into());
        out.push_str(&format!("\n[tool_call name={name} arguments={args}]"));
    }
    out
}

/// 上游按整段对话计长度（站点级限制，约 2 万汉字上限）。超预算时：
/// 优先丢弃最旧消息保最近的；若单条消息本身超长则硬截断该条文本，避免上游 413。
fn truncate_upstream_messages(msgs: &mut Vec<UpstreamMessage>, max_chars: usize) {
    fn msg_len(m: &UpstreamMessage) -> usize {
        m.parts
            .iter()
            .map(|p| p.text.as_deref().unwrap_or("").chars().count())
            .sum()
    }
    fn set_text(m: &mut UpstreamMessage, t: &str) {
        for p in m.parts.iter_mut() {
            if p.part_type == "text" {
                p.text = Some(t.to_string());
                return;
            }
        }
        m.parts.insert(
            0,
            MessagePart {
                part_type: "text".into(),
                text: Some(t.to_string()),
                media_type: None,
                url: None,
            },
        );
    }
    let total: usize = msgs.iter().map(msg_len).sum();
    if total <= max_chars {
        return;
    }
    // 从旧到新累计，标记需要丢弃的旧消息
    let mut drop_up_to = 0usize;
    let mut acc = 0usize;
    let count = msgs.len();
    let mut any_oversize = false;
    for (i, m) in msgs.iter().enumerate() {
        let n = msg_len(m);
        if n > max_chars {
            any_oversize = true;
        }
        if acc + n > max_chars {
            drop_up_to = i;
            break;
        }
        acc += n;
    }
    // 单条超长（含首条超长但后面还有消息的情况）：直接硬截断该条，避免 413
    if any_oversize || drop_up_to >= count.saturating_sub(1) {
        // 单条/近尾部超长：保留最近 1/2 并硬截断每条文本
        let keep = (msgs.len() / 2).max(1);
        let keep_start = msgs.len() - keep;
        let per = (max_chars / 2 / keep).max(1);
        let mut updates: Vec<(usize, String)> = Vec::new();
        for (idx, m) in msgs.iter().enumerate().skip(keep_start) {
            let t = m
                .parts
                .iter()
                .find(|p| p.part_type == "text")
                .and_then(|p| p.text.as_deref())
                .unwrap_or("");
            let cut: String = t.chars().take(per).collect();
            updates.push((idx, format!("{cut}…(内容过长已截断)")));
        }
        for (idx, t) in updates {
            if let Some(m) = msgs.get_mut(idx) {
                set_text(m, &t);
            }
        }
        if msgs.len() > keep {
            // 保留「最近 keep 条」（已截断），丢弃更旧的消息（不变量：不能截断后又删掉截断目标）
            msgs.drain(0..keep_start);
            msgs.insert(
                0,
                UpstreamMessage {
                    id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
                    role: "user".into(),
                    parts: vec![MessagePart {
                        part_type: "text".into(),
                        text: Some(
                            "[earlier messages omitted: chat was too long for the upstream]".into(),
                        ),
                        media_type: None,
                        url: None,
                    }],
                    metadata: None,
                },
            );
        }
        return;
    }
    // 丢弃最旧的 drop_up_to 条，保留剩余
    if drop_up_to > 0 {
        msgs.drain(0..drop_up_to);
        msgs.insert(
            0,
            UpstreamMessage {
                id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
                role: "user".into(),
                parts: vec![MessagePart {
                    part_type: "text".into(),
                    text: Some(
                        "[earlier messages omitted: chat was too long for the upstream]".into(),
                    ),
                    media_type: None,
                    url: None,
                }],
                metadata: None,
            },
        );
    }
}

fn extract_image_parts(messages: &[ChatMessage]) -> Vec<MessagePart> {
    let mut out = Vec::new();
    for m in messages {
        if let serde_json::Value::Array(arr) = &m.content {
            for part in arr {
                if let Some(t) = part.get("type").and_then(|v| v.as_str()) {
                    if t == "image_url" {
                        let url = part
                            .get("image_url")
                            .and_then(|v| v.as_str())
                            .or_else(|| {
                                part.get("image_url")
                                    .and_then(|v| v.get("url"))
                                    .and_then(|v| v.as_str())
                            })
                            .unwrap_or("");
                        if !url.is_empty() {
                            out.push(MessagePart {
                                part_type: "file".into(),
                                text: None,
                                media_type: Some(media_type(url)),
                                url: Some(url.to_string()),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

fn media_type(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("data:") {
        let header = rest.split(',').next().unwrap_or("");
        let mime = header.split(';').next().unwrap_or("").to_string();
        if !mime.is_empty() {
            return mime;
        }
    }
    let path = url.split('?').next().unwrap_or(url);
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg".into(),
        "png" => "image/png".into(),
        "webp" => "image/webp".into(),
        "gif" => "image/gif".into(),
        _ => "application/octet-stream".into(),
    }
}

/// 移除文本中已存在的 [SYSTEM INSTRUCTIONS]...[/SYSTEM INSTRUCTIONS] 块。
/// 多轮会话时历史首条 user 已含网关上一轮注入的系统块，重放回来会重复堆积，
/// 注入前先剥离，保证每个请求只有一份系统提示。
fn strip_system_block(text: &str) -> String {
    let start = "[SYSTEM INSTRUCTIONS]";
    let end = "[/SYSTEM INSTRUCTIONS]";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut removed = false;
    while let Some(i) = rest.find(start) {
        out.push_str(&rest[..i]);
        removed = true;
        match rest[i..].find(end) {
            Some(j) => {
                rest = &rest[i + j + end.len()..];
            }
            None => {
                // 未闭合的 system 块：丢弃到结尾
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    if removed {
        out
    } else {
        text.to_string()
    }
}

/// 组装上游请求体（与浏览器的 HAR 请求体同构）
fn build_upstream_request(
    model: &str,
    messages: &[ChatMessage],
    _images: Vec<MessagePart>,
    effort: &str,
    tools: Option<&serde_json::Value>,
    tool_choice: Option<&serde_json::Value>,
) -> StreamRequest {
    let mut converted: Vec<UpstreamMessage> = Vec::new();
    let mut system_texts: Vec<String> = Vec::new();
    // 工具调用模式：把工具定义注入系统提示（上游无原生 tool_calls，模型按纯文本 JSON 输出）
    if let Some(tools) = tools {
        if tools.as_array().map(|v| !v.is_empty()).unwrap_or(false) {
            let mut instr = format!("[TOOL CALLING MODE]\nAvailable tools (JSON): {}", tools);
            if let Some(tc) = tool_choice {
                instr.push_str(&format!("\nTool choice: {}", tc));
            }
            instr.push_str(
                "\nIf you need to call a tool, respond with ONLY a single JSON object and no other text, no markdown fences: {\"tool_call\":{\"name\":\"<exact tool name>\",\"arguments\":{...}}}",
            );
            system_texts.push(instr);
        }
    }
    for m in messages {
        if m.role == "system" || m.role == "developer" {
            let t = message_text(&m.content);
            if !t.is_empty() {
                system_texts.push(t);
            }
        }
    }
    for m in messages {
        let role_raw = m.role.as_str();
        if role_raw == "system" || role_raw == "developer" {
            continue;
        }
        let mut text = message_text(&m.content);
        if let Some(calls) = &m.tool_calls {
            text.push_str(&render_tool_calls(calls));
        }
        // 上游不接受 role=tool/function，否则回 Invalid messages。改写成 user 文本。
        let role = if role_raw == "assistant" {
            "assistant"
        } else {
            "user"
        };
        if role_raw == "tool" || role_raw == "function" {
            let name = m.name.clone().unwrap_or_else(|| "tool".into());
            let id = m.tool_call_id.clone().unwrap_or_default();
            text = format!("[tool_result name={name} id={id}]\n{text}");
        }
        let mut parts: Vec<MessagePart> = vec![MessagePart {
            part_type: "text".into(),
            text: Some(text),
            media_type: None,
            url: None,
        }];
        if role_raw == "user" {
            if let serde_json::Value::Array(arr) = &m.content {
                for part in arr {
                    if part.get("type").and_then(|v| v.as_str()) != Some("image_url") {
                        continue;
                    }
                    let url = part
                        .get("image_url")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            part.get("image_url")
                                .and_then(|v| v.get("url"))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("");
                    if !url.is_empty() {
                        parts.push(MessagePart {
                            part_type: "file".into(),
                            text: None,
                            media_type: Some(media_type(url)),
                            url: Some(url.to_string()),
                        });
                    }
                }
            }
        }
        if message_text(&m.content).trim().is_empty()
            && m.tool_calls.is_none()
            && parts.len() == 1
            && role_raw != "user"
        {
            continue;
        }
        converted.push(UpstreamMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
            role: role.to_string(),
            parts,
            metadata: None,
        });
    }
    truncate_upstream_messages(&mut converted, 16_000);
    // 系统提示 → 拼进第一条 user 的 [SYSTEM INSTRUCTIONS]（上游无 system 角色）
    if !system_texts.is_empty() {
        let sys = format!(
            "[SYSTEM INSTRUCTIONS]\n{}\n[/SYSTEM INSTRUCTIONS]",
            system_texts.join("\n\n")
        );
        if let Some(first_user) = converted.iter_mut().find(|m| m.role == "user") {
            // 幂等：剥离历史重放带来的旧系统块，只保留本请求注入的一份
            if let Some(first_text) = first_user.parts.first_mut() {
                if let Some(t) = first_text.text.take() {
                    first_text.text = Some(strip_system_block(&t));
                }
            }
            first_user.parts.insert(
                0,
                MessagePart {
                    part_type: "text".into(),
                    text: Some(sys),
                    media_type: None,
                    url: None,
                },
            );
        } else {
            converted.insert(
                0,
                UpstreamMessage {
                    id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
                    role: "user".into(),
                    parts: vec![MessagePart {
                        part_type: "text".into(),
                        text: Some(sys),
                        media_type: None,
                        url: None,
                    }],
                    metadata: None,
                },
            );
        }
    }
    if converted.is_empty() {
        converted.push(UpstreamMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
            role: "user".into(),
            parts: vec![MessagePart {
                part_type: "text".into(),
                text: Some("".into()),
                media_type: None,
                url: None,
            }],
            metadata: None,
        });
    }
    StreamRequest {
        msg_type: None,
        id: format!("chat-{}", uuid::Uuid::new_v4().simple()),
        trigger: "submit-message".into(),
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        model: model.to_string(),
        effort: effort.to_string(),
        messages: converted,
        stream: true,
    }
}

async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChatRequest>,
) -> Response {
    let start = std::time::Instant::now();
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        state
            .metrics
            .record_request("v1_chat_completions", "openai", "4xx");
        state
            .metrics
            .observe_duration(start.elapsed().as_secs_f64());
        log_request(
            state.cfg.redact_logs,
            "v1_chat_completions",
            request_key(&headers).as_deref(),
            body.model.as_str(),
            body.stream,
            "401",
            Some(e.message()),
            start.elapsed(),
        );
        return api_err_response(e);
    }
    // 每 API Key 限流（公网防滥用）
    if let Some(key) = request_key(&headers) {
        if let Err(retry_after) = state.limiter.check(&key) {
            state
                .metrics
                .record_request("v1_chat_completions", "openai", "429");
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            log_request(
                state.cfg.redact_logs,
                "v1_chat_completions",
                Some(key.as_str()),
                body.model.as_str(),
                body.stream,
                "429",
                Some(&format!("retry_after={retry_after}s")),
                start.elapsed(),
            );
            record_usage(
                &state,
                Some(key.as_str()),
                false,
                true,
                false,
                false,
                start.elapsed(),
            );
            return rate_limited_response(
                ApiError::rate_limited("请求过于频繁，请稍后重试"),
                retry_after,
            );
        }
    }
    // 上游熔断：OPEN 时直接 503
    if !state.breaker.allow() {
        state
            .metrics
            .record_request("v1_chat_completions", "openai", "5xx");
        state
            .metrics
            .observe_duration(start.elapsed().as_secs_f64());
        let rb_key = request_key(&headers);
        record_usage(
            &state,
            rb_key.as_deref(),
            false,
            false,
            true,
            true,
            start.elapsed(),
        );
        log_request(
            state.cfg.redact_logs,
            "v1_chat_completions",
            rb_key.as_deref(),
            body.model.as_str(),
            body.stream,
            "503",
            Some("circuit_breaker_open"),
            start.elapsed(),
        );
        return cb_open_response();
    }
    let created = chrono::Utc::now().timestamp();
    let model = state
        .registry
        .resolve(&body.model, &state.cfg.fallback_models)
        .await;

    let images = extract_image_parts(&body.messages);
    let tool_mode = body
        .tools
        .as_ref()
        .map(|t| t.as_array().map(|v| !v.is_empty()).unwrap_or(false))
        .unwrap_or(false);
    let req = build_upstream_request(
        &model,
        &body.messages,
        images,
        &body.effort,
        body.tools.as_ref(),
        body.tool_choice.as_ref(),
    );

    let thread_key = body
        .user
        .clone()
        .unwrap_or_else(|| "default-thread".to_string());
    state.sessions.ensure(&thread_key, &model).await;

    match try_rounds(&state, &req).await {
        Ok(up) => {
            state.breaker.record_success();
            state
                .metrics
                .record_request("v1_chat_completions", "openai", "2xx");
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            state.sessions.touch(&thread_key).await;
            if body.stream {
                let rb_key = request_key(&headers);
                record_usage(
                    &state,
                    rb_key.as_deref(),
                    true,
                    false,
                    false,
                    false,
                    start.elapsed(),
                );
                log_request(
                    state.cfg.redact_logs,
                    "v1_chat_completions",
                    request_key(&headers).as_deref(),
                    &model,
                    true,
                    "200",
                    Some("streaming-sse"),
                    start.elapsed(),
                );
                let s = openai_events(up, &model, created, tool_mode);
                let body = axum::body::Body::from_stream(s);
                SseResponse { body }.into_response()
            } else {
                let nr = collect_nonstream(up).await;
                if let Some(em) = nr.upstream_error.as_deref() {
                    state
                        .metrics
                        .record_request("v1_chat_completions", "openai", "5xx");
                    state.metrics.record_upstream_error("upstream_error_event");
                    state
                        .metrics
                        .observe_duration(start.elapsed().as_secs_f64());
                    log_request(
                        state.cfg.redact_logs,
                        "v1_chat_completions",
                        request_key(&headers).as_deref(),
                        &model,
                        false,
                        "502",
                        Some(em),
                        start.elapsed(),
                    );
                    record_usage(
                        &state,
                        request_key(&headers).as_deref(),
                        false,
                        false,
                        true,
                        true,
                        start.elapsed(),
                    );
                    return api_err_response(ApiError::upstream(format!(
                        "{}: {em}",
                        crate::upstream::describe_upstream_error(em)
                    )));
                }
                let rb_key = request_key(&headers);
                record_usage(
                    &state,
                    rb_key.as_deref(),
                    true,
                    false,
                    false,
                    false,
                    start.elapsed(),
                );
                log_request(
                    state.cfg.redact_logs,
                    "v1_chat_completions",
                    rb_key.as_deref(),
                    &model,
                    false,
                    "200",
                    Some(&format!("text={} tok", nr.text.chars().count())),
                    start.elapsed(),
                );
                // 非流式工具调用：若上游返回 tool_call JSON → 转标准 tool_calls message
                let body = if let Some(tc) = crate::protocol::openai_sse::detect_tool_call(&nr.text)
                {
                    let message = crate::protocol::openai_sse::nonstream_tool_message(&tc);
                    serde_json::to_string(&serde_json::json!({
                        "id": format!("chatcmpl-{}", created),
                        "object": "chat.completion",
                        "created": created,
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "message": message,
                            "finish_reason": "tool_calls"
                        }],
                        "usage": nr.usage.clone().unwrap_or(serde_json::json!({
                            "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0
                        }))
                    }))
                    .unwrap_or_default()
                } else {
                    crate::protocol::openai_sse_helper::openai_nonstream_full(
                        &nr.text,
                        &nr.reasoning,
                        nr.usage.as_ref(),
                        &model,
                        created,
                    )
                };
                Response::builder()
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(text_body(&body)))
                    .unwrap()
            }
        }
        Err(e) => {
            // 上游 model-not-found → 标记模型下线（避免继续请求已移除模型）
            if crate::upstream::is_model_not_found_error(e.message()) {
                state.registry.mark_offline(&model).await;
            }
            // 熔断：仅上游 5xx 故障计失败；4xx（模型不存在/参数错/限流）不计——
            // 避免客户端用已下线模型连打把整个上游熔断，正常模型也被 503
            if e.status().is_server_error() {
                state.breaker.record_failure();
            }
            let class = if e.status().is_client_error() {
                "4xx"
            } else {
                "5xx"
            };
            let is_mnf = crate::upstream::is_model_not_found_error(e.message());
            if is_mnf {
                tracing::info!("MODEL_OFFLINE model={model} err={}", e.message());
            }
            state
                .metrics
                .record_request("v1_chat_completions", "openai", class);
            state.metrics.record_upstream_error(upstream_error_kind(&e));
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            log_request(
                state.cfg.redact_logs,
                "v1_chat_completions",
                request_key(&headers).as_deref(),
                &model,
                body.stream,
                &e.status().as_str().replace(" ", ""),
                Some(e.message()),
                start.elapsed(),
            );
            let is_5xx = e.status().is_server_error();
            let is_429 = e.status().as_u16() == 429;
            record_usage(
                &state,
                request_key(&headers).as_deref(),
                false,
                is_429,
                is_5xx,
                is_5xx,
                start.elapsed(),
            );
            api_err_response(e)
        }
    }
}

/// 代理轮换重试：max_attempts 轮代理 + 1 次直连兜底
async fn try_rounds(state: &AppState, req: &StreamRequest) -> Result<reqwest::Response, ApiError> {
    let max = state.cfg.max_attempts.max(1);
    let cooldown = state.cfg.cooldown_vec();
    let hourly = state.cfg.hourly_per_ip;
    let mut last_err: Option<String> = None;

    for attempt in 0..max {
        let proxy = match state
            .pool
            .acquire(Some("residential"), hourly, &cooldown)
            .await
        {
            Some(u) => Some(u),
            None => state.pool.acquire(Some("free"), hourly, &cooldown).await,
        };
        let proxy_str = proxy.clone();
        let client = &state.client;
        match client.stream(req, proxy.as_deref()).await {
            Ok(resp) => {
                if let Some(u) = &proxy_str {
                    state.pool.mark_success(u).await;
                }
                tracing::debug!(
                    "PROXY_OK attempt={} model={} proxy={}",
                    attempt + 1,
                    req.model,
                    proxy_str.as_deref().unwrap_or("direct")
                );
                return Ok(resp);
            }
            Err(e) => {
                let msg = e.to_string();
                let is_429 = msg.starts_with("upstream-429");
                if let Some(u) = &proxy_str {
                    state.pool.mark_failure(u, is_429, &cooldown).await;
                }
                tracing::warn!(
                    "PROXY_FAIL attempt={} model={} proxy={} is429={} err={}",
                    attempt + 1,
                    req.model,
                    proxy_str.as_deref().unwrap_or("none"),
                    is_429,
                    msg
                );
                // 模型暂停/容量不足：不继续轮换白等，立即短路返回
                // （换更多出口也一样暂停，快速失败让客户端换模型）
                if crate::upstream::is_model_paused_error(&msg) {
                    return Err(ApiError::upstream(format!(
                        "模型当前暂停/容量不足，请换一个模型重试: {}",
                        msg
                    )));
                }
                // 429 + 上游建议 cheaperFallbackId → 后续轮次换到建议模型重试
                // （不在同一被限流出口上重试建议模型：上游按 IP 限流，同出口大概率仍 429）
                if is_429 {
                    if let Some(cf) = state
                        .registry
                        .meta(&req.model)
                        .await
                        .and_then(|m| m.cheaper_fallback)
                    {
                        if !cf.is_empty() && cf != req.model {
                            tracing::info!(
                                "MODEL_DOWNGRADE via cheaperFallback {} -> {} (attempt={})",
                                req.model,
                                cf,
                                attempt + 1
                            );
                            let mut fallback_req = req.clone();
                            fallback_req.model = cf.clone();
                            // 下一轮用建议模型 + 新出口重试（同轮不再发第二次请求，省配额）
                            return upgrade_retry(
                                state,
                                fallback_req,
                                max,
                                attempt + 1,
                                &cooldown,
                                hourly,
                            )
                            .await;
                        }
                    }
                }
                if crate::upstream::is_chat_too_long_error(&msg) {
                    return Err(ApiError::bad_request(
                        "上游拒绝：当前对话文本过长（HTTP 413）。请新开对话，或减少历史消息后再试。",
                    ));
                }
                last_err = Some(msg);
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt.min(3) as u32))).await;
            }
        }
    }
    // 直连兜底（受专用配额约束：匿名上游每 24h UTC 日约 20 次，全局共享防打满）
    if state.cfg.direct_fallback {
        if let Err(retry) = state.direct_quota.check("__global_direct__") {
            return Err(ApiError::rate_limited(format!(
                "直连兜底配额已用尽（每窗口 {} 次），请稍后重试或配置更多代理：retry_after={retry}s",
                state.cfg.direct_fallback_quota
            )));
        }
        match state.client.stream(req, None).await {
            Ok(resp) => {
                tracing::debug!("PROXY_OK attempt=direct model={} proxy=direct", req.model);
                return Ok(resp);
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::warn!(
                    "PROXY_FAIL attempt=direct model={} proxy=direct err={msg}",
                    req.model
                );
                if crate::upstream::is_model_paused_error(&msg) {
                    return Err(ApiError::upstream(format!(
                        "模型当前暂停/容量不足，请换一个模型重试: {msg}"
                    )));
                }
                if crate::upstream::is_chat_too_long_error(&msg) {
                    return Err(ApiError::bad_request(
                        "上游拒绝：当前对话文本过长（HTTP 413）。请新开对话，或减少历史消息后再试。",
                    ));
                }
                last_err = Some(msg);
            }
        }
    }
    let detail = last_err.unwrap_or_else(|| "全部出口失败".into());
    if detail.starts_with("upstream-429") {
        Err(ApiError::rate_limited(format!(
            "TryingOpen 全部出口限流中（每 IP 每 24h UTC 日约 {} 次）：{}",
            state.cfg.hourly_per_ip, detail
        )))
    } else {
        Err(ApiError::upstream(format!(
            "TryingOpen 上游调用失败（已轮换 {} 个出口 + 直连兜底）: {}",
            max, detail
        )))
    }
}

/// cheaperFallback 降级后的继续轮换：用建议模型从下一出口开始重试
/// （与 try_rounds 主循环同语义：住宅→免费→直连兜底，全部失败返回限流错误）
async fn upgrade_retry(
    state: &AppState,
    fallback_req: crate::upstream::StreamRequest,
    max: usize,
    start_attempt: usize,
    cooldown: &[u32],
    hourly: usize,
) -> Result<reqwest::Response, ApiError> {
    for attempt in start_attempt..max {
        let proxy = match state
            .pool
            .acquire(Some("residential"), hourly, cooldown)
            .await
        {
            Some(u) => Some(u),
            None => state.pool.acquire(Some("free"), hourly, cooldown).await,
        };
        match state.client.stream(&fallback_req, proxy.as_deref()).await {
            Ok(resp) => {
                if let Some(u) = &proxy {
                    state.pool.mark_success(u).await;
                }
                tracing::debug!(
                    "PROXY_OK attempt={} model={} proxy={} (cheaperFallback)",
                    attempt + 1,
                    fallback_req.model,
                    proxy.as_deref().unwrap_or("direct")
                );
                return Ok(resp);
            }
            Err(e) => {
                let msg = e.to_string();
                let is_429 = msg.starts_with("upstream-429");
                if let Some(u) = &proxy {
                    state.pool.mark_failure(u, is_429, cooldown).await;
                }
                if crate::upstream::is_model_paused_error(&msg)
                    || crate::upstream::is_chat_too_long_error(&msg)
                {
                    return Err(ApiError::upstream(format!(
                        "模型当前不可用（cheaperFallback 尝试 {}）: {}",
                        fallback_req.model, msg
                    )));
                }
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt.min(3) as u32))).await;
            }
        }
    }
    Err(ApiError::rate_limited(format!(
        "TryingOpen 全部出口限流中（含 cheaperFallback {}）：请稍后重试",
        fallback_req.model
    )))
}

/// 非流式收集结果
pub struct NonstreamResult {
    pub text: String,
    pub reasoning: String,
    pub usage: Option<serde_json::Value>,
    /// 上游流内是否出现 error 事件（应转为 HTTP 错误而非假成功）
    pub upstream_error: Option<String>,
}

fn tokens_from_metadata(meta: &serde_json::Value) -> Option<serde_json::Value> {
    let input = meta.get("inputTokens").and_then(|v| v.as_i64());
    let output = meta.get("outputTokens").and_then(|v| v.as_i64());
    let total = meta.get("totalTokens").and_then(|v| v.as_i64());
    let reasoning = meta.get("reasoningTokens").and_then(|v| v.as_i64());
    if input.is_none() && output.is_none() && total.is_none() {
        return None;
    }
    let mut u = serde_json::json!({
        "prompt_tokens": input.unwrap_or(0),
        "completion_tokens": output.unwrap_or(0),
        "total_tokens": total.unwrap_or(input.unwrap_or(0) + output.unwrap_or(0)),
    });
    if let Some(r) = reasoning {
        u["reasoning_tokens"] = serde_json::Value::from(r);
    }
    Some(u)
}

/// 非流式：收集上游所有 chunk 拼成完整文本 + 思考 + usage
async fn collect_nonstream(up: reqwest::Response) -> NonstreamResult {
    let reader = crate::protocol::stream::reader_with_bytes(up.bytes_stream());
    let mut reader = tokio::io::BufReader::new(reader);
    let mut line = String::new();
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut usage: Option<serde_json::Value> = None;
    let mut upstream_error: Option<String> = None;
    loop {
        line.clear();
        use tokio::io::AsyncBufReadExt;
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let t = line.trim();
        if let Some(data) = t.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                    "reasoning-delta" => {
                        if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
                            reasoning.push_str(d);
                        }
                    }
                    "text-delta" => {
                        if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
                            text.push_str(d);
                        }
                    }
                    "finish" => {
                        if let Some(meta) = v.get("messageMetadata") {
                            usage = tokens_from_metadata(meta);
                        }
                        break;
                    }
                    "error" => {
                        let em = v
                            .get("errorText")
                            .and_then(|x| x.as_str())
                            .unwrap_or("上游流错误")
                            .to_string();
                        upstream_error = Some(em);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    NonstreamResult {
        text,
        reasoning,
        usage,
        upstream_error,
    }
}

fn text_body(s: &str) -> String {
    s.to_string()
}

// ---------- Anthropic /v1/messages ----------

#[derive(Debug, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    #[serde(default)]
    pub messages: Vec<AnthropicMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub system: Option<serde_json::Value>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    /// 工具选择（如 {"type":"auto"} / {"type":"any"} / {"type":"tool","name":"x"}）
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// 思考程度（balanced/deep/low 等，上游决定）
    #[serde(default = "default_effort")]
    pub effort: String,
    /// Anthropic thinking 参数（如 {"type":"enabled","budget_tokens":N}）：
    /// enabled → effort 强制 deep；disabled / 未传 → 保持 effort
    #[serde(default)]
    pub thinking: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    #[serde(default)]
    pub content: serde_json::Value,
}

fn anthropic_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                }
            }
            out
        }
        _ => String::new(),
    }
}

fn anthropic_image_parts(messages: &[AnthropicMessage]) -> Vec<MessagePart> {
    let mut out = Vec::new();
    for m in messages {
        if let serde_json::Value::Array(arr) = &m.content {
            for part in arr {
                if part.get("type").and_then(|v| v.as_str()) == Some("image") {
                    if let Some(src) = part.get("source") {
                        if let Some(data) = src.get("data").and_then(|v| v.as_str()) {
                            let mt = src
                                .get("media_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image/png");
                            out.push(MessagePart {
                                part_type: "file".into(),
                                text: None,
                                media_type: Some(mt.to_string()),
                                url: Some(format!("data:{mt};base64,{data}")),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

async fn handle_claude_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnthropicRequest>,
) -> Response {
    let start = std::time::Instant::now();
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        state
            .metrics
            .record_request("v1_messages", "anthropic", "4xx");
        state
            .metrics
            .observe_duration(start.elapsed().as_secs_f64());
        log_request(
            state.cfg.redact_logs,
            "v1_messages",
            request_key(&headers).as_deref(),
            body.model.as_str(),
            body.stream,
            "401",
            Some(e.message()),
            start.elapsed(),
        );
        return api_err_response_anthropic(e);
    }
    if let Some(key) = request_key(&headers) {
        if let Err(retry_after) = state.limiter.check(&key) {
            state
                .metrics
                .record_request("v1_messages", "anthropic", "429");
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            log_request(
                state.cfg.redact_logs,
                "v1_messages",
                Some(key.as_str()),
                body.model.as_str(),
                body.stream,
                "429",
                Some(&format!("retry_after={retry_after}s")),
                start.elapsed(),
            );
            record_usage(
                &state,
                Some(key.as_str()),
                false,
                true,
                false,
                false,
                start.elapsed(),
            );
            return rate_limited_response(
                ApiError::rate_limited("请求过于频繁，请稍后重试"),
                retry_after,
            );
        }
    }
    if !state.breaker.allow() {
        state
            .metrics
            .record_request("v1_messages", "anthropic", "5xx");
        state
            .metrics
            .observe_duration(start.elapsed().as_secs_f64());
        let cb_key = request_key(&headers);
        record_usage(
            &state,
            cb_key.as_deref(),
            false,
            false,
            true,
            true,
            start.elapsed(),
        );
        log_request(
            state.cfg.redact_logs,
            "v1_messages",
            cb_key.as_deref(),
            body.model.as_str(),
            body.stream,
            "503",
            Some("circuit_breaker_open"),
            start.elapsed(),
        );
        return cb_open_response();
    }
    let model = state
        .registry
        .resolve(&body.model, &state.cfg.fallback_models)
        .await;

    let last_user = body.messages.iter().rev().find(|m| m.role == "user");
    let content = last_user
        .map(|m| anthropic_text(&m.content))
        .unwrap_or_default();
    if content.trim().is_empty() && anthropic_image_parts(&body.messages).is_empty() {
        state
            .metrics
            .record_request("v1_messages", "anthropic", "4xx");
        state
            .metrics
            .observe_duration(start.elapsed().as_secs_f64());
        return api_err_response_anthropic(ApiError::bad_request("消息内容为空"));
    }

    let thread_key = body
        .metadata
        .as_ref()
        .and_then(|m| m.get("thread_id").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "default-thread".to_string());
    state.sessions.ensure(&thread_key, &model).await;

    // 组装上游消息：系统 + 历史 user/assistant + 图片
    let tool_mode = body
        .tools
        .as_ref()
        .map(|t| t.as_array().map(|v| !v.is_empty()).unwrap_or(false))
        .unwrap_or(false);
    // Anthropic thinking 参数 → effort 映射：enabled 时强制 deep（Claude Code 语义）
    let thinking_enabled = crate::upstream::resolve_effort(body.thinking.as_ref(), "balanced")
        == "deep"
        && body
            .thinking
            .as_ref()
            .and_then(|t| t.get("type").and_then(|v| v.as_str()))
            .map(|ty| ty == "enabled")
            .unwrap_or(false);
    let effective_effort = crate::upstream::resolve_effort(body.thinking.as_ref(), &body.effort);
    let mut up_msgs: Vec<UpstreamMessage> = Vec::new();
    let mut system_texts: Vec<String> = Vec::new();
    if thinking_enabled {
        system_texts.push("请逐步推理（step-by-step），在最终回答中展示思考过程。".to_string());
    }
    if tool_mode {
        let mut instr = format!(
            "[TOOL CALLING MODE]\nAvailable tools (JSON): {}\nIf you need to call a tool, respond with ONLY a single JSON object and no other text, no markdown fences: {{\"tool_call\":{{\"name\":\"<exact tool name>\",\"arguments\":{{...}}}}}}",
            body.tools.as_ref().unwrap()
        );
        if let Some(tc) = &body.tool_choice {
            if tc.is_object() {
                instr.push_str(&format!("\nTool choice: {tc}"));
            }
        }
        system_texts.push(instr);
    }
    if let Some(sys) = &body.system {
        let t = anthropic_text(sys);
        if !t.is_empty() {
            system_texts.push(t);
        }
    }
    for m in &body.messages {
        if m.role == "system" {
            let t = anthropic_text(&m.content);
            if !t.is_empty() {
                system_texts.push(t);
            }
            continue;
        }
        let mut parts = vec![MessagePart {
            part_type: "text".into(),
            text: Some(anthropic_text(&m.content)),
            media_type: None,
            url: None,
        }];
        if m.role == "user" {
            // 只取当前消息自己的图片（不能扫描全会话：否则每个 user 消息都会拿到全部图片）
            for img in anthropic_image_parts(std::slice::from_ref(m)) {
                parts.push(img);
            }
        }
        up_msgs.push(UpstreamMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
            role: m.role.clone(),
            parts,
            metadata: None,
        });
    }
    if !system_texts.is_empty() {
        if let Some(first_user) = up_msgs.iter_mut().find(|m| m.role == "user") {
            // 幂等：剥离历史重放带来的旧系统块，只保留本请求注入的一份
            if let Some(first_text) = first_user.parts.first_mut() {
                if let Some(t) = first_text.text.take() {
                    first_text.text = Some(strip_system_block(&t));
                }
            }
            first_user.parts.insert(
                0,
                MessagePart {
                    part_type: "text".into(),
                    text: Some(format!(
                        "[SYSTEM INSTRUCTIONS]\n{}\n[/SYSTEM INSTRUCTIONS]",
                        system_texts.join("\n\n")
                    )),
                    media_type: None,
                    url: None,
                },
            );
        }
    }
    // 与 OpenAI 路径一致：长对话做上游长度截断，避免 413
    truncate_upstream_messages(&mut up_msgs, 16_000);
    let req = StreamRequest {
        msg_type: None,
        id: format!("chat-{}", uuid::Uuid::new_v4().simple()),
        trigger: "submit-message".into(),
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        model: model.clone(),
        effort: effective_effort,
        messages: up_msgs,
        stream: true,
    };

    match try_rounds(&state, &req).await {
        Ok(up) => {
            state.breaker.record_success();
            state
                .metrics
                .record_request("v1_messages", "anthropic", "2xx");
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            state.sessions.touch(&thread_key).await;
            if body.stream {
                let cb_key = request_key(&headers);
                record_usage(
                    &state,
                    cb_key.as_deref(),
                    true,
                    false,
                    false,
                    false,
                    start.elapsed(),
                );
                log_request(
                    state.cfg.redact_logs,
                    "v1_messages",
                    request_key(&headers).as_deref(),
                    &model,
                    true,
                    "200",
                    Some("streaming-sse"),
                    start.elapsed(),
                );
                let s = crate::protocol::anthropic_sse::anthropic_events(
                    up,
                    &model,
                    &thread_key,
                    tool_mode,
                );
                return AnthropicSseResponse {
                    body: axum::body::Body::from_stream(s),
                }
                .into_response();
            }
            let nr = collect_nonstream(up).await;
            if let Some(em) = nr.upstream_error.as_deref() {
                state
                    .metrics
                    .record_request("v1_messages", "anthropic", "5xx");
                state.metrics.record_upstream_error("upstream_error_event");
                state
                    .metrics
                    .observe_duration(start.elapsed().as_secs_f64());
                log_request(
                    state.cfg.redact_logs,
                    "v1_messages",
                    request_key(&headers).as_deref(),
                    &model,
                    false,
                    "502",
                    Some(em),
                    start.elapsed(),
                );
                record_usage(
                    &state,
                    request_key(&headers).as_deref(),
                    false,
                    false,
                    true,
                    true,
                    start.elapsed(),
                );
                return api_err_response_anthropic(ApiError::upstream(format!(
                    "{}: {em}",
                    crate::upstream::describe_upstream_error(em)
                )));
            }
            let mut content: Vec<serde_json::Value> = Vec::new();
            if !nr.reasoning.is_empty() {
                content.push(json!({ "type": "thinking", "thinking": nr.reasoning }));
            }
            // Anthropic 非流式工具调用：检测上游 tool_call JSON → 转 tool_use block
            let tool_used =
                if let Some(tc) = crate::protocol::openai_sse::detect_tool_call(&nr.text) {
                    content.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": serde_json::from_str::<serde_json::Value>(&tc.arguments_json)
                            .unwrap_or(serde_json::Value::Null)
                    }));
                    true
                } else {
                    content.push(json!({ "type": "text", "text": nr.text }));
                    false
                };
            // Anthropic usage 键名：prompt_tokens→input_tokens、completion_tokens→output_tokens
            let usage = nr
                .usage
                .as_ref()
                .map(|u| {
                    json!({
                        "input_tokens": u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0)),
                        "output_tokens": u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0)),
                        "total_tokens": u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                    })
                })
                .unwrap_or(json!({ "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }));
            let cb_key = request_key(&headers);
            record_usage(
                &state,
                cb_key.as_deref(),
                true,
                false,
                false,
                false,
                start.elapsed(),
            );
            log_request(
                state.cfg.redact_logs,
                "v1_messages",
                cb_key.as_deref(),
                &model,
                false,
                "200",
                Some(&format!("text={} tok", nr.text.chars().count())),
                start.elapsed(),
            );
            let resp = json!({
                "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
                "type": "message", "role": "assistant", "model": model,
                "content": content,
                "stop_reason": if tool_used { "tool_use" } else { "end_turn" },
                "stop_sequence": null,
                "usage": usage
            });
            Json(resp).into_response()
        }
        Err(e) => {
            if crate::upstream::is_model_not_found_error(e.message()) {
                state.registry.mark_offline(&model).await;
            }
            if e.status().is_server_error() {
                state.breaker.record_failure();
            }
            let class = if e.status().is_client_error() {
                "4xx"
            } else {
                "5xx"
            };
            if crate::upstream::is_model_not_found_error(e.message()) {
                tracing::info!("MODEL_OFFLINE model={model} err={}", e.message());
            }
            state
                .metrics
                .record_request("v1_messages", "anthropic", class);
            state.metrics.record_upstream_error(upstream_error_kind(&e));
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            log_request(
                state.cfg.redact_logs,
                "v1_messages",
                request_key(&headers).as_deref(),
                &model,
                body.stream,
                &e.status().as_str().replace(" ", ""),
                Some(e.message()),
                start.elapsed(),
            );
            let is_5xx = e.status().is_server_error();
            let is_429 = e.status().as_u16() == 429;
            record_usage(
                &state,
                request_key(&headers).as_deref(),
                false,
                is_429,
                is_5xx,
                is_5xx,
                start.elapsed(),
            );
            api_err_response_anthropic(e)
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResponsesRequest {
    model: String,
    #[serde(default)]
    input: serde_json::Value,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    tools: Option<serde_json::Value>,
    #[serde(default)]
    tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    max_output_tokens: Option<u64>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

async fn handle_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResponsesRequest>,
) -> Response {
    let start = std::time::Instant::now();
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    if let Some(key) = request_key(&headers) {
        if let Err(retry_after) = state.limiter.check(&key) {
            record_usage(
                &state,
                Some(key.as_str()),
                false,
                true,
                false,
                false,
                start.elapsed(),
            );
            return rate_limited_response(
                ApiError::rate_limited("请求过于频繁，请稍后重试"),
                retry_after,
            );
        }
    }
    if !state.breaker.allow() {
        let cb_key = request_key(&headers);
        record_usage(
            &state,
            cb_key.as_deref(),
            false,
            false,
            true,
            true,
            start.elapsed(),
        );
        return cb_open_response();
    }
    let created = chrono::Utc::now().timestamp();
    let resp_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
    let model = state
        .registry
        .resolve(&body.model, &state.cfg.fallback_models)
        .await;
    let tools = body
        .tools
        .as_ref()
        .map(crate::protocol::responses::normalize_tools);
    let msg_vals =
        crate::protocol::responses::input_to_messages(body.instructions.as_deref(), &body.input);
    let messages: Vec<ChatMessage> =
        serde_json::from_value(serde_json::Value::Array(msg_vals)).unwrap_or_default();
    let tool_mode = tools
        .as_ref()
        .map(|t| t.as_array().map(|v| !v.is_empty()).unwrap_or(false))
        .unwrap_or(false);
    let effort = body.effort.clone().unwrap_or_else(default_effort);
    let req = build_upstream_request(
        &model,
        &messages,
        Vec::new(),
        &effort,
        tools.as_ref(),
        body.tool_choice.as_ref(),
    );
    let _ = (body.max_output_tokens, body.user);
    match try_rounds(&state, &req).await {
        Ok(up) => {
            state.breaker.record_success();
            state
                .metrics
                .record_request("v1_responses", "openai", "2xx");
            state
                .metrics
                .observe_duration(start.elapsed().as_secs_f64());
            if body.stream {
                let inner =
                    crate::protocol::openai_sse::openai_events(up, &model, created, tool_mode);
                let wrapped = crate::protocol::responses::ResponsesSse {
                    inner,
                    acc: crate::protocol::responses::StreamAccum::default(),
                    id: resp_id,
                    model: model.clone(),
                };
                return Response::builder()
                    .header("content-type", "text/event-stream; charset=utf-8")
                    .header("cache-control", "no-cache")
                    .body(axum::body::Body::from_stream(wrapped))
                    .unwrap();
            }
            let nr = collect_nonstream(up).await;
            if let Some(em) = nr.upstream_error.as_deref() {
                state
                    .metrics
                    .record_request("v1_responses", "openai", "5xx");
                state.metrics.record_upstream_error("upstream_error_event");
                state
                    .metrics
                    .observe_duration(start.elapsed().as_secs_f64());
                log_request(
                    state.cfg.redact_logs,
                    "v1_responses",
                    request_key(&headers).as_deref(),
                    &model,
                    false,
                    "502",
                    Some(em),
                    start.elapsed(),
                );
                return api_err_response(ApiError::upstream(format!(
                    "{}: {em}",
                    crate::upstream::describe_upstream_error(em)
                )));
            }
            let tc = crate::protocol::openai_sse::detect_tool_call(&nr.text);
            let body = crate::protocol::responses::completed_response(
                &resp_id,
                &model,
                if tc.is_some() { "" } else { &nr.text },
                tc.as_ref().map(|t| t.name.as_str()),
                tc.as_ref().map(|t| t.arguments_json.as_str()),
                tc.as_ref().map(|t| t.id.as_str()),
                nr.usage.as_ref(),
            );
            let u_key = request_key(&headers);
            record_usage(
                &state,
                u_key.as_deref(),
                true,
                false,
                false,
                false,
                start.elapsed(),
            );
            log_request(
                state.cfg.redact_logs,
                "v1_responses",
                u_key.as_deref(),
                &model,
                false,
                "200",
                Some("responses"),
                start.elapsed(),
            );
            Json(body).into_response()
        }
        Err(e) => {
            if crate::upstream::is_model_not_found_error(e.message()) {
                state.registry.mark_offline(&model).await;
            }
            if e.status().is_server_error() {
                state.breaker.record_failure();
            }
            log_request(
                state.cfg.redact_logs,
                "v1_responses",
                request_key(&headers).as_deref(),
                &model,
                body.stream,
                "err",
                Some(e.message()),
                start.elapsed(),
            );
            let is_5xx = e.status().is_server_error();
            let is_429 = e.status().as_u16() == 429;
            record_usage(
                &state,
                request_key(&headers).as_deref(),
                false,
                is_429,
                is_5xx,
                is_5xx,
                start.elapsed(),
            );
            api_err_response(e)
        }
    }
}

// ---------- 代理池 / 目录 / 面板 ----------

async fn handle_proxies(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let snap = state.pool.snapshot(state.cfg.hourly_per_ip).await;
    Json(snap).into_response()
}

async fn handle_refresh_free(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    if !state.cfg.free_proxy_enabled {
        return api_err_response(ApiError::bad_request(
            "免费代理未开启（config free_proxy_enabled:true 或 FREE_PROXY_ENABLED=1）",
        ));
    }
    let n = crate::free_proxy::refresh_once(&state.pool).await;
    Json(json!({ "ok": true, "injected": n, "total": state.pool.len().await })).into_response()
}

async fn handle_catalog_refresh(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    match state.client.fetch_catalog().await {
        Ok(records) => {
            let n = state.registry.replace_from_parsed(records).await;
            Json(json!({ "ok": true, "models": n })).into_response()
        }
        Err(e) => api_err_response(ApiError::upstream(format!("目录刷新失败: {e}"))),
    }
}

/// GET /api/usage：每 key 用量统计（鉴权与 /api/* 一致）
async fn handle_usage(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let me = request_key(&headers);
    let all = state.usage.all();
    let mine = me.as_deref().and_then(|k| state.usage.get(k));
    let total_requests: u64 = all.iter().map(|(_, u)| u.requests).sum();
    let total_ok: u64 = all.iter().map(|(_, u)| u.ok).sum();
    let masked_me = me
        .as_deref()
        .map(mask_key)
        .unwrap_or_else(|| "-".to_string());
    Json(json!({
        "ok": true,
        "current_key": masked_me,
        "usage": mine.map(|u| json!({
            "requests": u.requests,
            "ok": u.ok,
            "errors_4xx": u.errors_4xx,
            "errors_5xx": u.errors_5xx,
            "rate_limited": u.rate_limited,
            "upstream_errors": u.upstream_errors,
            "duration_ms_total": u.duration_ms_total,
        })),
        "summary": {
            "tracked_keys": all.len(),
            "total_requests": total_requests,
            "total_ok": total_ok,
        }
    }))
    .into_response()
}

async fn handle_guide(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let models = state.registry.all().await;
    let ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    Json(json!({
        "listen_addr": state.cfg.listen_addr,
        "api_keys_configured": !state.cfg.api_keys.is_empty() || !state.api_keys.read().map(|g| g.is_empty()).unwrap_or(true),
        "models": ids,
        "proxy_count": state.pool.len().await,
        "base_url": format!("http://{}/v1", state.cfg.listen_addr),
        "upstream": state.cfg.upstream_base_url,
        "usage_total_requests": state.usage.all().iter().map(|(_, u)| u.requests).sum::<u64>(),
        "note": "完全匿名：无需 Cookie/Domain/Key（每 24h UTC 日约 20 次，代理池自动轮换）"
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyAction {
    pub action: String,
    #[serde(default)]
    pub key: Option<String>,
    /// 高风险操作（clear）的二次确认：必须显式 true
    #[serde(default)]
    pub admin_confirm: Option<bool>,
}
async fn handle_config_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ApiKeyAction>,
) -> Response {
    // 安全：管理操作必须携带一个有效 key（防止公网任意生成/清空 key 关闭鉴权）
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let Ok(mut keys) = state.api_keys.write() else {
        return api_err_response(ApiError::internal("锁错误"));
    };
    match body.action.as_str() {
        "generate" => {
            // 防滥用：动态 key 数量上限（避免无限自增 + 限流 map 无界）
            if keys.len() >= 64 {
                return api_err_response(ApiError::bad_request(
                    "动态 key 已达上限（64），请先删除/清空专用 key",
                ));
            }
            let key = format!("sk-to-{}", uuid::Uuid::new_v4().simple());
            keys.push(key.clone());
            Json(json!({ "ok": true, "key": key })).into_response()
        }
        "set" => {
            let k = body.key.clone().unwrap_or_default().trim().to_string();
            if k.is_empty() {
                return api_err_response(ApiError::bad_request("缺少 key"));
            }
            if !keys.contains(&k) {
                keys.push(k.clone());
            }
            Json(json!({ "ok": true, "key": k })).into_response()
        }
        "clear" => {
            // 高风险：清空会关闭鉴权（回到空 key 放行模式），必须显式二次确认，
            // 且当 config.json 配置了 api_keys 时不允许通过动态接口清空静态 key（防自锁）
            if !state.cfg.api_keys.is_empty() {
                return api_err_response(ApiError::bad_request(
                    "config.json 已配置静态 api_keys，禁止动态清空（防误关闭鉴权）",
                ));
            }
            if body.admin_confirm != Some(true) {
                return api_err_response(ApiError::bad_request(
                    "清空全部动态 key 将导致服务回到无鉴权状态；确认请输入 admin_confirm:true",
                ));
            }
            keys.clear();
            Json(json!({ "ok": true, "cleared": true })).into_response()
        }
        _ => api_err_response(ApiError::bad_request("action 必须为 generate/set/clear")),
    }
}

// ---------- 错误响应 ----------

fn api_err_response(e: ApiError) -> Response {
    let status = e.status();
    (status, e.openai_json()).into_response()
}

fn api_err_response_anthropic(e: ApiError) -> Response {
    let status = e.status();
    (status, e.anthropic_json()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream::{MessagePart, UpstreamMessage};

    #[test]
    fn strip_system_block_idempotent() {
        // 无系统块 → 原样
        assert_eq!(strip_system_block("hi there"), "hi there");
        // 单块 → 剥离
        let one = "[SYSTEM INSTRUCTIONS]\nkeep me\n[/SYSTEM INSTRUCTIONS]\nhello";
        assert_eq!(strip_system_block(one), "\nhello");
        // 多轮重放：旧块 + 新内容 → 只留新内容（幂等）
        let replayed = "[SYSTEM INSTRUCTIONS]\nold system\n[/SYSTEM INSTRUCTIONS]\nuser says hi";
        assert_eq!(strip_system_block(replayed), "\nuser says hi");
        // 多个块 → 全部剥离
        let multi = "a[SYSTEM INSTRUCTIONS]x[/SYSTEM INSTRUCTIONS]b[SYSTEM INSTRUCTIONS]y[/SYSTEM INSTRUCTIONS]c";
        assert_eq!(strip_system_block(multi), "abc");
        // 未闭合块 → 剥到结尾
        let unclosed = "head[SYSTEM INSTRUCTIONS]never closed";
        assert_eq!(strip_system_block(unclosed), "head");
    }

    fn msg(role: &str, text: &str) -> UpstreamMessage {
        UpstreamMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
            role: role.into(),
            parts: vec![MessagePart {
                part_type: "text".into(),
                text: Some(text.into()),
                media_type: None,
                url: None,
            }],
            metadata: None,
        }
    }

    fn total_len(msgs: &[UpstreamMessage]) -> usize {
        msgs.iter()
            .flat_map(|m| m.parts.iter())
            .map(|p| p.text.as_deref().unwrap_or("").chars().count())
            .sum()
    }

    #[test]
    fn truncate_first_message_oversize() {
        // 首条单条超长 + 后面还有消息：必须硬截断而非不处理
        let mut msgs = vec![
            msg("user", &"长".repeat(20_000)),
            msg("assistant", "短回复"),
        ];
        truncate_upstream_messages(&mut msgs, 16_000);
        assert!(total_len(&msgs) < 20_000, "超长应被截断到预算内");
        assert!(
            msgs.iter().flat_map(|m| m.parts.iter()).any(|p| p
                .text
                .as_deref()
                .unwrap_or("")
                .contains("已截断")),
            "应含截断标记"
        );
    }

    #[test]
    fn truncate_keeps_recent_when_over_budget() {
        // 整体超预算：丢最旧保留最近
        let mut msgs = vec![msg("user", "旧"), msg("assistant", "中"), msg("user", "新")];
        truncate_upstream_messages(&mut msgs, 100);
        assert!(total_len(&msgs) <= 100 + 200, "总长应受约束（含截断提示）");
        assert!(msgs
            .iter()
            .any(|m| m.parts[0].text.as_deref().unwrap_or("").contains("新")));
    }

    #[test]
    fn truncate_noop_when_within_budget() {
        let mut msgs = vec![msg("user", "hi"), msg("assistant", "hello")];
        truncate_upstream_messages(&mut msgs, 16_000);
        assert_eq!(total_len(&msgs), 7);
        assert_eq!(msgs.len(), 2);
    }
}
