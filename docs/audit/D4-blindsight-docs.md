# D4 盲点扫描/文档资产审计报告

> 审计角色：只读审计子代理 D（盲点扫描/需求对照/文档资产）
> 日期：2026-09-26　证据基准：HEAD=10f0247（`git log -1`）；公网 healthz 实测 version=0.1.10, proxies=6074, models=24
> 只读约束：未修改任何源文件；仅整理本报告与证据核对

## 结论摘要
- BLOCKER：1（见 D1）
- MAJOR：8
- MINOR：8
- 正向：git 全历史与工作区未发现真实凭据；CI 已含 gitleaks；依赖 0 已知 CVE（B 报告交叉验证）

## BLOCKER

### D1. log_request 对多字节 API key 做字节切片 → 远程可触发 panic（DoS）
- 位置：`src/api.rs:217-224`
- 问题：`k.len() > 8` 时 `&k[..4]` 与 `&k[k.len()-4..]` 按字节切；多字节 UTF-8 key（如 `密码密码密码`，9 字节）会切进字符中间 → panic。攻击者无需有效 key，带合法 UTF-8 长 Bearer 头触发 401 路径 → log_request panic → 请求 500（可重复 DoS）。
- 修复：改用 `k.chars().take(4)` / 尾 4 字符，或以字节安全函数切（`floor_char_boundary`）。

## MAJOR

### D2. 「每日/每小时」限流语义混乱且与代码不符
- 证据：README.md:7/36/42/121/170 用「每日 20 次」；docs/PROTOCOL.md:10/73/76、docs/DEPLOYMENT_SOP.md:58、docs/adr/0001:12 用「每小时 20 次」；workflow_status 容量写 90000/h；代码 `proxy_pool.rs:87-93` 实际按 UTC 日（24h）重置。
- 问题：`hourly_per_ip=20` 实际含义是「每 24h UTC 日」，文档两种口径并存；「90000/h」容量是「天」不是「小时」。
- 修复：统一为「每日(UTC 24h)」并在 API_CONTRACT/README 明示；`hourly_per_ip` 命名可保留但注释与文档对齐。

### D3. sqlite/telemetry 是空壳：配置/依赖/文档声称 vs 代码零使用
- 证据：`src/config.rs` 有 sqlite_path/telemetry_path；Cargo.toml 依赖 rusqlite（bundled）；DOCKER.md:23/34/105-108/148/181 声称「生成 sqlite/telemetry、会话持久化、升级前备份 sqlite」——但全仓（除 config.rs）无任何 sqlite/telemetry 读写，会话是纯内存（session.rs）。
- 问题：文档误导部署者备份不存在的文件；bundled rusqlite 让 Windows/容器构建需要 C 编译器且拖慢镜像。
- 修复：a) 文档删除 sqlite/telemetry 声称，改为「会话为内存态，重启即清」；b) 移除 rusqlite 依赖（或如实标注预留）；c) 若保留路径字段则标注「预留」。

### D4. 版本/变更记录漂移
- 证据：Cargo.toml=0.1.10；API_CONTRACT.md:86 / DEPLOYMENT_SOP.md:48/81 硬编码 0.1.1；DOCKER.md=0.1.7；workflow_status 头 0.1.6+；CHANGELOG 止于 0.1.2；RELEASE_NOTES 止于 v0.1.0；git tags 止于 v0.1.2；release.yml 与 ci.yml release job 双份构建。
- 问题：调用方/部署者按文档抄 0.1.1 却实际 0.1.10；上线记录与发布资产脱节。
- 修复：文档统一用「运行时 version 为准」+ 自动从 Cargo.toml 生成；CHANGELOG 补齐 0.1.3-0.1.10；git tag 补打（v0.1.10）；release.yml 与 ci release job 去重。

### D5. docs/INDEX.md 缺文档
- 证据：INDEX.md 未收录 DOCKER.md、NGINX_DEPLOY.md、SERVER_DEPLOYMENT.md、RELEASE_NOTES.md、本审计目录新报告。
- 修复：补全索引并新增 docs/audit/README.md（审计台账）。

### D6. CD deploy 无回滚 + cancel-in-progress 可把服务停在中途
- 证据：`.github/workflows/cd-deploy.yml:24-26`（concurrency cancel-in-progress: true）、`deploy.sh` `systemctl stop` → `cp -f` → `start`（停到起之间若取消/失败 → 服务停在停止态；旧二进制已被覆盖无回滚）。
- 修复：cancel-in-progress:false；deploy 改为「下载新 bin → 校验 → 原子替换 + restart」并保留上一版 bin + 失败自动回滚旧版。

### D7. 上游目录/协议解析脆弱 + 字段丢弃
- 证据：`src/upstream.rs:156/195` 用 regex 抓 JS chunk；PROTOCOL.md 记录的 messageLimit/cheaperFallbackId 未被 models 使用；webSearch 未实现。
- 问题：上游改版会导致目录静默失败（日志 warn）；messageLimit 语义（模型会话上限）客户端拿不到 → 长会话 413 体验差。
- 修复：目录解析失败提升告警；把 messageLimit 透传到 /v1/models meta 与 /api/guide（诚实暴露能力边界）。

### D8. 免费代理池可跨轮累积超 4500
- 证据：公网实测 proxies=6074（>4500）；代码按轮截断注入，但池逐轮累积，仅 3h 未用才 reap。
- 问题：文档「4500 截断」不成立；池越大快照越重、越限/冷却耗时越长。
- 修复：文档改为「单轮注入上限 4500、池总量随活性清退」；或增加池总量硬上限。

## MINOR
1. `config.json` 被 git 跟踪（含空 keys；有未来泄密风险）→ 建议移出 git 仅留 example + env。
2. `proxies_path` 与 `proxy_file` 双字段冗余（`proxies_path` 无使用者）→ 移除或合并。
3. `precheck_concurrency` 配置解析了但 `check_all_concurrent` 用常量 → 死配置。
4. 若 api_keys 空且公网暴露 → 无限流/无鉴权（文档明示风险）。
5. Docker 无 healthcheck（docs 有片段但 compose 无）→ 补 compose healthcheck。
6. 代理 URL 只接受字面 IP；住宅域名:port 被拒（SSRF 权衡，文档明示）。
7. release pipeline 重复（ci release job + release.yml）。
8. N1/N3/N5 审计文件未随修复更新（N1 仍写「非流工具调用未转 tool_calls」等旧结论）→ 在新审计台账里标注「已修」。

## 防重复机制建议
- 新建 docs/audit/README.md：审计台账（check ID/范围/状态/证据文件/最后运行）→ 下次改动先查台账，命中已验范围不重复跑。
- workflow_status.md 收敛为单一表格（N/P/F/S/R/D 系列），删重复「验证记录」块。
- CI 增加 smoke（healthz/限流/熔断）写入台账，避免人工重跑。

## 正向验证记录（供下次避免重复）
- 依赖：cargo audit 0 已知 CVE（本地 advisory-db 1269 条）。
- 密钥：全历史无真实凭据；gitleaks 在 CI。
- 门禁：fmt/clippy/42 tests 绿；doctest 因本机 rustdoc 缺失不可跑（CI 可跑）。
- 公网：healthz version=0.1.10 / proxies=6074 / models=24（2026-09-26 采集，随时间漂移）。
- 上游协议：匿名无 Cookie；429 语义「每 IP 每日 20 次」（实际代码按 UTC 日重置，见 D2）。
