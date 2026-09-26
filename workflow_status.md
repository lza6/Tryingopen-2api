# Workflow Status — TryingOpen2API 终局闭环总审计

> 更新：2026-09-26（v0.1.12 已发布，终局第 4 轮闭环 + 缺口清零）
> 仓库：lza6/Tryingopen-2api（main 分支，v0.1.12 已发布，CI/CD 全绿）

## 产品定位
TryingOpen2API = Rust(axum) 免费模型 OpenAI/Anthropic 兼容本地网关：
匿名上游 + 代理池（44源/4500+代理/低延迟/并发门控/容量）+ 双协议桥接 + 工具/思考/多模态 + 控制面板

## 任务节点状态（终局闭环）

| 节点 | 内容 | 状态 | 验收证据 |
|---|---|---|---|
| N1 | 架构/代码审计 | ✅ 完成 | 8 个真实缺陷全修复 + 终局审计 10 项新修复，fmt/clippy/37tests 绿，CI 全绿 |
| N2 | API 契约文档 | ✅ 完成 | docs/API_CONTRACT.md（认证/错误/端点/防坑） |
| N3 | 性能压测 | ✅ 完成 | docs/audit/N3（真实数据：并发 5 全成功、无崩溃） |
| N4 | 安全审计 | ✅ 完成 | cargo audit 0 漏洞 + SSRF 纵深防护修复 |
| N5 | UI/UX 审计 | ✅ 完成 | 用户路径验证 + 移动端/空态/减动效 |
| N6 | 部署/SOP | ✅ 完成 | docs/DEPLOYMENT_SOP.md（构建/配置/排障/交接） |
| N7 | 产品头脑风暴 | ✅ 完成 | docs/N7-product-brainstorm.md |
| N8 | 文档/ADR/索引 | ✅ 完成 | docs/INDEX.md + docs/adr/ |

## 关键修复（终局审计发现）
1. 免费代理后台周期刷新失效（watch channel sender 提前 drop）→ 改无限循环
2. truncate UTF-8 panic（&s[..n] 中文边界）→ char_boundary 截断
3. 代理池 permits 单槽覆盖 → Vec 多槽，release 只 pop 一个
4. acquire 全冷却仍返回代理 → 返回 None 直连兜底
5. Anthropic 流缺 message_start/stop_reason → 补全
6. Anthropic 非流 usage 键名 → input_tokens/output_tokens
7. session map 无界增长 → >5000 清理
8. 工具模式正文前缀丢失 → preamble 字段
9. SSRF：住宅/免费代理统一 sanitize 公网地址校验
10. 请求体无限制 → DefaultBodyLimit 16MB

