//! OpenAI 非流式响应辅助

/// 非流式 chat.completion 响应体（含 reasoning_content / usage）
pub fn openai_nonstream_full(
    text: &str,
    reasoning: &str,
    usage: Option<&serde_json::Value>,
    model: &str,
    created: i64,
) -> String {
    let mut message = serde_json::json!({
        "role": "assistant",
        "content": text,
    });
    if !reasoning.is_empty() {
        message["reasoning_content"] = serde_json::Value::String(reasoning.to_string());
    }
    let mut resp = serde_json::json!({
        "id": format!("chatcmpl-{}", created),
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }],
        "usage": usage.cloned().unwrap_or(serde_json::json!({
            "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0
        }))
    });
    if let Some(u) = usage {
        resp["usage"] = u.clone();
    }
    resp.to_string()
}

/// 兼容旧调用（无 reasoning/usage）
pub fn openai_nonstream(text: &str, model: &str, created: i64) -> String {
    openai_nonstream_full(text, "", None, model, created)
}
