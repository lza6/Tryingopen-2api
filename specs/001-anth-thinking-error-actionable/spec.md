# Feature Specification: Anthropic thinking 参数映射 + 错误可操作化 (v0.1.16)

**Feature Branch**: `main`（用户规范：所有提交/推送/发行版走 main）
**Created**: 2026-09-27
**Status**: ✅ 已实现并交付（v0.1.16）

**Input**: 用户要求——Claude Code（Anthropic 客户端）接入时 `thinking` 参数语义对齐；上游错误（信用耗尽/模型暂停/429）给调用方可操作的中文提示；全仓文档/契约同步。

## User Scenarios & Testing

### User Story 1 - Claude Code 传 thinking 参数能正确映射 effort (Priority: P1)

Claude Code 默认可能在请求体带 `thinking: {"type":"enabled","budget_tokens":N}`。网关应把"需要思考"语义映射到上游 effort 档位，而不是忽略。

**Why this priority**: Anthropic 客户端是核心接入方（Claude Code），语义错位会让思考类模型不思考或过度思考。

**Independent Test**: `POST /v1/messages` 带 `thinking:{type:enabled}` → 日志/上游请求 effort=deep；带 disabled/不带 → 保持 effort。

**Acceptance Scenarios**:
1. Given 客户端发 `{"thinking":{"type":"enabled"}}` 且未显式 effort, When 网关构造上游请求, Then effort=deep 且系统提示含"请逐步推理"
2. Given 客户端发 `{"thinking":{"type":"disabled"}}`, When 网关构造上游请求, Then effort 保持默认 balanced
3. Given 客户端未传 thinking, When 网关构造上游请求, Then effort 保持原值

### User Story 2 - 上游错误给可操作提示 (Priority: P1)

上游返回"API credit 耗尽 / 模型暂停 / 429"时，调用方应得到"为什么 + 怎么办"的中文提示，而非裸英文。

**Why this priority**: 免费上游配额受限，用户最常遇到的错误必须有可执行指引，否则接入口碑崩。

**Independent Test**: 三类错误文案断言（单元测试）+ 流式/非流式路径一致。

**Acceptance Scenarios**:
1. Given 上游 `run out of API credit`, When 网关 502 响应, Then 文案含"免费额度已耗尽…请稍后或配置代理"
2. Given 上游"模型暂停", When 网关响应, Then 文案含"请换一个模型重试"
3. Given Claude Code 走流式, When 上游 error 事件, Then 同样输出可操作中文提示（与非流式一致）

### User Story 3 - 多轮会话系统提示不重复堆积 (Priority: P2)

多轮工具/对话回放时，历史首条 user 已含上一轮注入的 `[SYSTEM INSTRUCTIONS]`，重新注入会重复堆积。

**Why this priority**: 长会话可用性（上游 16k 截断前提示膨胀）。

**Independent Test**: strip_system_block 单测（幂等去重）。

**Acceptance Scenarios**:
1. Given 历史消息含旧系统块, When 网关注入新系统提示, Then 旧块被剥离，只保留一份
2. Given 无系统块, When 注入, Then 原样保留用户内容

## Requirements

### Functional Requirements
- **FR-001**: AnthropicRequest 接受 `thinking` 字段（serde default，未知时静默忽略）
- **FR-002**: `resolve_effort(thinking, effort)`：enabled→deep；disabled/None/未知→保持原 effort
- **FR-003**: thinking enabled 时注入"请逐步推理"系统提示（OpenAI/Anthropic 双路径）
- **FR-004**: `describe_upstream_error(err)` 分类：credit 耗尽/模型暂停/429/未知 → 可操作中文
- **FR-005**: 非流式 502 与 Anthropic 流式 error 事件文案一致（可操作中文）
- **FR-006**: `strip_system_block` 幂等去重（多轮重放不堆积）

### Key Entities
- `AnthropicRequest.thinking`: `Option<serde_json::Value>`（enabled/disabled/其他）
- `resolve_effort`: `(Option<&Value>, &str) -> String`
- `describe_upstream_error`: `(&str) -> &'static str`
- `strip_system_block`: `(&str) -> String`

## Success Criteria

### Measurable Outcomes
- **SC-001**: 4 分支 thinking 映射单测全绿（resolve_effort）
- **SC-002**: 4 分类错误文案单测全绿（describe_upstream_error）
- **SC-003**: 5 分支 strip 幂等单测全绿（strip_system_block_idempotent）
- **SC-004**: 全量门禁 60 tests 绿（39 lib + 13 models + 8 proxy）
- **SC-005**: 真实 E2E：responses 流式 200 + 502 可操作文案被真实上游触发验证
- **SC-006**: 生产 healthz 200 + version=v0.1.16

## Assumptions
- 上游只暴露 effort（balanced/deep/low），无 thinking/budget 通道 → budget_tokens 无法透传，文档声明不支持（诚实边界）
- 每 24h UTC 日约 20 次配额是上游硬约束，E2E 正路径 chat 200 由历史轮次证明（R020_OK/R015_OK），本轮上游配额耗尽期间验证负路径
- Constitution 模板未填充为历史遗留，不阻塞本轮交付

## Edge Cases
- thinking type 未知值 → 保持原 effort（宽容）
- 未闭合 `[SYSTEM INSTRUCTIONS]` 块 → 剥到结尾
- describe 未命中任何已知分类 → 通用"上游返回错误事件"
