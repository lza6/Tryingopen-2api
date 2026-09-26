# TryingOpen2API 发布说明

> 当前版本：v0.1.15（面板 UX + CI Actions v5 + 工程卫生）· 仓库 main 分支
> 完整变更历史见 [CHANGELOG.md](CHANGELOG.md)；详细协议/部署见 docs/INDEX.md。

## v0.1.15（2026-09-27）

- feat: 面板"复制 curl/Python"按钮 + 健康自检（真实探测，auth:false）
- fix: bench 脚本 proxies Auth 标志
- ci: Actions v5（checkout/upload-artifact）
- chore: config.json 移出 git 跟踪；config.example 补齐
- 验收：本地 E2E（面板渲染/usage 计数/JSON 日志）+ 生产 healthz；57 tests

## v0.1.14（2026-09-26）


- fix: 代理 read_timeout 用配置值（不再硬编码 120s）
- feat: kimi 价格 8.5（上游实测）；release panic=abort（产物 -30%）
- docs: 全仓漂移修复 + 审计报告 + release 样本归档
- 验收：本地真实上游 E2E 200；生产 healthz 200；57 tests

## v0.1.13（2026-09-26）


- feat: 模型能力字段透传（reasoning / messageLimit / cheaperFallbackId）+ 429 按上游建议模型降级
- feat: GET /api/usage 每 key 用量统计（内存有界）+ 面板用量摘要/思考徽章/降级 chip
- feat: 请求日志结构化 JSON 行（key 脱敏）
- feat: config.local.json 局部覆盖合并
- ci: release job 去重；test: 53 → 57 全绿
- 验收：本地真实上游 E2E（chat/anthropic/responses 三协议 200 + usage 统计 + 401 鉴权 + JSON 日志）

## v0.1.11（2026-09-26）

- 安全：日志脱敏按 char（修多字节 key panic DoS）；`clear` 需二次确认；`/metrics` 加鉴权；直连兜底配额；限流 map 上限
- 协议：Anthropic error 后 message_stop；start_event 携带 model；图片去重；Anthropic 长度截断；非流式 error → 502
- 前端：面板重写（API Key UI / fetch 超时 / 指南实时 / 表格兜底 / XSS 转义）
- 发布：CD 原子替换+回滚；compose healthcheck；config.local.json 覆盖通道
- 验收：本地完整带 key E2E（真实上游 11 PASS + 2 EXPECTED）；Release 资产 SHA256 三方一致；公网 version=0.1.11

校验：`Get-FileHash tryingopen2api.exe -Algorithm SHA256`
