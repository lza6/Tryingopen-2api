//! Anthropic SSE 流：TryingOpen SSE → Anthropic /v1/messages 事件流
//!
//! - reasoning-delta → content_block_delta thinking_delta（思考区）
//! - text-delta       → content_block_delta text_delta
//! - finish           → message_delta + message_stop

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct AnthropicSseResponse {
    pub body: Body,
}

impl IntoResponse for AnthropicSseResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(self.body)
            .unwrap()
    }
}

pub fn anthropic_events(
    upstream: reqwest::Response,
    _model: &str,
    _session_id: &str,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(upstream.bytes_stream()));
    AnthropicTransform {
        reader: Box::pin(reader),
        finished: false,
        pending_event: String::new(),
    }
}

struct AnthropicTransform {
    reader: Pin<Box<dyn tokio::io::AsyncBufRead + Send>>,
    finished: bool,
    pending_event: String,
}

impl Stream for AnthropicTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
                    return Poll::Ready(Some(Ok(self.stop_events())));
                }
                Poll::Ready(Ok(_)) => {
                    let line = line.trim_end_matches('\n').trim_end_matches('\r').trim().to_string();
                    if line.is_empty() { continue; }
                    if let Some(ev_name) = line.strip_prefix("event: ") {
                        self.pending_event = ev_name.trim().to_string();
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        let json: serde_json::Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let event = json.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        match event.as_str() {
                            "reasoning-delta" => {
                                let delta = json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                if delta.is_empty() { continue; }
                                let frame = format!(
                                    "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\n",
                                    serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
                                    serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":delta}})
                                );
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            "text-delta" => {
                                let delta = json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                if delta.is_empty() { continue; }
                                let frame = format!(
                                    "event: content_block_delta\ndata: {}\n\n",
                                    serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":delta}})
                                );
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            "finish" => {
                                self.finished = true;
                                return Poll::Ready(Some(Ok(self.stop_events())));
                            }
                            "error" => {
                                let msg = json.get("errorText").and_then(|v| v.as_str()).unwrap_or("上游流错误");
                                let frame = format!(
                                    "event: content_block_delta\ndata: {}\n\n",
                                    serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":format!("\n\n[上游错误: {msg}]")}})
                                );
                                self.finished = true;
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            _ => continue,
                        }
                    }
                }
                Poll::Ready(Err(_)) => {
                    self.finished = true;
                    return Poll::Ready(Some(Ok(self.stop_events())));
                }
            }
        }
    }
}

impl AnthropicTransform {
    fn stop_events(&self) -> String {
        format!(
            "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
            serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":0}}),
            serde_json::json!({"type":"message_stop"})
        )
    }
}
