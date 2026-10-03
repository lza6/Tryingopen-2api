# TryingOpen2API Constitution

## Core Principles

### I. 真实闭环优先（NON-NEGOTIABLE）
Every claimed feature must have executable code, runnable commands, and reproducible verification. "理论可行"、"有代码片段"、"有界面按钮" is not "已完成". Mock only proves isolation, not integration.

### II. 门禁铁律
Every change must pass: `cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features -- -D warnings` → `cargo test --tests --all-features`. All green required before commit. Test count must be recorded and synced across docs.

### III. 上游配额红线
tryingopen.com 每 24h UTC 日约 20 次/IP 是硬约束。任何自动化压测/批量调用必须走代理池且控制总量，或使用 mock 上游。禁止默认压测猛打 `/v1/chat`。

### IV. 面板无构建链
`src/web.rs` 内嵌 HTML/JS 无前端构建。任何 web.rs 改动必须真实浏览器（或 node JS 语法校验 + 结构断言）验证，不能只看字符串拼写。

### V. 诚实披露
结论使用四级标签：`已验证`（实际运行）/ `静态确认`（读代码）/ `合理推断`（间接证据）/ `待验证`（缺环境）。外部受限项（真实上游配额、付费 API、生产凭据、服务器运维）必须明确披露边界，不包装成已完成。

### VI. 不重构、小步走
只做小步、可回滚、可单测的行为级改进；大改架构先写 ADR 并经确认。不"顺手"重构没坏的东西。

### VII. 记忆台账防重复验证
每次验证记录测试范围与已优化点（`优化迭代计划/验收记录-*.md`、`workflow_status.md`、`.claude/memory/`）。下次改到相关模块先读台账，不重复跑已验证过的全量验证。
