//! OpenAI Responses API (/v1/responses) -> chat completions bridge.
//!
//! Upstream has no native Responses or tool_calls. We:
//! - flatten `input` / `instructions` / responses-style tools into chat messages
//! - reuse the chat upstream call
//! - map text or tool_call JSON back into Responses `output` items

use serde_json::{json, Value};

pub fn normalize_tools(tools: &Value) -> Value {
    let Some(arr) = tools.as_array() else {
        return json!([]);
    };
    let out: Vec<Value> = arr
        .iter()
        .map(|t| {
            if t.get("function").is_some() {
                return t.clone();
            }
            let name = t.get("name").cloned().unwrap_or(Value::Null);
            let mut function = json!({ "name": name });
            if let Some(d) = t.get("description") {
                function["description"] = d.clone();
            }
            if let Some(p) = t.get("parameters") {
                function["parameters"] = p.clone();
            }
            json!({ "type": "function", "function": function })
        })
        .collect();
    Value::Array(out)
}

pub fn input_to_messages(instructions: Option<&str>, input: &Value) -> Vec<Value> {
    let mut msgs = Vec::new();
    if let Some(ins) = instructions {
        if !ins.trim().is_empty() {
            msgs.push(json!({"role":"system","content": ins}));
        }
    }
    match input {
        Value::String(s) => msgs.push(json!({"role":"user","content": s})),
        Value::Array(items) => {
            for item in items {
                if let Some(role) = item.get("role").and_then(|v| v.as_str()) {
                    let content = item
                        .get("content")
                        .cloned()
                        .unwrap_or(Value::String(String::new()));
                    let mut msg = json!({"role": role, "content": flatten_content(&content)});
                    if let Some(tc) = item.get("tool_calls") {
                        msg["tool_calls"] = tc.clone();
                    }
                    if let Some(id) = item.get("tool_call_id") {
                        msg["tool_call_id"] = id.clone();
                    }
                    if let Some(name) = item.get("name") {
                        msg["name"] = name.clone();
                    }
                    msgs.push(msg);
                    continue;
                }
                match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "message" => {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let content = item.get("content").cloned().unwrap_or(Value::Null);
                        msgs.push(json!({"role": role, "content": flatten_content(&content)}));
                    }
                    "input_text" | "output_text" => {
                        let role =
                            if item.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                                "assistant"
                            } else {
                                "user"
                            };
                        let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        msgs.push(json!({"role": role, "content": text}));
                    }
                    "function_call" => {
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                        let args = item.get("arguments").cloned().unwrap_or(json!("{}"));
                        let args_s = if args.is_string() {
                            args
                        } else {
                            Value::String(args.to_string())
                        };
                        msgs.push(json!({
                            "role": "assistant",
                            "content": "",
                            "tool_calls": [{
                                "id": item.get("call_id").cloned().unwrap_or(json!("call_prev")),
                                "type": "function",
                                "function": {"name": name, "arguments": args_s}
                            }]
                        }));
                    }
                    "function_call_output" => {
                        let out = item
                            .get("output")
                            .cloned()
                            .unwrap_or(Value::String(String::new()));
                        let text = if let Some(s) = out.as_str() {
                            s.to_string()
                        } else {
                            out.to_string()
                        };
                        msgs.push(json!({
                            "role": "tool",
                            "tool_call_id": item.get("call_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "content": text
                        }));
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if msgs.is_empty() {
        msgs.push(json!({"role":"user","content":""}));
    }
    msgs
}

fn flatten_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut out = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
            out
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn completed_response(
    id: &str,
    model: &str,
    text: &str,
    tool_name: Option<&str>,
    tool_args: Option<&str>,
    tool_id: Option<&str>,
    usage: Option<&Value>,
) -> Value {
    let mut output = Vec::new();
    if let Some(name) = tool_name {
        output.push(json!({
            "type": "function_call",
            "id": format!("fc_{}", tool_id.unwrap_or("0")),
            "call_id": tool_id.unwrap_or("call_0"),
            "name": name,
            "arguments": tool_args.unwrap_or("{}"),
            "status": "completed"
        }));
    }
    if tool_name.is_none() || !text.trim().is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "content": [{"type":"output_text","text": text}]
        }));
    }
    let usage = usage
        .cloned()
        .unwrap_or(json!({"input_tokens":0,"output_tokens":0,"total_tokens":0}));
    json!({
        "id": id,
        "object": "response",
        "status": "completed",
        "model": model,
        "output": output,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").or_else(|| usage.get("input_tokens")).cloned().unwrap_or(json!(0)),
            "output_tokens": usage.get("completion_tokens").or_else(|| usage.get("output_tokens")).cloned().unwrap_or(json!(0)),
            "total_tokens": usage.get("total_tokens").cloned().unwrap_or(json!(0))
        }
    })
}

