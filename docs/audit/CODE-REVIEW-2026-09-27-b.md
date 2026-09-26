# CODE-REVIEW-2026-09-27-b

- 审查对象：`tryingopen-2api`（v0.1.15，main HEAD `fbbc6b8`）+ 当前未提交工作区（仅审查，不修改）
- 审查范围：/v1/messages 的 Anthropic thinking 映射（enabled→effort=deep+系统提示）、错误可操作化、面板可访问性
- 审查时间：2026-09-27（Asia/Shanghai）
- 方法：只读审查。`git diff` 与工作区源码逐行核对；以最终刷新后的工作区为准（审查中工作区发生过一次更新）
- 本次未运行 `cargo test` / `cargo build`（只读审查；协作者已自行跑门禁）；结论以静态读代码为准
- 文件行号以最终刷新后工作区为准

## 结论

- 主控要做的两件事（thinking→effort=deep+系统提示；错误可操作化）**现状已基本就位且方向正确**：`describe_upstream_error`（区分信用耗尽/模型暂停/429/未知）+ `resolve_effort`（enabled→deep，其余保持）+ 系统提示注入都已落地，且有对应单元测试。
- 但 **Anthropic 流式路径的错误可操作化缺失**：`anthropic_events` 的 `error` 事件只把原始 `errorText` 渲染进 `[上游错误: …]` 文本，不经过 `describe_upstream_error`；Claude Code/Codex 走流式（默认 `stream:true`）时，用户看到的是原始英文错误，和处理过的非流式路径（502 中文可操作提示）不一致。
- 其次，**thinking 映射的 `budget_tokens` 被完全丢弃**：`resolve_effort` 只读 `type`，`budget_tokens` 与 `thinking` 均未随请求上送。设计目标明确是"语义映射"而非透传（文档注释也写了 `Anthropic thinking 参数`→effort 映射），因此直接透传不是必选项；但如果想保留限制思考预算的语义，需要把 budget 通过其他渠道传递（当前能力边界，需如实写明，不能假装支持）。
- 632/643 两条重复系统提示注入路径（OpenAI 与 Anthropic）是既有问题，非本轮引入，但 Anthropic 改版后不仅有重复风险，还多了一个 redirect：system 被拼进第一条 user 后，`redirect_contents` 会把它带回多轮对话，每轮重复。
- 错误分类还有一处可行缺口：上游流内 `error` 事件（`errorText`）目前**不含 HTTP 状态码**，仅靠关键词识别（429 判别就是 `"429"` 子串匹配），遇到纯文案限流（本站只见 `errorText` 字符串，无结构化 error.code）时分类可靠性有限。

## 发现

### P1（建议合并前处理）

1. **Anthropic 流式 error 事件未走 describe_upstream_error**
   - 位置：`src/api.rs:1665`（构造 `anthropic_events` 流）→ `src/protocol/anthropic_sse.rs:191-204`（`"error"` 分支）
   - 现状：`anthropic_sse.rs:193-195` 取 `errorText`，`200` 行直接 `format!("\n\n[上游错误: {msg}]")` 渲染原始文案；非流式路径 `api.rs:1701-1704` 已用 `describe_upstream_error`，流式未用。
   - 影响：Claude Code/Codex（默认 `stream:true`）收到的是用户不可操作的原始英文（"This site has run out of API credit…"），而 502 中文可操作提示只在非流式出现；同一错误两种表现。
   - 建议：`anthropic_sse.rs` 需要拿到上游错误描述。方案（二选一）：
     - A. 在 SSE 转换器内直接 import `crate::upstream::describe_upstream_error`（`anthropic_sse.rs` 已依赖 `crate::errors`，无循环），`error` 分支输出 `[上游错误: {可操作提示}]\n原始: {msg}`；
     - B. 仿 `collect_nonstream`（`api.rs:1278-1331`）在上游层把 `error` 流事件抽取出来，作为 `ApiError::Upstream` 返回独立 502，再由 `api.rs` 统一 `describe_upstream_error`。（B 改观少，但会改变流式错误语义：目前流式错误以 200 + 文本结束，客户端靠文本发现。）
   - 另注意 `anthropic_sse.rs` 的 `error` 分支没有写 `pending_frames`，靠 `stop_events()` 终止；方案 A 只要替换 `msg` 文案即可，协议形态不变。

