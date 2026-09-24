//! OpenAI SSE 流：TryingOpen SSE → OpenAI chat.completions SSE
//!
//! 上游事件 → OpenAI delta：
//! - reasoning-delta → delta.reasoning_content（思考区）
//! - text-delta       → delta.content（正文增量）
//! - 工具模式（tool_mode=true）：text-delta 累积成 {"tool_call":{"name","arguments"}} JSON
//!   后转标准 OpenAI delta.tool_calls 增量帧（首帧 role/name + 后续 arguments 分块）
//! - finish           → finish_reason（从 finishReason 读）
//! - error            → 上游错误文本追加

use crate::errors::ApiError;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use futures::{Future, Stream};

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

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
    tool_mode: bool,
) -> impl Stream<Item = Result<String, ApiError>> {
    let reader = BufReader::new(crate::protocol::stream::reader_with_bytes(
        upstream.bytes_stream(),
    ));
    openai_events_reader(Box::pin(reader), model, created, tool_mode)
}

/// 供测试/复用：直接由 AsyncBufRead reader 构造
pub fn openai_events_reader(
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: &str,
    created: i64,
    tool_mode: bool,
) -> impl Stream<Item = Result<String, ApiError>> {
    OpenAiTransform {
        reader,
        model: model.to_string(),
        created,
        finished: false,
        done_sent: false,
        saw_content: false,
        finish_reason: "stop".to_string(),
        pending_event: String::new(),
        tool_mode,
        tool_buf: String::new(),
        tool_done: false,
        pending_frames: Vec::new(),
    }
}

struct OpenAiTransform {
    reader: Pin<Box<dyn AsyncBufRead + Send>>,
    model: String,
    created: i64,
    finished: bool,
    done_sent: bool,
    saw_content: bool,
    finish_reason: String,
    pending_event: String,
    tool_mode: bool,
    tool_buf: String,
    tool_done: bool,
    pending_frames: Vec<String>,
}

#[derive(Debug, Clone)]
struct ToolCallInfo {
    id: String,
    name: String,
    arguments_json: String,
    preamble: String,
    rest: String,
}

