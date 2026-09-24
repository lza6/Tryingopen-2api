# API 契约文档（N2 契约审计产出）

> 面向调用接入方。所有字段、错误码、认证与真实行为一致（经真实验证）。

## 通用

### 认证
- `api_keys` 未配置（默认）→ 本机放行，任意 key 可用（如 `sk-local`）
- `api_keys` 配置后 → 必须带 `Authorization: Bearer <key>` 或 `x-api-key: <key>`
- 运行时可在 `/ui` 或 `POST /api/config/api-key` 生成 key

### 错误响应形状
- OpenAI 端点：`{"error":{"message","type","code"}}`（type: invalid_request_error/authentication_error/upstream_error/rate_limit_error/not_found_error/internal_error）
- Anthropic 端点：`{"type":"error","error":{"type","message"}}`
- HTTP 状态：400=bad_request、401=unauthorized、404=not_found、429=rate_limit、502=upstream、500=internal

### 请求体限制
- 最大 16MB（DefaultBodyLimit，防内存打爆）

---

## 端点契约

### POST /v1/chat/completions（OpenAI）
请求：
```json
{
  "model": "qwen/qwen3.8-27b",        // 必填；支持裸名自动补前缀
  "messages": [{"role":"user","content":"hi"}],  // 必填
  "stream": false,                     // 可选
  "max_tokens": 200,                   // 可选（上游不读，透传语义）
  "temperature": 0.7,                  // 可选
  "effort": "balanced",                // 可选 balanced/deep/low（思考程度）
  "tools": [...],                      // 可选（工具定义 → 上游提示词模式）
  "tool_choice": null,                 // 可选
  "user": "thread-1"                   // 可选（会话粘滞 key）
}
```
响应（非流）：
```json
{"id":"chatcmpl-...","object":"chat.completion","created":...,"model":"qwen/qwen3.8-27b",
 "choices":[{"index":0,"message":{"role":"assistant","content":"...","reasoning_content":"...（思考）"},"finish_reason":"stop"}],
 "usage":{"prompt_tokens":N,"completion_tokens":N,"reasoning_tokens":N,"total_tokens":N}}
```
流式：SSE `data:` 帧，delta 含 `reasoning_content`/`content`/`tool_calls`，结束 `[DONE]`。

### POST /v1/messages（Anthropic）
请求：
```json
{"model":"qwen/qwen3.8-27b","messages":[{"role":"user","content":"hi"}],
 "stream":false,"max_tokens":200,"system":"...","tools":[...],"metadata":{"thread_id":"t1"},"effort":"balanced"}
```
响应（非流）：`{"type":"message","content":[{"type":"thinking",...},{"type":"text",...}],"usage":{"input_tokens":N,"output_tokens":N}}`
流式：SSE 事件 `message_start` → `content_block_start` → `content_block_delta`（thinking_delta/text_delta/input_json_delta）→ `content_block_stop` → `message_delta` → `message_stop`。

### GET /v1/models
- OpenAI 形状：`{"object":"list","data":[{"id":"provider/model","object":"model","created":0,"owned_by":"family"}]}`
- 动态目录启动/定时同步；已下线模型隐藏

### GET /healthz
`{"ok":true,"app":"tryingopen2api","version":"0.1.1","models":N,"proxies":N,"upstream":"..."}`

### GET /api/proxies
`{"total":N,"residential":N,"free":N,"available":N,"cooldown":N,"capacity":{"capacity_total":N,"capacity_used":N,"capacity_remaining":N},"items":[{host_port,source,daily_uses,cooling,cooldown_seconds,fails,health_score,latency_ms}]}`
- 脱敏：只暴露 host:port（住宅 user:pass 不泄漏）

### POST /api/proxies/refresh-free
- 手动触发免费代理抓取；需 free_proxy_enabled=true 否则 400

### POST /api/catalog/refresh
- 手动同步上游模型目录；失败返回 502

### GET /api/guide
- 接入信息（监听地址/key 状态/模型列表/代理数）

### POST /api/config/api-key
- `{"action":"generate"|"set"|"clear","key":optional}` → `{"ok":true,"key":...}`

### GET /ui（Basic Auth 保护）
- 配置 `ui_password` 后需 `Authorization: Basic base64(任意用户:密码)`
- 无密码/错密码 → 401 + `WWW-Authenticate: Basic realm="TryingOpen2API"`
- /healthz 不受此限制（探活）

### GET /ui
- 内置控制面板（HTML）

---

## 已知行为与限制（防坑）
1. **模型不存在自动降级**：请求未知模型 → 自动 fallback 到默认模型并正常回答（不报错）。如需严格报错，客户端应先用 /v1/models 校验。
2. **上游生成慢**：单请求 4-58s 属正常（上游模型生成耗时）。生产客户端务必用 `stream=true`。
3. **每 IP 每小时约 20 次**：直连兜底会消耗本机配额；代理池自动轮换出口。
4. **工具调用是提示词式**：模型可能选择不调用工具（会说明），客户端需容忍文本回复。
5. **多模态**：支持 image_url/data URL → 上游 file part（需 supportsImages 模型，如 qwen）。
6. **effort**：balanced/deep 等由上游决定，未知值可能被忽略或报错。