2. **`resolve_effort` 丢弃 `budget_tokens` 及 `type` 之外的 thinking 细节**
   - 位置：`src/upstream.rs:404-414`
   - 现状：只判断 `type=="enabled"`；`budget_tokens`（Claude Code 会正常发 `{"type":"enabled","budget_tokens":N}`）未参与下游字段，也不进入 `StreamRequest`。
   - 影响：`thinking.enabled` 的语义（限制思考预算）丢失。若上游 `effort=deep` 强制满预算或端侧无法关闭思考，可能使每次请求都产生大段 reasoning tokens（配额/延迟成本）。若上游根本不支持 budgeting，则此丢弃可接受，但要写文档。
   - 建议：
     - 若上游仅消费 `effort` 且无法表达 budget：保留现状，但把「`budget_tokens` 被忽略」「thinking=disabled 等于 effort 默认值」写进 `README`/`docs/API_CONTRACT.md`（当前无任何文档提到 thinking 字段）；
     - 若想保留 budget 语义：在 `StreamRequest` 加 `thinking_budget: Option<u64>`（或 `extra` JSON），由 `resolve_effort` 返回结构化结果 `{effort, enabled}`，由 `UpstreamClient::stream` 按需附到上游 JSON body。注意 `build_upstream_request`（OpenAI 路径 `api.rs:607-758`）无 thinking 概念，只有 Anthropic `/v1/messages` 需要。

3. **system 注入 + 多轮历史可能导致系统提示重复**
   - 位置：`src/api.rs:1578-1591`（Anthropic 注入点）→ `src/session.rs` `redirect_contents`（会话回放）
   - 现状：`src/api.rs:1550-1551` 把思考提示放进 `system_texts`，而 `1594-1614`（`truncate_upstream_messages`）只截断、不含去重；若用户循环复用同一 `thread_id`（Claude Code 默认按会话携带 `metadata.thread_id`），每次请求都把系统提示重新注入到第一条 user，会话重放后可能累积多条 `[SYSTEM INSTRUCTIONS]`。
   - 影响：多轮后系统提示/思考提示重复堆积，token 膨胀，少数模型可能因上下文或提示位置敏感产生行为漂移；`truncate_upstream_messages` 是字符级截断，不会去重。
   - 建议：
     - 会话首消息重放（`redirect_contents`）时，若首条已带 `[SYSTEM INSTRUCTIONS]`，不重复注入；（若改 `session.rs` 则超出"错误可操作化"范围，需按 主题拆分提交）
     - 或：在 `handle_claude_messages` 组包时，若 `first_user` 已存在于历史（来自会话回放）且已含 `[SYSTEM INSTRUCTIONS]`，跳过注入。
   - （OpenAI 路径 `api.rs:705-723` 是同样的既有模式；本轮若只改 Anthropic，两个路径的差异会更大，建议一并处理或至少文档标注差异。）

4. **`thinking` 仅在 Anthropic 路径生效**（一致性提醒）
   - 位置：`src/api.rs:1538-1547` 仅在 `handle_claude_messages`；`src/api.rs:864-871`（OpenAI chat/completions）与 `src/api.rs:1889-1894`（responses）无 thinking 字段。
   - 现状：Codex/OpenAI 客户端原生是 `reasoning_effort`/`effort`，不会发 `{"type":"enabled","budget_tokens":N}`，因此功能上可接受；但若未来有客户端把 thinking 透传到 chat/completions，会被静默忽略。
   - 建议：在 `ChatRequest`/`ResponsesRequest` 加 `#[serde(flatten)] extra` 或显式 `thinking: Option<Value>` 并走同一条 `resolve_effort`（低优先）。

### P2（可滞后，建议排入下一轮）

5. **429 判定只有子串匹配，无结构化 error.code**
   - 位置：`src/upstream.rs:135-137`（HTTP 429 明确）；`src/upstream.rs:396`（流内 errorText 关键词匹配）
   - 现状：流内 `errorText` 若不说 "429/rate limit/限流"，`describe_upstream_error` 会落到「未知」分支（`上游返回错误事件（可能被限流或模型不可用）`），仍是不可操作文案。
   - 建议：给 `NonstreamResult`/SSE 转换器加 `error_type`（或 code）字段，`describe_upstream_error` 增加已知 phases：`credit`/`paused`/`rate_limited`/`unknown`，与 `ApiError::err_type` 对齐；纯 429 判定改为 `status==429 || code=="rate_limit" || 关键词`。

