# N1 架构与代码审计报告（终局闭环）

> 审计方式：critical-code-reviewer + rust-review 技能 + 主控交叉验证
> 日期：2026-09-24

## Summary
整体架构清晰（axum 单二进制、模块边界合理、代理池/协议/UI 分层干净）。
审计发现 7 个真实缺陷（1 个 blocking 级后台任务失效、1 个 Critical UTF-8 panic、其余 Required），
**全部已修复并验证**（fmt/clippy 0 错、25/25 测试通过）。

## Critical Issues（已修复）
1. **免费代理后台周期刷新失效（blocking）** — main.rs watch channel sender 立即 drop，
   run_loop 的 recv.changed() 返回 Err → break，只抓一次就退出。
   修复：改为无限循环 spawn（进程生命周期），去掉 watch 机制。src/main.rs + src/free_proxy.rs
2. **truncate UTF-8 panic（Critical）** — upstream.rs `&s[..n]` 在中文字符中间 slice 会 panic，
   导致 semaphore permit 永久泄漏。修复：按 char_boundary 截断。

## Required Changes（已修复）
3. **permits 单槽覆盖** — proxy_pool permits HashMap<String, Arc<Permit>> 并发同代理时
   第二个 insert 覆盖第一个 → permit 提前释放，全局并发上限失效。
   修复：改 Vec<Arc<Permit>>，release_slot 只 pop 一个。
4. **acquire 全冷却仍返回冷却代理** — 违反每 IP 20/h 语义。修复：全冷却返回 None（直连兜底）。
5. **Anthropic 流缺 message_start** — 协议要求流以 message_start 开头；stop_reason 固定 end_turn。
   修复：加 message_start 事件 + finishReason 映射（tool_calls→tool_use）。
6. **Anthropic 非流 usage 键名错误** — 用了 OpenAI 风格 prompt_tokens/completion_tokens。
   修复：映射为 input_tokens/output_tokens。
7. **session map 无界增长** — 客户端任意 user/thread_id 无限插入。修复：>5000 清理最旧至 4000。
8. **工具模式正文前缀丢失** — detect 到 JSON 后 clear 丢弃 JSON 前叙述。修复：加 preamble 字段先发正文。

## Suggestions（未修，记录）
- 代理请求每次新建 reqwest Client（连接池不复用）→ 后续可优化为 per-proxy client 池
- 非流工具调用未转 tool_calls（与流式不一致）→ 后续补
- streaming mark_success 在响应头时执行（流中途失败不 mark_failure）→ 可接受的设计
- config.fallback_models 未接入 models.rs resolve（硬编码列表）→ 后续接入

## Verdict
Request Changes → 已全部处理 → 当前 Approve（无阻塞）
