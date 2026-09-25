# ADR-001: 选择 Rust(axum) 单二进制网关架构

- 状态：已采纳（2026-09-24）
- 背景：需要把 tryingopen.com 免费模型转成 OpenAI/Anthropic 兼容本地 API，要求零外部依赖、易部署、高并发。
- 决策：Rust + axum + reqwest + tokio，单二进制，SQLite 内嵌（未实际使用但保留），无 Docker 依赖。
- 备选：Python FastAPI（部署重、GIL 并发弱）、Node（内存高）。
- 结果：7.9MB 单 exe，无运行时依赖，Windows/Linux 可交叉编译，测试 25 项。

# ADR-002: 代理池架构（住宅+免费双源）

- 状态：已采纳
- 背景：上游每 24h UTC 日约 20 次限流，单出口无法支撑高并发。
- 决策：住宅代理文件（可信）+ 免费代理抓取（44 源）双源；acquire 按 latency/health/inflight 排序；429 冷却+退避+直连兜底；容量=可用代理数×20−已用。
- 结果：4500+ 代理，8 并发实测全成功；SSRF 防护（sanitize_proxy_url）。

# ADR-003: 完全匿名（无 Cookie/登录）

- 状态：已采纳
- 背景：tryingopen /api/open 无需认证。
- 决策：网关不做上游凭证管理（对比 tokenharbor2api 的 Cookie 池），简化架构。
- 结果：无凭证泄漏面；限流靠代理池轮换。

# ADR-004: SSE 转换层（OpenAI/Anthropic 双协议）

- 状态：已采纳
- 背景：客户端生态分 OpenAI 兼容与 Anthropic 兼容。
- 决策：独立 protocol/ 模块做上游 SSE → 两种目标协议转换（reasoning/text/tool/usage）。
- 结果：Claude Code 与 OpenAI SDK 均可直接接入；Anthropic 流含 message_start 等标准事件。