6. **422/400 与 paused 的 HTTP 语义**（与 erro 分类一致）
   - 位置：`src/upstream.rs:139-144`（非 2xx 统一 anyhow）、`src/api.rs:1100/1166/1231-1234`（paused 短路）
   - 现状：上游 `paused/容量不足` 通常带 4xx（如 400/422），网关仍按 `Upstream(502)` 返回；`is_model_paused_error` 能匹配 `paused`/`overloaded` 等，但非流式错误事件若只有 `errorText` 且没有 HTTP 前缀，仍然只进入「未知」。
   - 建议：把 `is_model_paused_error` 的命中前置（先于 429/未知）——当前 `describe_upstream_error` 顺序正确，但 `try_rounds`/`upgrade_retry` 只有 `is_model_paused_error` 短路，未用 `describe_upstream_error` 分类错误（`api.rs:1100-1104` 等）。可统一成 `upstream_error_kind` 供 metrics 与文案复用（`api.rs:322-334` 目前只按 429/timeout/other 分类，未覆盖 credit/paused）。

7. **`describe_upstream_error` 返回 `&'static str`，无法携带动态详情**
   - 位置：`src/upstream.rs:386-401`
   - 现状：固定文案，不含具体模型/出口/原始 errorText；调用处 `api.rs:942-945` 已拼 `原始 em` 与固定文案，但固定文案里的「每 24h UTC 日约 20 次」与 `config.hourly_per_ip`/`direct_fallback_quota` 绑定（`config.example.json` 里 `hourly_per_ip:20`、`direct_fallback_quota:10` 可能被改）。
   - 建议：文案中「20 次」改为引用配置值（函数签名改为接收 `hourly: usize` 或在调用处补配置值）；避免文案与配置漂移。

8. **面板：focus-visible 已达标，aria 仍不完整**
   - 位置：`src/web.rs:27`（focus-visible 已统一）、`src/web.rs:75-78`（nav 无 `role="tab"`）、`src/web.rs:199-216`（toast/加载态无 aria-live）、`src/web.rs:205-208`（setBtnBusy 改 text 并 disabled，无 aria-busy）
   - 现状：`button:focus-visible/input/...` 统一样式已存在；但
     - nav tab 没有 `role="tablist"/"tab"` + `aria-selected` 只在 JS 更新、初始 HTML 无 `aria-selected` 属性；
     - `toast()` 修改文本但无 `aria-live="polite"`，屏幕阅读器收不到错误/刷新提示；
     - `setBtnBusy` 无 `aria-busy`，加载中状态对屏幕阅读器不可感知；
     - 表格 `<th>` 无 `scope`，模型/代理表对读屏不友好；
     - `.guide-box` 用 `white-space:pre-wrap` 的 `<div>` 展示代码块，无 `role="code"`，且 `guide-curl` 无 label。
   - 建议（P2，低成本）：
     - nav 加 `role="tablist"` + 初始 `aria-selected`，JS 已有切换逻辑（`api.rs` 内部 web.rs:211-218）加上 `aria-controls`；
     - `#toast` 加 `aria-live="polite"`；
     - `setBtnBusy`/`selfCheck` 用 `aria-busy` 替代仅改文本；
     - 给 `<th>` 加 `scope="col"`，`guide-curl` 加 `aria-label="curl 示例代码"`。
   - （P1 不做；仅提示，因上轮已做 focus-visible，本轮达标基线 OK。）

9. **README/API_CONTRACT/UI guide 未提及 thinking 参数**
   - 位置：`docs/API_CONTRACT.md:76-79`、`README.md:58-63`、`src/web.rs:161-162`
   - 现状：文档只写 `effort`，无 `thinking` 字段说明；启用 thinking 后用户不知道「thinking=enabled → effort=deep」，也不知道 budget 被忽略。
   - 建议：文档补一段：「`/v1/messages` 支持 `thinking`（`{"type":"enabled","budget_tokens":N}`），语义映射为 `effort=deep`，`budget_tokens` 当前不生效；流式响应中 `thinking_delta` 为推理内容」。

10. **测试缺口：Anthropic SSE error 分类、thinking 映射 E2E**
    - 位置：`src/protocol/anthropic_sse.rs:441-455`（只有 `限流` 文案断言）、`src/upstream.rs:474-506`（describe/resolve_effort 单测）
    - 现状：resolve_effort 单测已覆盖 enabled/disabled/None/unknown；但无「云端真实 errorText → describe_upstream_error」断言（上游真实 credit 文案 `run out of API credit` 有覆盖）；`collect_nonstream` 无测试。
    - 建议：加 `anthropic_sse` 流式 error 分支测试（含 credit/paused/429 三种 errorText），断言输出包含可操作中文；可选加 `collect_nonstream` 测试。

