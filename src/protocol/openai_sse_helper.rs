//! OpenAI 非流式响应辅助

/// 非流式 chat.completion 响应体
pub fn openai_nonstream(text: &str, model: &str, created: i64) -> String {
    serde_json::json!({
        "id": format!("chatcmpl-{}", created),
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
    }).to_string()
}
