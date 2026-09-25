# TryingOpen 上游协议逆向笔记

> 数据来源：抓包 `源代码、网络数据包/www.tryingopen.com.har` + 站点 JS chunk（`07cl9ce_x7idy.js` 等）+ 实时首页抓取。
> 更新：2026-09-24。

## 1. 认证

- **完全匿名**：`POST /api/open` 不需要 Cookie / 登录 / API Key / 会话 id。
- 请求头：`origin: https://www.tryingopen.com`、`referer: https://www.tryingopen.com/`、`user-agent: Chrome 151`（站点校验浏览器语义）。
- 限流：**单 IP 每 24h UTC 日约 20 次**（站点未公开精确配额，实测 429 响应触发）。

## 2. 对话流

```
POST /api/open
Content-Type: application/json
{
  "id": "chat-xxxxxxxxxxxxxxxx",
  "trigger": "submit-message",
  "messageId": "msg-xxxxxxxxxxxxxxxx",
  "model": "qwen/qwen3.8-27b",
  "effort": "balanced",
  "messages": [
    {"id":"msg-xxx","role":"user","parts":[{"type":"text","text":"你好"}]}
  ],
  "stream": true
}
```

响应：`text/event-stream; charset=utf-8`。事件：

| 事件 | data 字段 | 含义 |
|------|-----------|------|
| `start` | `{messageMetadata:{modelName,maker,logo,webSearch}}` | 开始 |
| `start-step` | `{}` | 步骤开始 |
| `reasoning-start` | `{id}` | 思考开始 |
| `reasoning-delta` | `{id, delta}` | 思考增量（reasoning_content） |
| `reasoning-end` | `{id, providerMetadata?}` | 思考结束 |
| `text-start` | `{id}` | 正文开始 |
| `text-delta` | `{id, delta}` | 正文增量 |
| `text-end` | `{id}` | 正文结束 |
| `finish-step` | `{}` | 步骤结束 |
| `finish` | `{finishReason, messageMetadata:{inputTokens,outputTokens,totalTokens,reasoningTokens,costUsd,msToFirstToken,msGenerating}}` | 结束 |
| `error` | `{errorText}` | 错误 |
| `[DONE]` | - | 流结束 |

消息元数据（HAR 实测，多轮时回传）：`{modelName, maker, logo, webSearch, finishReason, inputTokens, outputTokens, totalTokens, msToFirstToken, msGenerating, costUsd, reasoningTokens}`。

## 3. 消息结构

- 每条消息：`{id, role:"user"|"assistant", parts:[...], metadata?}`
- part 类型：
  - `{type:"text", text:"..."}`
  - `{type:"file", mediaType:"image/png", url:"data:...;base64,..."}`（图片输入）
  - `{type:"step-start"}` / `{type:"reasoning", id, text, state:"done"}` / `{type:"text", text, state:"done"}`（assistant 历史回放）
- **没有 system 角色**：系统提示词拼进第一条 user 的 `[SYSTEM INSTRUCTIONS] ... [/SYSTEM INSTRUCTIONS]`。
- **没有原生 tool_calls**：工具调用是纯文本 JSON（`{"tool_call":{"name":"...","arguments":{...}}}`），网关/客户端自行解析。

## 4. effort 档位

- `balanced`（默认）、`deep` 等。HAR 里 qwen 用 balanced 和 deep 都正常。
- 客户端请求的 `max_tokens` / `temperature` 上游不读（`/api/open` 固定参数）。

## 5. 模型目录

- `GET /` 首页 HTML → 提取 `/_next/static/chunks/*.js` 路径
- 每个 chunk 里模型记录：`{id:"provider/model",name:"...",maker:"...",logo:"...",params:"...",context:"262k",blurb:"...",supportsTools:!0,supportsImages:!0,pricePerMTok:3.2,zdr:!0,...}`
- 可选字段：`messageLimit`（如 kimi-k3=5）、`cheaperFallbackId`（如 kimi-k3 → minimax/minimax-m3）
- 静态目录 12 模型兜底；2026-09-24 实时抓到 24 个（新增 glm-5.3-flash、qwen3.8-flash、deepseek-v4.1-flash、glm-5.3、nemotron-3-ultra、qwen3.8-2.4t、mimo-v2.6 系、inclusionai/ling-3.0、anthropic/claude-sonnet-5、openai/gpt-5.6-terra 等）

## 6. 限制与风险

- 每 24h UTC 日约 20 次：**必须配代理池轮换**（本项目已内置）。
- 免费额度：模型按 `pricePerMTok` 计费但站点提供免费额度（登录后可见）；匿名端点额度以站点为准。
- 免费代理为明文 http，仅建议用于低敏感度对话；住宅代理更稳。
- `direct_fallback` 直连本机 IP 每 24h UTC 日只有约 20 次额度，高并发会触发 429。