### 无（未发现）

- 未见硬编码凭据、密钥、token 提交到源码；config.json 已在 git 跟踪外（`.gitignore` 含 config.json，但 `config.example.json` 仍在）——本次 diff 未涉及。
- 未见 SQL 注入/路径穿越/XSS 新引入（web.rs 有 `esc()` 转义，未新增危险 `innerHTML` 使用；`__API_KEYS_JSON__` 注入已在上轮处理）。
  本次未重新跑测试/构建，仅静态审查；若改 `api.rs`/`upstream.rs`/`anthropic_sse.rs` 需 `cargo fmt` + `cargo clippy` + `cargo test` 回归（README `cargo test` 描述 57 个测试）。

## 建议实现（按优先级）

- P1-1：`anthropic_sse.rs` `error` 分支接入 `describe_upstream_error`（输出 `[上游错误: 可操作提示]\n原始: …`），补 SSE error 分类测试。
- P1-2：`resolve_effort` 至少把 `thinking_type`/`budget` 传给上游做结构化（若决定不支持，则把「budget_tokens 忽略」写文档）；对 `StreamRequest` 增加 `thinking: Option<Value>` 由 `UpstreamClient::stream` 原样附加（注意只对 Anthropic 路径生效）。
- P1-3：多轮历史系统提示去重（`session.rs` 或 `handle_claude_messages` 注入前检查首条是否已含 `[SYSTEM INSTRUCTIONS]`）。
- P2-5：`describe_upstream_error` 增加 `error_type`/code 判定（`rate_limit`/`credit`/`paused`/`unknown`），供 metrics 与文案复用。
- P2-8：面板 aria 补全（tablist/tab、aria-live、aria-busy、scope、aria-label）。
- P2-9：`docs/API_CONTRACT.md`/`README.md`/`web.rs` guide-box 补 thinking 参数说明。
- P2-10：补 `anthropic_sse` error 分类测试；`collect_nonstream` 单元测试。
- 验证路径（合入前）：`cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test`（含 `resolve_effort_thinking_map`/`describe_upstream_error_classifies`）。

## 附注

- 只读审查，未做任何 git 操作，未修改 `src/`；唯一写文件为本文档 `docs/audit/CODE-REVIEW-2026-09-27-b.md`。
- 审查期间工作区 `src/api.rs`/`src/upstream.rs` 被外部更新（`resolve_effort` 重构 + describe 扩展），本报告以最终刷新后的行号为准；若后续再变更，请重新核对行号。

## 审查证据 / 验证记录

- 基线：`git log --oneline -1` → `fbbc6b8 release: v0.1.15（面板 UX + CI Actions v5 + 工程卫生）`；`git status` 显示 `src/api.rs`、`src/upstream.rs`（+ 上轮后新增 `src/web.rs`）未提交修改，未做任何 git 操作。
- 工作机制核对：`AnthropicRequest { model, messages, stream, max_tokens, system, tools, tool_choice, metadata, effort(String, default=balanced), thinking(Option<Value>) }`（src/api.rs:1342-1366）；`handle_claude_messages` 经 `resolve_effort` 映射后把 `effort` 填入 `StreamRequest.effort`（src/api.rs:1547、1622）。
- HAR 证据（抓包快照）：上游 POST /api/open body 为 `{"model":...,"effort":"balanced"|"deep",...,"messages":[{...}],...}`（www.tryingopen.com.har L493/L957/L1636），SSE 事件含 `reasoning-start/reasoning-delta/reasoning-end`（07cl9ce_x7idy.js 事件 schema：`sb("error"),errorText:aN()`，流内错误只有 `errorText` 字符串、无结构化 code）；`messageMetadata` 含 input/output/total/reasoningTokens（README L134 亦有记录）。
- 结论依据：`resolve_effort` 单测（src/upstream.rs:494-506）与 `describe_upstream_error_classifies`（474-491）均已存在且断言正确；流式错误路径 `anthropic_sse.rs:191-204` 未接入 describe（相对非流式 `api.rs:1701-1704` 可见）。
- 主力未验证项：`cargo test` / `cargo build` 未运行（只读审查）；面板 aria 未做浏览器实测（静态 DOM/CSS/JS 核对）。
## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0     | pass   |
| HIGH     | 2     | warn   |
| MEDIUM   | 3     | info   |
| LOW      | 5     | note   |

Verdict: WARNING — 2 HIGH（P1）应先解决（Anthropic 流式错误未可操作化；thinking mapping 语义/预算缺失 + 系统提示多轮重复）。
