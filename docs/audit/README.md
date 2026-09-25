# 审计台账 / Audit Registry

> 目标：每次改动前先查本表，命中已验范围不重复跑；每轮审计在对应文件留证据。
> 台账编辑遵循「防重复」原则（用户要求）：记录检查 ID、范围、状态、证据文件、最后运行时间。

## 检查矩阵

| ID | 范围/主题 | 状态 | 证据文件 | 最后运行 |
|---|---|---|---|---|
| N1 | 架构/数据流审计 | ✅ 已修（遗留见下） | docs/audit/N1-architecture-audit.md | 2026-09-24 |
| N3 | 性能压测（并发 5） | ✅ 完成 | docs/audit/N3-perf-load-test.md | 2026-09-24 |
| N4 | 安全审计 | ✅ 完成（后续见 B4/D4） | docs/audit/N4-security-audit.md | 2026-09-24 |
| N5 | UI/UX 审计 | ⚠️ 快照过期（见 C4 INFO-7） | docs/audit/N5-ui-ux-audit.md | 2026-09-24 |
| P1-P6 | 公网限流/熔断/metrics/优雅停机/文档 | ✅ 完成 | workflow_status.md | 2026-09-25 |
| F1-F10 | 终局审计修复 | ✅ 完成（F9 凭据历史重写待用户授权；F10 见 B1 已修） | workflow_status.md | 2026-09-25 |
| S1-S4 | 终局第三轮 | ✅ 完成 | workflow_status.md | 2026-09-25 |
| R1-R5 | v0.1.10 工具/Responses | ✅ 完成 | workflow_status.md | 2026-09-25 |
| **A4** | 架构/数据流（第 4 轮） | 🟡 修复中（Batch-2） | docs/audit/A4-architecture-dataflow.md | 2026-09-26 |
| **B4** | 生产安全/加固（第 4 轮） | 🟡 修复中（Batch-1/3） | docs/audit/B4-security-hardening.md | 2026-09-26 |
| **C4** | 前端/契约/UX（第 4 轮） | 🟡 修复中（Batch-3） | docs/audit/C4-frontend-contract.md | 2026-09-26 |
| **D4** | 盲点/文档资产（第 4 轮） | 🟡 修复中（Batch-4） | docs/audit/D4-blindsight-docs.md | 2026-09-26 |

## 已验证（无需重复跑，除非改动涉及）
- 依赖：cargo audit 本地 advisory-db（1269 条）扫 243 依赖 0 已知漏洞（2026-09-26）。如 Cargo.lock 变更需重跑。
- 密钥泄漏：git 全历史 + 工作区无真实凭据（B4 全量核过）；CI 已含 gitleaks。若新增 docs/workflow 含占位符注意保持。
- 门禁：fmt/clippy/44 tests 绿（2026-09-26 Batch-1/2/3 后）。每次代码改动复跑一次门禁。
- 公网 E2E：healthz/UI/models/chat/Anthropic/429/503/metrics（P6，2026-09-25）；公网当前状态会漂移，仅作参考。

## 约定
- 新增审计/修复请在对应 docs/audit/*.md 与 workflow_status.md 记录，并在此表加一行。
- 已修复的审计项在 workflow_status.md 标记 ✅ 并链到本台账，避免下次再当未修事项审计。