impl Stream for OpenAiTransform {
    type Item = Result<String, ApiError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 优先排空待发工具帧
        if !self.pending_frames.is_empty() {
            let f = self.pending_frames.remove(0);
            return Poll::Ready(Some(Ok(f)));
        }
        if self.finished && !self.done_sent {
            // 工具模式未检测到完整 JSON：把累积缓冲当正文 flush
            if self.tool_mode && !self.tool_done && !self.tool_buf.trim().is_empty() {
                self.tool_done = true;
                let buf = std::mem::take(&mut self.tool_buf);
                let frame = content_frame(&self.model, self.created, &buf);
                return Poll::Ready(Some(Ok(frame)));
            }
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
                    if !self.saw_content && self.tool_buf.trim().is_empty() {
                        return Poll::Ready(Some(Ok(openai_empty_error(
                            &self.model,
                            self.created,
                        ))));
                    }
                    if self.tool_mode && !self.tool_done && !self.tool_buf.trim().is_empty() {
                        let buf = std::mem::take(&mut self.tool_buf);
                        return Poll::Ready(Some(Ok(content_frame(
                            &self.model,
                            self.created,
                            &buf,
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
                                    // 工具模式：累积检测
                                    if self.tool_mode && !self.tool_done {
                                        self.tool_buf.push_str(delta);
                                        if let Some(tc) = detect_tool_call(&self.tool_buf) {
                                            self.tool_done = true;
                                            self.tool_buf.clear();
                                            let model_name = self.model.clone();
                                            let created_ts = self.created;
                                            self.pending_frames = Vec::new();
                                            if !tc.preamble.is_empty() {
                                                self.pending_frames.push(content_frame(
                                                    &model_name,
                                                    created_ts,
                                                    &tc.preamble,
                                                ));
                                            }
                                            self.pending_frames.extend(openai_tool_frames(
                                                &model_name,
                                                created_ts,
                                                &tc,
                                            ));
                                            if !tc.rest.is_empty() {
                                                self.pending_frames.push(content_frame(
                                                    &model_name,
                                                    created_ts,
                                                    &tc.rest,
                                                ));
                                            }
                                            // 立即返回待发帧（不能 continue：下一行可能是 finish）
                                            let f = self.pending_frames.remove(0);
                                            return Poll::Ready(Some(Ok(f)));
                                        }
                                        continue;
                                    }
                                    let frame = content_frame(&self.model, self.created, delta);
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

fn content_frame(model: &str, created: i64, delta: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "id": format!("chatcmpl-{}", created),
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{ "index": 0, "delta": { "content": delta }, "finish_reason": null }]
        })
    )
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

/// 从累积文本检测完整工具调用 JSON（剥 markdown fence、容忍前后缀文本）
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
            id: format!("call_{}", uuid::Uuid::new_v4().simple()),
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

/// 生成 OpenAI 工具调用增量帧：首帧 role/name + arguments 分块
fn openai_tool_frames(model: &str, created: i64, tc: &ToolCallInfo) -> Vec<String> {
    let mut out = Vec::new();
    let base = serde_json::json!({
        "id": format!("chatcmpl-{}", created),
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": { "role": "assistant", "tool_calls": [{ "index": 0, "id": tc.id, "type": "function", "function": { "name": tc.name, "arguments": "" } }] }, "finish_reason": null }]
    });
    out.push(format!("data: {}\n\n", base));
    for chunk in chunk_str(&tc.arguments_json, 16) {
        out.push(format!(
            "data: {}\n\n",
            serde_json::json!({
                "id": format!("chatcmpl-{}", created),
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": chunk } }] }, "finish_reason": null }]
            })
        ));
    }
    out
}

fn chunk_str(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cnt = 0usize;
    for ch in s.chars() {
        cur.push(ch);
        cnt += 1;
        if cnt == n {
            out.push(std::mem::take(&mut cur));
            cnt = 0;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
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

/// 计算 JSON 值在文本中消耗的字节数（从 start 起的完整对象）
fn json_consumed(text: &str, value: &serde_json::Value) -> usize {
    let compact = serde_json::to_string(value).unwrap_or_default();
    let t = text.trim_start();
    let leading = text.len() - t.len();
    if let Some(pos) = t.find(&compact) {
        return leading + pos + compact.len();
    }
    // 兜底：从第一个 { 到匹配的 }
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

    fn collect_frames(
        reader: Pin<Box<dyn AsyncBufRead + Send>>,
        model: &str,
        tool_mode: bool,
    ) -> Vec<String> {
        let mut s = openai_events_reader(reader, model, 1, tool_mode);
        let mut out = Vec::new();
        while let Some(item) = futures::executor::block_on(s.next()) {
            out.push(item.unwrap());
        }
        out
    }

    #[test]
    fn detect_tool_call_parses_complete_json() {
        let buf = r#"{"tool_call":{"name":"get_weather","arguments":"{\"city\":\"shanghai\"}"}}"#;
        let tc = detect_tool_call(buf).expect("should detect tool call");
        assert_eq!(tc.name, "get_weather");
        assert!(tc.arguments_json.contains("shanghai"));
        assert!(
            tc.rest.is_empty(),
            "rest should be empty, got {:?}",
            tc.rest
        );
    }

    #[test]
    fn openai_tool_call_incremental_frames() {
        // 真实上游 text-delta 的 delta（parse 后）是干净 JSON 文本
        let clean_json =
            r#"{"tool_call":{"name":"get_weather","arguments":"{\"city\":\"shanghai\"}"}}"#;
        let parts = [
            r#"{"tool_call":{"name":"get_weather","#,
            r#""arguments":"{\"city\":\"shanghai\"}"}}"#,
        ];
        let full_buf: String = parts.concat();
        assert_eq!(full_buf, clean_json);
        assert!(
            detect_tool_call(&full_buf).is_some(),
            "detect_tool_call should parse full buf: {full_buf}"
        );
        let mut sse = String::new();
        for p in parts {
            let evt = serde_json::json!({"type": "text-delta", "delta": p});
            sse.push_str(&format!("data: {evt}\n\n"));
        }
        sse.push_str(r#"data: {"type":"finish","finishReason":"tool_calls"}"#);
        sse.push_str("\n\n");
        sse.push_str("data: [DONE]\n\n");
        let frames = collect_frames(mock_reader(&sse), "m", true);
        assert!(
            frames.len() >= 3,
            "expected tool frames + done, got {frames:?}"
        );
        let first = frames[0].to_string();
        eprintln!("DEBUG first frame: {first}");
        assert!(
            first.contains("get_weather"),
            "first frame should carry name: {first}"
        );
        assert!(
            first.contains(r#""arguments":"""#),
            "first frame args empty: {first}"
        );
        let mut args = String::new();
        for f in frames.iter().skip(1) {
            if f.contains("arguments") && !f.contains(r#""finish_reason":"tool_calls""#) {
                let v: serde_json::Value =
                    serde_json::from_str(f.trim().trim_start_matches("data: ").trim()).unwrap();
                if let Some(ts) = v["choices"][0]["delta"]["tool_calls"].as_array() {
                    for t in ts {
                        if let Some(a) = t["function"]["arguments"].as_str() {
                            args.push_str(a);
                        }
                    }
                }
            }
        }
        assert!(
            args.contains("shanghai"),
            "args should contain shanghai: {args}"
        );
    }

    #[test]
    fn openai_normal_reasoning_and_text() {
        let sse = [
            r#"data: {"type":"reasoning-delta","delta":"思考中"}"#,
            "\n\n",
            r#"data: {"type":"text-delta","delta":"你好"}"#,
            "\n\n",
            r#"data: {"type":"finish","finishReason":"stop"}"#,
            "\n\n",
            "data: [DONE]\n\n",
        ]
        .concat();
        let frames = collect_frames(mock_reader(&sse), "m", false);
        assert!(frames.iter().any(|f| f.contains("reasoning_content")));
        assert!(frames.iter().any(|f| f.contains(r#""content":"你好""#)));
        assert!(frames.iter().any(|f| f.contains("[DONE]")));
    }
}
