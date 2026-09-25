# A4 架构/数据流审计报告

> 审计角色：只读审计子代理 A（架构/数据流/正确性）
> 日期：2026-09-26　证据基准：HEAD=10f0247（`git log -1`）
> 只读约束：未修改任何源文件，未 commit；仅整理本报告
> 范围：src/api.rs, src/upstream.rs, src/proxy_pool.rs, src/free_proxy.rs, src/session.rs, src/protocol/*

## 结论摘要
- BLOCKER：0（MAJOR 中有 2 项可致客户端假成功/流中断）
- MAJOR：8
- MINOR：7
- 编译门禁：fmt/clippy 通过，`cargo test`（lib+tests）正常；本机 doctest 因 rustdoc 缺失无法跑（环境问题，不影响 CI）

## MAJOR（须修复）

### A1. Anthropic 图片被重复注入到每个 user 消息
- 位置：`src/api.rs:1283-1287`
- 问题：循环每个消息时 `anthropic_image_parts(&body.messages)` 扫描整个会话，把「全部图片」追加到「每一个 user 消息」上。3 个 user 消息各带 1 图 → 上游收到 9 个附件，语义错误且浪费额度/带宽。
- 修复：只取当前消息自己的图片（按 `m.content` 解析），并全局去重（图片仅在与它所在的 user 消息一起出现时注入一次）。

### A2. 非流式路径把上游 SSE "error" 事件当成功返回
- 位置：`src/api.rs:1033-1061`（collect_nonstream）、`src/api.rs:923-950`（Ok 分支）
- 问题：`data` 里 type=error 时仅 `break`，返回 200 + 部分/空文本；usage 不更新、熔断不计失败、代理标成功。客户端收到 200 的空答复（假成功）。
- 修复：collect_nonstream 检测 error 事件 → 返回 Err(ApiError::upstream(...))；调用方走错误分支（记 metrics/日志、breaker 5xx 计数按需）。

### A3. Anthropic 流式 error 事件后缺 message_stop
- 位置：`src/protocol/anthropic_sse.rs:188-199`
- 问题：type=error 时 `finished=true` 并返回一个 text_delta 帧，但随后 poll 直接 `Poll::Ready(None)`（`anthropic_sse.rs:88-95`）→ 流没有 message_delta/message_stop，客户端挂起等待终止事件。
- 修复：error 事件后也发 message_delta + message_stop（stop_reason=end_turn）或统一走 stop_events 并附带错误文案。

### A4. 流式读取错误被静默吞掉
- 位置：`src/protocol/stream.rs`（BytesReader err→eof）、`src/api.rs:1033`（unwrap_or(0)）、openai_sse Err→done
- 问题：上游连接中断/超时在读取层被当成正常 EOF，流以 [DONE]/message_stop 正常结束，客户端无法区分「正常结束」与「截断」。非流式同样被吞。
- 修复：读取层保留 Err；SSE 转换在 Err 分支发出 error 事件（OpenAI：error SSE + 停止；Anthropic：error + stop_events），非流式转 HTTP 5xx/502。

### A5. 并发许可 accounting：gate 在响应头到达即释放
- 位置：`src/proxy_pool.rs` mark_success/mark_failure 在响应头阶段调用 release_slot；`src/protocol/openai_sse.rs` 消费 body 时并发上限已释放
- 问题：`max_concurrent_requests` 只约束「发起等待响应头」，SSE 长连接 body 阶段不计数；同代理 2 个并发时先完成者 remove 整个 permits Vec（提前释放另一请求的许可）。低概率越限/长流堆积。
- 修复：改为「流结束才释放」（把 release 钩子挂到 SSE 消费完成/终止），并让 permits Vec 按实际计数释放（inflight==0 才 remove）。

### A6. 上游消息长度截断对「第一条超长消息」无效
- 位置：`src/api.rs` truncate_upstream_messages（`drop_up_to` 仅当 >0 才生效，首条超限且有多条消息时 drop_up_to=0 → 不截断）
- 问题：首条 user 超长 16000 时配合后续历史仍然不截断 → 上游 413 chat-too-long（本机已修复「单条超长也截断」，但此处收尾仍漏）。
- 修复：把「单条超长」与「整体超预算丢旧」逻辑合并：从旧到新累计，若单条超限则对该条按 char_boundary 截断。

### A7. Anthropic 路径未做上游长度截断 / max_tokens 未使用
- 位置：`src/api.rs:1255-1310`（build up_msgs 未调 truncate）、AnthropicRequest.max_tokens 解析未使用
- 问题：长对话经 Anthropic 端点 → 上游 413（与 OpenAI 路径行为不一致）；max_tokens 静默忽略。
- 修复：Anthropic 组装后复用同一 truncate 逻辑；max_tokens 至少透传/截断提示（上游不支持则文档明示）。

### A8. Anthropic SSE 协议保真度：model=""、content_block index、usage 缺失
- 位置：`src/protocol/anthropic_sse.rs:214-244`
- 问题：message_start.model 硬编码空串（调用方传 model 但被 `_model` 丢弃）；thinking 用 index=0、tool_use 用初始 block_index=1（无 thinking 时首个 block 缺 index=0）；usage 恒 0 不具备参考性。
- 修复：start_event 接收 model；block 索引按实际发出顺序分配；usage 尽力从 upstream metadata 填入。

## MINOR
1. `per-proxy` 提前释放（并入 A5）。
2. `try_rounds` 末次错误判断仅看最后一次错误（早先 429 被网络错误覆盖 → 错误分类不精确；上游计数影响小）。
3. 代理路径 HTTP 客户端 read_timeout 硬编码 120s，未用 config.request_timeout_sec（默认一致，配置不同步时行为不符）。
4. `extract_image_parts` 顶层返回值被忽略（`_images`；死代码）。
5. `get_sticky` 目前无调用方（死代码，保留语义注释）。
6. `log_request` 每次调用重新编译 regex（perf 微小）。
7. 流式长间隔（>120s 无 chunk）会被 read_timeout 切断 —— 设计约束，文档需明示。

## 证据
- `src/api.rs:1283-1287`（图片重复循环）；`src/api.rs:1033-1061`（error 仅 break）；`src/protocol/anthropic_sse.rs:188-199,88-95`（错误后无 message_stop）；`src/api.rs:923-950`（Ok 分支把空文本当成功）；`src/proxy_pool.rs:413-446,481-505`（release 时机）；`src/api.rs` truncate_upstream_messages（drop_up_to 条件）。
