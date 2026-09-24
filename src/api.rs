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
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_dashboard))
        .route("/ui", get(handle_dashboard))
        .route("/healthz", get(handle_healthz))
        .route("/v1/models", get(handle_v1_models))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/v1/messages", post(handle_claude_messages))
        .route("/api/proxies", get(handle_proxies))
        .route("/api/proxies/refresh-free", post(handle_refresh_free))
        .route("/api/catalog/refresh", post(handle_catalog_refresh))
        .route("/api/guide", get(handle_guide))
        .route("/api/config/api-key", post(handle_config_api_key))
        .with_state(state)
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

async fn handle_dashboard() -> Html<String> {
    Html(crate::web::INDEX_HTML.to_string())
}

async fn handle_healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    let model_count = state.registry.all().await.len();
    let proxy_count = state.pool.len().await;
    Json(
        json!({ "ok": true, "app": "tryingopen2api", "version": env!("CARGO_PKG_VERSION"),
        "upstream": state.cfg.upstream_base_url, "models": model_count, "proxies": proxy_count }),
    )
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
}

fn message_text(content: &serde_json::Value) -> String {
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

/// 组装上游请求体（与浏览器的 HAR 请求体同构）
fn build_upstream_request(
    model: &str,
    messages: &[ChatMessage],
    images: Vec<MessagePart>,
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
        if m.role == "system" {
            let t = message_text(&m.content);
            if !t.is_empty() {
                system_texts.push(t);
            }
        }
    }
    for m in messages {
        let role = m.role.as_str();
        if role == "system" {
            continue;
        }
        let text = message_text(&m.content);
        let mut parts: Vec<MessagePart> = vec![MessagePart {
            part_type: "text".into(),
            text: Some(text),
            media_type: None,
            url: None,
        }];
        if role == "user" {
            for img in &images {
                parts.push(img.clone());
            }
        }
        converted.push(UpstreamMessage {
            id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
            role: role.to_string(),
            parts,
            metadata: None,
        });
    }
    // 系统提示 → 拼进第一条 user 的 [SYSTEM INSTRUCTIONS]（上游无 system 角色）
    if !system_texts.is_empty() {
        let sys = format!(
            "[SYSTEM INSTRUCTIONS]\n{}\n[/SYSTEM INSTRUCTIONS]",
            system_texts.join("\n\n")
        );
        if let Some(first_user) = converted.iter_mut().find(|m| m.role == "user") {
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
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response(e);
    }
    let created = chrono::Utc::now().timestamp();
    let model = state.registry.resolve(&body.model).await;

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
            state.sessions.touch(&thread_key).await;
            if body.stream {
                let s = openai_events(up, &model, created, tool_mode);
                let body = axum::body::Body::from_stream(s);
                SseResponse { body }.into_response()
            } else {
                let nr = collect_nonstream(up).await;
                let body = crate::protocol::openai_sse_helper::openai_nonstream_full(
                    &nr.text,
                    &nr.reasoning,
                    nr.usage.as_ref(),
                    &model,
                    created,
                );
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
                return Ok(resp);
            }
            Err(e) => {
                let msg = e.to_string();
                let is_429 = msg.starts_with("upstream-429");
                if let Some(u) = &proxy_str {
                    state.pool.mark_failure(u, is_429, &cooldown).await;
                }
                last_err = Some(msg);
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt.min(3) as u32))).await;
            }
        }
    }
    // 直连兜底
    if state.cfg.direct_fallback {
        match state.client.stream(req, None).await {
            Ok(resp) => return Ok(resp),
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    let detail = last_err.unwrap_or_else(|| "全部出口失败".into());
    if detail.starts_with("upstream-429") {
        Err(ApiError::rate_limited(format!(
            "TryingOpen 全部出口限流中（每 IP 每小时约 {} 次）：{}",
            state.cfg.hourly_per_ip, detail
        )))
    } else {
        Err(ApiError::upstream(format!(
            "TryingOpen 上游调用失败（已轮换 {} 个出口 + 直连兜底）: {}",
            max, detail
        )))
    }
}

/// 非流式收集结果
pub struct NonstreamResult {
    pub text: String,
    pub reasoning: String,
    pub usage: Option<serde_json::Value>,
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
                    "error" => break,
                    _ => {}
                }
            }
        }
    }
    NonstreamResult {
        text,
        reasoning,
        usage,
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
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// 思考程度（balanced/deep/low 等，上游决定）
    #[serde(default = "default_effort")]
    pub effort: String,
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
    if let Err(e) = check_api_key(&state.cfg, &state.api_keys, &headers) {
        return api_err_response_anthropic(e);
    }
    let model = state.registry.resolve(&body.model).await;

    let last_user = body.messages.iter().rev().find(|m| m.role == "user");
    let content = last_user
        .map(|m| anthropic_text(&m.content))
        .unwrap_or_default();
    if content.trim().is_empty() && anthropic_image_parts(&body.messages).is_empty() {
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
    let mut up_msgs: Vec<UpstreamMessage> = Vec::new();
    let mut system_texts: Vec<String> = Vec::new();
    if tool_mode {
        let instr = format!(
            "[TOOL CALLING MODE]\nAvailable tools (JSON): {}\nIf you need to call a tool, respond with ONLY a single JSON object and no other text, no markdown fences: {{\"tool_call\":{{\"name\":\"<exact tool name>\",\"arguments\":{{...}}}}}}",
            body.tools.as_ref().unwrap()
        );
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
            for img in anthropic_image_parts(&body.messages) {
                parts.push(img.clone());
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
    let req = StreamRequest {
        msg_type: None,
        id: format!("chat-{}", uuid::Uuid::new_v4().simple()),
        trigger: "submit-message".into(),
        message_id: format!("msg-{}", uuid::Uuid::new_v4().simple()),
        model: model.clone(),
        effort: body.effort.clone(),
        messages: up_msgs,
        stream: true,
    };

    match try_rounds(&state, &req).await {
        Ok(up) => {
            state.sessions.touch(&thread_key).await;
            if body.stream {
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
            let mut content: Vec<serde_json::Value> = Vec::new();
            if !nr.reasoning.is_empty() {
                content.push(json!({ "type": "thinking", "thinking": nr.reasoning }));
            }
            content.push(json!({ "type": "text", "text": nr.text }));
            let usage = nr.usage.clone().unwrap_or_else(
                || json!({ "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }),
            );
            let resp = json!({
                "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
                "type": "message", "role": "assistant", "model": model,
                "content": content,
                "stop_reason": "end_turn", "stop_sequence": null,
                "usage": usage
            });
            Json(resp).into_response()
        }
        Err(e) => {
            if crate::upstream::is_model_not_found_error(e.message()) {
                state.registry.mark_offline(&model).await;
            }
            api_err_response_anthropic(e)
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
        "note": "完全匿名：无需 Cookie/Domain/Key（每 IP 每小时约 20 次，代理池自动轮换）"
    })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyAction {
    pub action: String,
    #[serde(default)]
    pub key: Option<String>,
}

async fn handle_config_api_key(
    State(state): State<AppState>,
    Json(body): Json<ApiKeyAction>,
) -> Response {
    let Ok(mut keys) = state.api_keys.write() else {
        return api_err_response(ApiError::internal("锁错误"));
    };
    match body.action.as_str() {
        "generate" => {
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
            keys.clear();
            Json(json!({ "ok": true })).into_response()
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
