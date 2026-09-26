//! Anthropic SSE 流：TryingOpen SSE → Anthropic /v1/messages 事件流
//!
//! - reasoning-delta → content_block_delta thinking_delta（思考区）
//! - text-delta       → content_block_delta text_delta
//! - 工具模式：text-delta 累积成 {"tool_call":{...}} JSON 后转 tool_use
//!   content_block_start（id/name/input）→ input_json_delta（partial_json 分块）→ content_block_stop
//! - finish           → message_delta + message_stop

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

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
    model: &str,
    _session_id: &str,
    tool_mode: bool,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(
        upstream.bytes_stream(),
    ));
    anthropic_events_reader(Box::pin(reader), model, tool_mode)
}

pub fn anthropic_events_reader(
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: &str,
    tool_mode: bool,
) -> impl Stream<Item = Result<String, ApiError>> {
    AnthropicTransform {
        reader,
        model: model.to_string(),
        started: false,
        finished: false,
        pending_event: String::new(),
        tool_mode,
        tool_buf: String::new(),
        tool_done: false,
        pending_frames: Vec::new(),
        block_index: 1usize,
        finish_reason: "stop".to_string(),
    }
}

struct AnthropicTransform {
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: String,
    started: bool,
    finished: bool,
    pending_event: String,
    tool_mode: bool,
    tool_buf: String,
    tool_done: bool,
    pending_frames: Vec<String>,
    block_index: usize,
    finish_reason: String,
}

