# Feature Specification: 终局审计收尾 + 交付资产沉淀（final-audit-closeout）

**Feature Branch**: `main`（用户规范：所有提交/推送/发行版走 main）
**Created**: 2026-09-27
**Status**: Draft → In Progress
**Input**: 用户要求——对 TryingOpen2API v0.1.17 做全面深度终局审计、盲点扫描、前后端衔接查漏补缺、独立审查循环复验、产出 HTML 验收报告（含测验），并将本项目沉淀为可复用的 workflow/skills 资产（含记忆台账防重复验证）。

## 背景（重建事实基础，2026-09-27 实测）

- git：main @ 29f5576（v0.1.17 已提交推送），工作树干净
- 生产：try.hwhcie.bond healthz 显示 version=0.1.17 / models=24 / proxies=8936
- Release：v0.1.17 已发布（exe 6,117,888B + SHA256）
- 门禁基线：fmt ✅ / clippy -D warnings ✅ / 65 tests ✅
- 模块：api.rs(桥接) config.rs models.rs upstream.rs proxy_pool.rs free_proxy.rs prod_guard.rs session.rs errors.rs web.rs(内嵌面板) + protocol/(openai/anthropic/responses)
- 上轮已闭环：B1 协议透传、B2 U1-U3/U5、B3 usage、B4 JSON 日志、B5 压测+编译、H1 文档、安全 H1/M1/M2
- 技能：本地 Speck It（spec-kit）十件套在 `.agents/skills/`，无需联网重装

## User Scenarios & Testing

### User Story 1 - 全量盲点审计 + 前后端衔接查漏（P1）

在已交付 v0.1.17 之上，从 6 个角色视角（产品验收 / 架构审计 / 真实用户 / 调用接入方 / 部署维护 / 反向辩手）全面扫描未知未知，重点：前端面板按钮↔后端路由是否全部真实接通、API 契约与文档是否一致、异常路径、边界、兼容性、配置容错。

**Independent Test**: 审计产物 = 分级问题清单（P0/P1/P2/P3），每条带证据（文件:行/命令/复现）。
**Acceptance**:
1. Given 全量扫描后, When 汇总问题清单, Then 无 P0 遗留、P1 全修或明确阻塞
2. Given 面板每个按钮, When 追踪到后端 handler, Then 全部真实接通（无假按钮/死按钮）
3. Given 文档声称的每个端点, When curl 复核, Then 行为与文档一致（无伪声称）

### User Story 2 - 独立审查循环（P1）

主线程修复后，启动独立审查线程（不改代码），从需求完整性/逻辑正确性/边界/质量/测试/运行结果六维复验，产出修复清单交回主线程，循环直至通过。

**Independent Test**: 审查线程结论 PASS / CONDITIONAL / FAIL，附证据。
**Acceptance**:
1. Given 主线程完成修复, When 独立审查, Then 发现的问题进入修复清单
2. Given 修复后, When 复验, Then 通过或明确卡点（≤3 轮收敛）

### User Story 3 - HTML 验收报告 + 变更测验（P2）

产出 HTML 报告（上下文/直觉/做了什么/证据），底部含变更测验（用户必须通过）。

**Independent Test**: 打开 HTML 报告可读，测验题与变更一一对应。
**Acceptance**:
1. Given 报告, When 打开, Then 含上下文/直觉/变更/证据四部分
2. Given 测验, When 作答, Then 每题与真实变更对应（无编造）

### User Story 4 - 沉淀 workflow/skills + 记忆台账（P2）

把本项目整理成可复用资产：新增/更新 skill 或 workflow 文档（加入新 API/新功能时复用既有能力）；建立"验证台账"记忆（已测范围、已优化点），下次优先读取避免重复验证。

**Independent Test**: skill/workflow 文档存在且可被后续会话读取；记忆台账记录本次测试范围。
**Acceptance**:
1. Given 资产文件, When 后续会话读取, Then 能判断是否过时并定位测试范围
2. Given 台账, When 再次改到相关模块, Then 无需重复跑已验证过的全量验证

---

## 非功能性要求（贯穿）

- 兼容性：修复不得破坏三协议（OpenAI/Anthropic/Responses）+ 面板 + 代理池 + 生产保护
- 稳定性：异常路径有兜底，不 panic，不静默吞错
- 可维护性：按现有风格小步改，文档同步
- 可验证性：每项声称都有运行证据，禁止"理论可行"冒充"已闭环"
- 一致性：README/CHANGELOG/API_CONTRACT/workflow_status/验收记录口径统一

## 硬约束

1. 门禁铁律：每轮改动跑 `cargo fmt --check` → `cargo clippy -D warnings` → `cargo test --tests --all-features`
2. 前端无构建链：web.rs 改动用 node JS 语法校验 + 真实浏览器/curl 验证，不能只看字符串
3. 不重构：小步、可回滚、行为级改进；大改需 ADR
4. 外部受限项诚实披露：真实上游配额（每 24h 约 20 次）、生产凭据、付费 API
5. 版本口径：若产生新提交需 bump（v0.1.18），全仓同步