#[derive(Debug, Default)]
pub struct StreamAccum {
    pub text: String,
    pub tool_name: Option<String>,
    pub tool_args: String,
    pub tool_id: Option<String>,
    pub started: bool,
}

impl StreamAccum {
    /// Map one OpenAI SSE frame (`data: ...`) into zero or more Responses SSE frames.
    pub fn push(&mut self, frame: &str, resp_id: &str, model: &str) -> String {
        let mut out = String::new();
        if !self.started {
            self.started = true;
            out.push_str(&format!(
                "event: response.created\ndata: {}\n\n",
                json!({"type":"response.created","response":{"id":resp_id,"object":"response","status":"in_progress","model":model}})
            ));
        }
        let data = frame.trim().strip_prefix("data:").unwrap_or("").trim();
        if data == "[DONE]" {
            let body = completed_response(
                resp_id,
                model,
                &self.text,
                self.tool_name.as_deref(),
                Some(&self.tool_args),
                self.tool_id.as_deref(),
                None,
            );
            out.push_str(&format!(
                "event: response.completed\ndata: {}\n\n",
                json!({"type":"response.completed","response": body})
            ));
            return out;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return out;
        };
        let delta = &v["choices"][0]["delta"];
        if let Some(t) = delta.get("content").and_then(|x| x.as_str()) {
            if !t.is_empty() {
                self.text.push_str(t);
                out.push_str(&format!(
                    "event: response.output_text.delta\ndata: {}\n\n",
                    json!({"type":"response.output_text.delta","delta": t})
                ));
            }
        }
        if let Some(calls) = delta.get("tool_calls").and_then(|x| x.as_array()) {
            if let Some(c0) = calls.first() {
                if let Some(id) = c0.get("id").and_then(|x| x.as_str()) {
                    self.tool_id = Some(id.to_string());
                }
                if let Some(name) = c0
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|x| x.as_str())
                {
                    if !name.is_empty() {
                        self.tool_name = Some(name.to_string());
                        out.push_str(&format!(
                            "event: response.output_item.added\ndata: {}\n\n",
                            json!({"type":"response.output_item.added","item":{"type":"function_call","name":name,"call_id":self.tool_id,"arguments":""}})
                        ));
                    }
                }
                if let Some(args) = c0
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|x| x.as_str())
                {
                    if !args.is_empty() {
                        self.tool_args.push_str(args);
                        out.push_str(&format!(
                            "event: response.function_call_arguments.delta\ndata: {}\n\n",
                            json!({"type":"response.function_call_arguments.delta","delta": args})
                        ));
                    }
                }
            }
        }
        out
    }
}

pub struct ResponsesSse<S> {
    pub inner: S,
    pub acc: StreamAccum,
    pub id: String,
    pub model: String,
}

impl<S> futures::Stream for ResponsesSse<S>
where
    S: futures::Stream<Item = Result<String, crate::errors::ApiError>> + Unpin,
{
    type Item = Result<String, crate::errors::ApiError>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let frame = match std::pin::Pin::new(&mut self.inner).poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(frame))) => frame,
            std::task::Poll::Ready(Some(Err(e))) => return std::task::Poll::Ready(Some(Err(e))),
            std::task::Poll::Ready(None) => return std::task::Poll::Ready(None),
            std::task::Poll::Pending => return std::task::Poll::Pending,
        };
        let id = self.id.clone();
        let model = self.model.clone();
        let mapped = self.acc.push(&frame, &id, &model);
        std::task::Poll::Ready(Some(Ok(mapped)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_tools_and_input() {
        let tools =
            json!([{"type":"function","name":"get_weather","parameters":{"type":"object"}}]);
        let norm = normalize_tools(&tools);
        assert_eq!(norm[0]["function"]["name"], "get_weather");
        let msgs = input_to_messages(Some("be brief"), &json!("hello"));
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["content"], "hello");
    }

    #[test]
    fn function_call_roundtrip_shape() {
        let body = completed_response(
            "resp_1",
            "m",
            "",
            Some("get_weather"),
            Some("{\"city\":\"shanghai\"}"),
            Some("call_1"),
            None,
        );
        assert_eq!(body["output"][0]["type"], "function_call");
        assert_eq!(body["output"][0]["name"], "get_weather");
        assert_eq!(body["status"], "completed");
    }
}