impl Stream for AnthropicTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Anthropic 协议要求流以 message_start 开始
        if !self.started {
            self.started = true;
            return Poll::Ready(Some(Ok(self.start_event())));
        }
        if !self.pending_frames.is_empty() {
            let f = self.pending_frames.remove(0);
            return Poll::Ready(Some(Ok(f)));
        }
        if self.finished {
            // 工具模式未检测到完整 JSON：flush 缓冲为正文
            if self.tool_mode && !self.tool_done && !self.tool_buf.trim().is_empty() {
                self.tool_done = true;
                let buf = std::mem::take(&mut self.tool_buf);
                return Poll::Ready(Some(Ok(text_delta_frame(&buf))));
            }
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
                    if self.tool_mode && !self.tool_done && !self.tool_buf.trim().is_empty() {
                        let buf = std::mem::take(&mut self.tool_buf);
                        return Poll::Ready(Some(Ok(text_delta_frame(&buf))));
                    }
                    return Poll::Ready(Some(Ok(self.stop_events())));
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
                        let json: serde_json::Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let event = json
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        match event.as_str() {
                            "reasoning-delta" => {
                                let delta =
                                    json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                if delta.is_empty() {
                                    continue;
                                }
                                let frame = format!(
                                    "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\n",
                                    serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
                                    serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":delta}})
                                );
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            "text-delta" => {
                                let delta =
                                    json.get("delta").and_then(|v| v.as_str()).unwrap_or("");
                                if delta.is_empty() {
                                    continue;
                                }
                                if self.tool_mode && !self.tool_done {
                                    self.tool_buf.push_str(delta);
                                    if let Some(tc) = detect_tool_call(&self.tool_buf) {
                                        self.tool_done = true;
                                        self.tool_buf.clear();
                                        let idx = self.block_index;
                                        self.pending_frames = Vec::new();
                                        if !tc.preamble.is_empty() {
                                            self.pending_frames
                                                .push(text_delta_frame(&tc.preamble));
                                        }
                                        self.pending_frames.extend(anthropic_tool_frames(idx, &tc));
                                        if !tc.rest.is_empty() {
                                            self.pending_frames.push(text_delta_frame(&tc.rest));
                                        }
                                        // 立即返回待发帧（不能 continue：下一行可能是 finish）
                                        let f = self.pending_frames.remove(0);
                                        return Poll::Ready(Some(Ok(f)));
                                    }
                                }
                                let frame = text_delta_frame(delta);
                                return Poll::Ready(Some(Ok(frame)));
                            }
                            "finish" => {
                                self.finished = true;
                                self.finish_reason = json
                                    .get("finishReason")
                                    .and_then(|v| v.as_str())
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("stop")
                                    .to_string();
                                return Poll::Ready(Some(Ok(self.stop_events())));
                            }
                            "error" => {
                                let raw = json
                                    .get("errorText")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("上游流错误");
                                // 与非流式路径一致：可操作中文提示（区分 credit/暂停/429/未知）
                                let hint = crate::upstream::describe_upstream_error(raw);
                                self.finished = true;
                                // 错误后必须发出终止事件（message_stop），否则客户端挂起等待
                                let frame = format!(
                                    "event: content_block_delta\ndata: {}\n\n{}",
                                    serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":format!("\n\n[hint]")}}),
                                    self.stop_events()
                                );
                                let frame = frame
                                    .replace("[hint]", &format!("上游错误: {hint}（原始: {raw}）"));
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
    fn start_event(&self) -> String {
        format!(
            "event: message_start\ndata: {}\n\n",
            serde_json::json!({
                "type":"message_start",
                "message":{
                    "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
                    "type":"message",
                    "role":"assistant",
                    "model": self.model,
                    "content":[],
                    "stop_reason":null,
                    "stop_sequence":null,
                    "usage":{"input_tokens":0,"output_tokens":0}
                }
            })
        )
    }

    fn stop_events(&self) -> String {
        let stop = if self.finish_reason == "tool_calls" {
            "tool_use"
        } else {
            "end_turn"
        };
        format!(
            "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
            serde_json::json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":{"output_tokens":0}}),
            serde_json::json!({"type":"message_stop"})
        )
    }
}

fn text_delta_frame(delta: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {}\n\n",
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":delta}})
    )
}

struct ToolCallInfo {
    id: String,
    name: String,
    arguments_json: String,
    preamble: String,
    rest: String,
}

fn detect_tool_call(buf: &str) -> Option<ToolCallInfo> {
    let cleaned = strip_fences(buf);
    let mut search = 0usize;
    while search < cleaned.len() {
        let Some(rel) = cleaned[search..].find('{') else {
            break;
        };
        let start = search + rel;
        let (v, consumed) = match serde_json::from_str::<serde_json::Value>(&cleaned[start..]) {
            Ok(v) => {
                let consumed = json_consumed(&cleaned[start..], &v);
                (v, consumed)
            }
            Err(_) => {
                search = start + 1;
                continue;
            }
        };
        let tc = match v.get("tool_call") {
            Some(tc) => tc,
            None => {
                search = start + 1;
                continue;
            }
        };
        let name = tc
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            return None;
        }
        let args = tc
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let arguments_json = match &args {
            serde_json::Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        let rest = cleaned[start + consumed..].trim().to_string();
        return Some(ToolCallInfo {
            id: format!("toolu_{}", uuid::Uuid::new_v4().simple()),
            name,
            arguments_json,
            preamble: cleaned[..start].trim().to_string(),
            rest,
        });
    }
    None
}

fn strip_fences(s: &str) -> &str {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest).trim();
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    t
}

fn anthropic_tool_frames(index: usize, tc: &ToolCallInfo) -> Vec<String> {
    let mut out = Vec::new();
    let start = format!(
        "event: content_block_start\ndata: {}\n\n",
        serde_json::json!({
            "type":"content_block_start",
            "index": index,
            "content_block":{"type":"tool_use","id":tc.id,"name":tc.name,"input":{}}
        })
    );
    out.push(start);
    // Anthropic 协议要求 partial_json 是合法 JSON 片段（完整对象或合法后缀），
    // 前切分块会产生非法 JSON（{"city":"sh）导致 SDK 拼接失败。
    // 一次性发送完整 arguments（最兼容），超长参数可后续改后缀式分块。
    out.push(format!(
        "event: content_block_delta\ndata: {}\n\n",
        serde_json::json!({
            "type":"content_block_delta",
            "index": index,
            "delta":{"type":"input_json_delta","partial_json":tc.arguments_json}
        })
    ));
    out.push(format!(
        "event: content_block_stop\ndata: {}\n\n",
        serde_json::json!({"type":"content_block_stop","index": index})
    ));
    out
}

/// 计算 JSON 值在文本中消耗的字节数
fn json_consumed(text: &str, value: &serde_json::Value) -> usize {
    let compact = serde_json::to_string(value).unwrap_or_default();
    let t = text.trim_start();
    let leading = text.len() - t.len();
    if let Some(pos) = t.find(&compact) {
        return leading + pos + compact.len();
    }
    let mut depth = 0i32;
    let mut end = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, ch) in t.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if ch == '\\' {
                esc = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => {
                depth += 1;
                if depth == 1 {
                    end = i;
                }
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    leading + end
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::StreamExt;

    fn mock_reader(data: &str) -> Pin<Box<dyn AsyncBufRead + Send>> {
        let stream = futures::stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::from(
            data.to_string(),
        ))]);
        Box::pin(BufReader::new(crate::protocol::stream::reader_with_bytes(
            stream,
        )))
    }

    fn collect_frames(reader: Pin<Box<dyn AsyncBufRead + Send>>, tool_mode: bool) -> Vec<String> {
        let mut s = anthropic_events_reader(reader, "test-model", tool_mode);
        let mut out = Vec::new();
        while let Some(item) = futures::executor::block_on(s.next()) {
            out.push(item.unwrap());
        }
        out
    }

    #[test]
    fn anthropic_tool_use_conversion() {
        let sse = [
            "data: {\"type\":\"text-delta\",\"delta\":\"{\\\"tool_call\\\":{\\\"name\\\":\\\"search\\\",\\\"arguments\\\":\\\"{\\\\\\\"q\\\\\\\":\\\\\\\"rust\\\\\\\"}\\\"}}\"}\n\n",
            "data: {\"type\":\"finish\",\"finishReason\":\"tool_calls\"}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat();
        let frames = collect_frames(mock_reader(&sse), true);
        assert!(frames.iter().any(|f| f.contains("tool_use")));
        assert!(frames.iter().any(|f| f.contains("input_json_delta")));
        assert!(frames.iter().any(|f| f.contains("content_block_stop")));
    }

    #[test]
    fn anthropic_error_event_emits_message_stop() {
        let sse = [
            "data: {\"type\":\"error\",\"errorText\":\"限流了\"}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat();
        let frames = collect_frames(mock_reader(&sse), false);
        // 错误后仍必须给出终止事件，否则客户端挂起
        assert!(frames.iter().any(|f| f.contains("message_stop")));
        assert!(frames.iter().any(|f| f.contains("message_delta")));
        assert!(
            frames.iter().any(|f| f.contains("上游错误"))
                || frames.iter().any(|f| f.contains("限流"))
        );
    }

    #[test]
    fn anthropic_start_event_has_model() {
        let sse = "data: {\"type\":\"text-delta\",\"delta\":\"x\"}\n\n";
        let frames = collect_frames(mock_reader(sse), false);
        let first = frames.first().unwrap();
        assert!(
            first.contains("\"model\":\"test-model\""),
            "start event model missing: {first}"
        );
    }

    #[test]
    fn anthropic_reasoning_text() {
        let sse = [
            "data: {\"type\":\"reasoning-delta\",\"delta\":\"想\"}\n\n",
            "data: {\"type\":\"text-delta\",\"delta\":\"答\"}\n\n",
            "data: {\"type\":\"finish\",\"finishReason\":\"stop\"}\n\n",
            "data: [DONE]\n\n",
        ]
        .concat();
        let frames = collect_frames(mock_reader(&sse), false);
        assert!(frames.iter().any(|f| f.contains("thinking_delta")));
        assert!(frames.iter().any(|f| f.contains("text_delta")));
    }
}
