# 需求追踪矩阵（终局闭环总审计 · 2026-09-26 · v0.1.12）

> 依据用户主控代理指令模板：显式/隐式/验收/非功能需求逐条映射到真实实现与证据。
> 状态：✅ 已闭环 / 🟡 部分闭环 / ❌ 未闭环（含阻塞说明）。

## 显式需求

| # | 需求 | 映射（模块/文件/接口） | 状态 | 证据 |
|---|---|---|---|---|
| E1 | 全面审查/查漏补缺（前后端衔接、功能完整性、闭环完美性） | 4 子代理审计 A/B/C/D + FINAL-GAP + 主线程复核 | ✅ | docs/audit/{A4,B4,C4,D4,FINAL-GAP} |
| E2 | 生成 workflow_status.md 并循环直到闭环 | workflow_status.md（v0.1.10→v0.1.12 全程记录） | ✅ | workflow_status.md |
| E3 | 节点验收（单测/覆盖率/E2E/UI/UX 真实签收） | 53 tests + 本地带 key 真实上游 E2E + 公网无 key 探活 | ✅ | E2E-local/E2E-2026-09-26 |
| E4 | 提交推送到仓库（main） | git push main（v0.1.11/v0.1.12） | ✅ | 远端 68efeb2 |
| E5 | 创建发行版 | GitHub Release v0.1.11/v0.1.12 | ✅ | RELEASE-VERIFY + gh release view |
| E6 | 真实 E2E 测验验收审计 | 见 E3 + 发布资产 SHA 核对 | ✅ | RELEASE-VERIFY/E2E×2 |
| E7 | 使用 Spec Kit 技能/规范 | .specify/ 存在；speckit 技能本地可用 | ✅ | .specify/、docs/audit 规范化报告 |
| E8 | 主动补位/深度反向修复 | truncate bug 测试驱动发现并修复 | ✅ | api.rs tests + commit 7fc3017 |

## 隐式需求

| # | 需求 | 映射 | 状态 | 证据 |
|---|---|---|---|---|
| I1 | 可运行/可调用/可使用 | 三协议端点 + 面板 + 公网部署 | ✅ | 本地 E2E 真实上游 200 |
| I2 | 不要伪实现 | 所有声称均有命令/响应证据；FAIL/EXPECTED 如实标注 | ✅ | E2E 报告 |
| I3 | 文档同步 | README/CHANGELOG/API_CONTRACT/DOCKER/workflow_status 全部 v0.1.12 | ✅ | 本轮 sync commit |
| I4 | 链路完整 | chat/messages/responses 均真实打上游返回 200 | ✅ | E2E-local T4-T7 |
| I5 | 主动补齐遗漏 | rusqlite 死依赖/死配置/truncate bug/限流口径 | ✅ | FINAL-GAP G1-G7 |

## 验收导向需求

| # | 需求 | 映射 | 状态 | 证据 |
|---|---|---|---|---|
| A1 | 一次调用尽量跑通 | chat 非流式/流式 200 | ✅ | E2E-local T4/T5 |
| A2 | UI/按钮/功能真实接通 | 面板重写（API Key UI/指南实时/表格） | ✅ | C4 + web.rs |
| A3 | 非付费资源尽量真实验证 | 真实上游对话/限流/鉴权全部实测 | ✅ | E2E-local |
| A4 | md/README 主动更新 | v0.1.12 全量同步 | ✅ | 本轮 sync |

## 非功能需求

| # | 需求 | 映射 | 状态 | 证据 |
|---|---|---|---|---|
| N1 | 兼容性 | OpenAI/Anthropic/Responses 三协议 | ✅ | E2E-local T4-T7 |
| N2 | 稳定性 | 限流/熔断/优雅停机/并发门控 | ✅ | prod_guard + 测试 |
| N3 | 安全性 | 鉴权/SSRF/脱敏/clear 权限/metrics 鉴权 | ✅ | B4 + E2E 安全拒绝 |
| N4 | 可部署性 | Docker/CD/nginx/SOP | ✅ | DOCKER/DEPLOYMENT_SOP/NGINX_DEPLOY |
| N5 | 可维护性 | 审计台账防重复/模块化 | ✅ | docs/audit/README |
| N6 | 可排障性 | /metrics + 结构化日志 + 错误分类 | ✅ | prod_guard + API_CONTRACT |
| N7 | 一致性 | 版本/限流口径/文档-代码一致 | ✅ | FINAL-GAP G4/G7 |

## 未闭环（诚实标注）
| # | 项 | 阻塞 | 状态 |
|---|---|---|---|
| U1 | 公网带生产 key 全量调用 E2E | 缺生产 API key（用户未提供） | 🟡 待用户提供 key 后 `API_KEY=<key> scripts/e2e_smoke.sh https://try.hwhcie.bond` |
| U2 | config.json 移出 git | 需用户拍板（当前无凭据无害） | 🟡 建议项 |
| U3 | deploy.sh SSH key-only | 需服务器运维操作 | 🟡 B4 MINOR-9 |
