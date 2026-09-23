//! OpenAI SSE 流：TryingOpen SSE → OpenAI chat.completions SSE
//!
//! 上游事件 → OpenAI delta：
//! - reasoning-delta → delta.reasoning_content（思考区）
//! - text-delta       → delta.content
//! - finish           → finish_reason（从 finishReason 读）
//! - error            → 上游错误文本追加

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct SseResponse {
    pub body: Body,
}

impl IntoResponse for SseResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(self.body)
            .unwrap()
    }
}

pub fn openai_events(
    upstream: reqwest::Response,
    model: &str,
    created: i64,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(
        upstream.bytes_stream(),
    ));
    OpenAiTransform {
        reader: Box::pin(reader),
        model: model.to_string(),
        created,
        finished: false,
        done_sent: false,
        saw_content: false,
        finish_reason: "stop".to_string(),
        pending_event: String::new(),
    }
}

struct OpenAiTransform {
    reader: Pin<Box<dyn tokio::io::AsyncBufRead + Send>>,
    model: String,
    created: i64,
    finished: bool,
    done_sent: bool,
    saw_content: bool,
    finish_reason: String,
    pending_event: String,
}

impl Stream for OpenAiTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished && !self.done_sent {
            self.done_sent = true;
            return Poll::Ready(Some(Ok(openai_done(
                &self.model,
                self.created,
                &self.finish_reason,
            ))));
        }
        if self.finished {
            return Poll::Ready(None);
        }
        loop {
            let mut line = String::new();
            let reader = &mut self.reader;
            let fut = reader.read_line(&mut line);
            tokio::pin!(fut);
            match fut.poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(0)) => {
                    self.finished = true;
                    self.done_sent = true;
                    if !self.saw_content {
                        return Poll::Ready(Some(Ok(openai_empty_error(
                            &self.model,
                            self.created,
                        ))));
                    }
                    return Poll::Ready(Some(Ok(openai_done(
                        &self.model,
                        self.created,
                        &self.finish_reason,
                    ))));
                }
                Poll::Ready(Ok(_)) => {
                    let line = line
                        .trim_end_matches('\n')
                        .trim_end_matches('\r')
                        .trim()
                        .to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(ev_name) = line.strip_prefix("event: ") {
                        self.pending_event = ev_name.trim().to_string();
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        match parse_event(data, &self.pending_event) {
                            Some(evt) => match evt.event.as_str() {
                                "reasoning-delta" => {
                                    let delta = evt
                                        .json
                                        .get("delta")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if delta.is_empty() {
                                        continue;
                                    }
                                    let frame = format!(
                                        "data: {}\n\n",
                                        serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{ "index": 0, "delta": { "role": "assistant", "reasoning_content": delta }, "finish_reason": null }]
                                        })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                "text-delta" => {
                                    let delta = evt
                                        .json
                                        .get("delta")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if delta.is_empty() {
                                        continue;
                                    }
                                    self.saw_content = true;
                                    let frame = format!(
                                        "data: {}\n\n",
                                        serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{ "index": 0, "delta": { "content": delta }, "finish_reason": null }]
                                        })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                "finish" => {
                                    self.finished = true;
                                    self.finish_reason = evt
                                        .json
                                        .get("finishReason")
                                        .and_then(|v| v.as_str())
                                        .filter(|s| !s.is_empty())
                                        .unwrap_or("stop")
                                        .to_string();
                                    let err = error_text(&evt.json);
                                    if self.finish_reason == "error" && !err.is_empty() {
                                        let frame = format!(
                                            "data: {}\n\n",
                                            serde_json::json!({
                                                "id": format!("chatcmpl-{}", self.created),
                                                "object": "chat.completion.chunk",
                                                "created": self.created,
                                                "model": self.model,
                                                "choices": [{ "index": 0, "delta": { "content": format!("\n\n[上游错误: {}]", err) }, "finish_reason": null }]
                                            })
                                        );
                                        return Poll::Ready(Some(Ok(frame)));
                                    }
                                    return Poll::Ready(Some(Ok(openai_done(
                                        &self.model,
                                        self.created,
                                        &self.finish_reason,
                                    ))));
                                }
                                "error" => {
                                    let msg = evt
                                        .json
                                        .get("errorText")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("上游流错误");
                                    self.saw_content = true;
                                    let frame = format!(
                                        "data: {}\n\n",
                                        serde_json::json!({
                                            "id": format!("chatcmpl-{}", self.created),
                                            "object": "chat.completion.chunk",
                                            "created": self.created,
                                            "model": self.model,
                                            "choices": [{ "index": 0, "delta": { "content": format!("\n\n[上游错误: {msg}]") }, "finish_reason": null }]
                                        })
                                    );
                                    return Poll::Ready(Some(Ok(frame)));
                                }
                                _ => continue,
                            },
                            None => continue,
                        }
                    }
                }
                Poll::Ready(Err(_)) => {
                    self.finished = true;
                    self.done_sent = true;
                    return Poll::Ready(Some(Ok(openai_done(&self.model, self.created, "stop"))));
                }
            }
        }
    }
}

fn error_text(json: &serde_json::Value) -> String {
    json.get("errorText")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("messageMetadata")
                .and_then(|m| m.get("errorText"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            json.get("messageMetadata")
                .and_then(|m| m.get("error"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string()
}

fn parse_event(data: &str, _event_name: &str) -> Option<Event> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    let event = json
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(Event { event, json })
}

struct Event {
    event: String,
    json: serde_json::Value,
}

fn openai_done(model: &str, created: i64, finish_reason: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "id": format!("chatcmpl-{}", created),
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{ "index": 0, "delta": {}, "finish_reason": finish_reason }]
        })
    ) + "data: [DONE]\n\n"
}

fn openai_empty_error(model: &str, created: i64) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "id": format!("chatcmpl-{}", created),
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{ "index": 0, "delta": { "content": "\n\n[上游无响应（可能被限流）]" }, "finish_reason": null }]
        })
    ) + "data: [DONE]\n\n"
}