## 验证记录（防重复）
- [x] 门禁：fmt/clippy/37tests（每次改动后复跑）
- [x] CI：main push 全绿（35962399309 等）
- [x] E2E：对话/工具/多模态/容量/Anthropic message_start/无效模型降级
- [x] 依赖：cargo audit 0 漏洞
- 下次改动优先看此表 + docs/audit/* 避免重复验证同一范围

## 下一步（如继续）
- 按 N7 P0：/ui 加 curl 复制按钮、模型能力徽章
- 按 N7 P3：/metrics（Prometheus）、Dockerfile、可选 Redis 缓存
- 非流工具调用转 tool_calls（与流式一致）
- config.fallback_models 接入 resolve

## 生产就绪度补漏（公网部署后，2026-09-25）

| 节点 | 内容 | 状态 | 验收证据 |
|---|---|---|---|
| P1 | Rate Limiting（API key 限流防滥用） | ✅ 完成 | 公网实测阈值3：req1-3=200，req4/5=429 + Retry-After=3591/3590 |
| P2 | Circuit Breaker（上游故障熔断） | ✅ 完成 | 公网实测坏上游：502→连续503熔断保护→恢复配置后200 |
| P3 | Observability（/metrics + 用量统计） | ✅ 完成 | 公网 /metrics 含 requests_total 2xx=1、proxy_pool_size=4500、available=4500 |
| P4 | Graceful Shutdown（SIGTERM 平滑退出） | ✅ 完成 | 服务器日志实测：systemctl restart → received SIGTERM, graceful shutdown |
| P5 | 文档同步（部署后状态） | ✅ 完成 | API_CONTRACT/SERVER_DEPLOYMENT/config.example/workflow_status 已更新 |
| P6 | 真实验收（公网限流/熔断/metrics） | ✅ 完成 | 公网直连全链路：healthz/UI/models/chat/Anthropic/429/503/metrics/4500代理 |

## 验证记录（防重复）
- [x] 门禁 fmt/clippy/37tests（每次改动复跑，含 prod_guard + upstream 识别单测）
- [x] CI main push 全绿
- [x] 公网 E2E：healthz 200 / UI 401+200 / API key 对话 200 / /metrics
- [x] 服务器 systemd 双服务（主 + cf 隧道）
- [x] 公网直连 47831 已确认放行（curl --noproxy "*" 200）
- 本轮改动后复跑：门禁 + 公网限流/熔断/metrics 实测


## 终局闭环总审计（2026-09-25 第二轮，多 agent 并行 + 真实验收）

| 节点 | 内容 | 状态 | 验收证据 |
|---|---|---|---|
| F1 | /api/config/api-key 鉴权漏洞 | ✅ 修复 | 原无鉴权可远程清空 key；现必须带有效 key 才能管理 |
| F2 | 非流式工具调用假功能 | ✅ 修复 | collect_nonstream 检测 tool_call → 标准 tool_calls + finish_reason:tool_calls |
| F3 | /v1/models 缺 meta 字段 | ✅ 修复 | 补 tools/vision/context/price/label，UI 模型表真实渲染（去 TODO 假徽章） |
| F4 | fallback_models 死配置 | ✅ 修复 | resolve 读 config.fallback_models（空则内置默认），+测试 |
| F5 | 熔断误计 4xx | ✅ 修复 | 仅 5xx 记熔断失败；模型不存在不再误熔断整个上游 |
| F6 | Anthropic tool_choice 缺失 | ✅ 修复 | 新增 tool_choice 字段 + 注入系统提示 |
| F7 | Anthropic partial_json 非法分块 | ✅ 修复 | 一次发送完整合法 JSON arguments（Anthropic SDK 兼容） |
| F8 | 文档过度声称 | ✅ 同步 | README/config.example/API_CONTRACT 修正：每日限流语义、44源/4500截断、free_proxy 默认 true、非流工具调用说明 |
| F9 | 生产凭据泄漏 git 历史 | ⚠️ 需用户确认 | docs/SERVER_DEPLOYMENT.md 已脱敏为占位符；历史重写（filter-repo/BFG）需用户授权 |
| F10 | /metrics + /healthz 鉴权 | ✅ 已定 | /metrics 已加 key 鉴权（B1）；/healthz 保持无鉴权（仅探活，无敏感字段，设计如此） |


## 终局闭环第三轮（2026-09-25，v0.1.9）

| 节点 | 内容 | 状态 | 验收证据 |
|---|---|---|---|
| S1 | session map 无界增长 | ✅ 修复 | sweep 提取，ensure/touch 触发清理（>5000→4000） |
| S2 | redact_logs 假配置 | ✅ 修复 | log_request 接入配置（true 截断+剥离密钥，false 完整） |
| S3 | capacity 口径矛盾 | ✅ 修复 | 公网实测 used=31 ≤ total=90000 |
| S4 | Dockerfile + CD 全自动部署 | ✅ 完成 | v0.1.8/v0.1.9 均由 push main 自动部署（3m39s/3m44s） |


## v0.1.10：OpenAI 工具调用修复 + /v1/responses（2026-09-25）

| 项 | 内容 | 状态 |
|---|---|---|
| R1 | OpenAI chat 多轮工具往返（assistant.tool_calls + role=tool） | ✅ 修复，公网 200 |
| R2 | 上游 HTTP 413 对话过长（截断历史 + 立即 400 不轮换） | ✅ 修复 |
| R3 | /v1/responses 非流式（message / function_call） | ✅ 上线，公网 200 |
| R4 | /v1/responses 流式事件链 completed 去重 | ✅ 修复，completed 恰好 1 次 |
| R5 | 工具检测兼容 tool_call / function_call / tool_calls[] | ✅ 增强 |


## 终局闭环总审计 第 4 轮（2026-09-26，4 子代理并行 + 分主题修复）

> 审计报告：docs/audit/A4（架构/数据流）、B4（生产安全）、C4（前端/契约）、D4（盲点/文档）
> 修复批次：Batch-1 安全/日志 → Batch-2 协议正确性 → Batch-3 前端/契约 → Batch-4 文档/CI → Batch-5 验收

| 项 | 内容 | 状态 | 证据 |
|---|---|---|---|
| T1 | D1 log_request UTF-8 panic | ✅ 修复 | chars 脱敏替换字节切片 + 测试 |
| T2 | B4 clear 权限反转 | ✅ 修复 | 需 admin_confirm + 静态 key 禁清空 |
| T3 | B1 /metrics 无鉴权 | ✅ 修复 | 需 api key |
| T4 | B4 generate 无限自增 | ✅ 修复 | 上限 64 |
| T5 | B3 直连兜底配额 | ✅ 修复 | direct_fallback_quota 独立限流 |
| T6 | C MAJOR-1 默认面板 401 | ✅ 修复 | 会话级 key 自举 |
| T7 | A1 Anthropic 图片重复注入 | ✅ 修复 | 只取当前消息图片 |
| T8 | A3 Anthropic error 无 message_stop | ✅ 修复 | error 后发 stop_events |
| T9 | A2 非流式 error 假成功 | ✅ 修复 | 转 502 + 记 metrics |
| T10 | A8 Anthropic message_start.model 空 | ✅ 修复 | 传真实 model |
| T11 | A6/A7 截断首条 + Anthropic 截断 | ✅ 修复 | any_oversize 分支 |
| T12 | C MAJOR-2/3/4/5/6 前端 | ✅ 修复 | fetch 超时/指南实时/价格/表格/转义 |
| T13 | D3 sqlite 空壳文档 | ✅ 同步 | DOCKER.md 澄清内存会话 |
| T14 | D4 版本漂移 | ✅ 同步 | INDEX/CHANGELOG/README/API_CONTRACT |
| T15 | D6 CD 无回滚 + cancel-in-progress | ✅ 修复 | deploy.sh 原子替换+回滚；concurrency false |
| T16 | B4 MINOR config.json 跟踪 | 🟡 建议 | 推荐移出 git（历史干净，未强制） |

## 验证记录（第 4 轮，防重复）
- [x] fmt/clippy/53 tests（Batch-1/2/3 + v0.1.12 后均绿）
- [x] docs/audit/README.md 审计台账已建
- [x] 公网 E2E：无 key 探活+安全拒绝（E2E-2026-09-26）；本地带 key 真实上游（E2E-local）；带生产 key 公网全量待用户提供 key 后补跑



## v0.1.14（2026-09-26，修复 + 发布调优）

| 项 | 内容 | 状态 | 证据 |
|---|---|---|---|
| F1 | 代理 read_timeout 硬编码 120s -> config 统一 | ✅ | upstream.rs stream() 用 self.read_timeout；本地真实上游 chat 200 |
| F2 | kimi 静态价格 15.0 -> 8.5（上游实测） | ✅ | /v1/models 动态目录实测 price_per_mtok=8.5 |
| F3 | release panic=abort（产物 8.37MB -> 5.83MB/-30%） | ✅ | target/release/exe 5,320,704B（fat）→ 5.83MB（thin+abort 最终） |
| F4 | 文档漂移全修（README_en/NGINX/SOP/DOCKER/PROTOCOL/.dockerignore） | ✅ | DOCS-DRIFT 报告 P0/P1 全处理 |
| F5 | target 清理 13.4GB（debug/aarch64/tmp/audit-download） | ✅ | release 保留；样本归档 reference/release-audit |
| F6 | 门禁 + E2E | ✅ | fmt/clippy/57 tests；本地 chat 200 R020_OK；生产 healthz 200 |

## v0.1.13（2026-09-26，能力透传 + 用量可见性 + 结构化日志）


| 项 | 内容 | 状态 | 证据 |
|---|---|---|---|
| V1 | 模型能力字段透传（reasoning/messageLimit/cheaperFallbackId） | ✅ | models.rs/upstream.rs 解析 + /v1/models 实测（kimi messageLimit=5 静态、动态如实反映上游）；新增 2 测试 |
| V2 | 429 按上游 cheaperFallbackId 降级 | ✅ | api.rs try_rounds 降级分支（有界回退）；模型 429 时优先建议模型 |
| V3 | /api/usage 每 key 用量统计 | ✅ | prod_guard UsageTracker + 路由；真实 E2E 2 keys/3 请求全 OK；401 鉴权验证 |
| V4 | 面板思考徽章/降级 chip/用量摘要 | ✅ | web.rs 渲染；/ui 浏览器验证（思考列/累计请求/chip） |
| V5 | 请求日志结构化 JSON | ✅ | 实测 REQ JSON 行（key=***、took_ms、status） |
| V6 | config.local.json 深合并 | ✅ | 实测 redact_logs 覆盖生效 |
| V7 | CI release job 去重 | ✅ | ci.yml 移除重复 release job（保留 release.yml 独占） |
| V8 | 门禁 | ✅ | fmt/clippy -D warnings/57 tests 全绿（36+13+8） |
| V9 | 真实 E2E | ✅ | 本地真实上游：chat 200 E2E_OK / anthropic 200 / responses 200 RESP_OK / usage 统计 / 401 / JSON 日志 |

## v0.1.12（2026-09-26，终局第 4 轮收尾）


| 项 | 内容 | 状态 |
|---|---|---|
| G1 | truncate_upstream_messages「保留近半」方向反了（截断后丢截断目标、保留超长旧消息） | ✅ 修复（drain 旧保新）+ 3 测试 |
| G2 | rusqlite 死依赖 + sqlite_path/telemetry_path/proxies_path/precheck_concurrency 死配置 | ✅ 移除（纯 Rust，构建更快） |
| G3 | Dockerfile/compose/DOCKER.md sqlite 残留声称 | ✅ 清理（会话内存态） |
| G4 | 限流口径统一「每 24h UTC 日约 20 次」 | ✅ 全仓同步 |
| G5 | 审计台账/workflow_status 状态残留 | ✅ 全 ✅ + F10 定案 |
| G6 | 行为层测试补强（session sweep/config env/truncate） | ✅ 46 → 53 tests |
| G7 | README/ARCHITECTURE/NGINX/SERVER_DEPLOYMENT 版本与目录树漂移 | ✅ 同步 v0.1.12 |

验证：fmt/clippy/53 tests 绿；本地带 key 真实上游冒烟（chat 200 "OK"）；CI/CD/release 全 success；公网 healthz version=0.1.12。
