# DOCS-DRIFT 2026-09-26 — 文档漂移审计（v0.1.13 / main HEAD c42d1e6）

> 只读审计。未修改、未创建任何其他文件；唯一写集为本报告。审计基准：`git rev-parse HEAD` = c42d1e6c2315086f5ff2140f11621a3d87623078（Cargo.toml version 0.1.13）。
> 上游现场：2026-09-26 实测 www.tryingopen.com 首页 + 全部 13 个 `/ _next/static/chunks/*.js`（Python urllib / curl --ssl-no-revoke 抓取，暂存于 `%TEMP%\tk_*.js`）。

## 结论

- **README_en.md 与 README.md 关键信息不一致**（3/4 项漂移，且 README_en 完全过时：未提及 v0.1.13、测试数 57、12 静态模型+动态目录、/api/usage 端点；限流口径还写成了「per hour」，与全仓统一口径「每 24h UTC 日约 20 次」冲突）。
- **docs/DOCKER.md 与 Dockerfile/docker-compose.yml 大体一致**（阶段/镜像/ENTRYPOINT/VOLUME/config.json 挂载/环境变量表基本正确），但**存在 1 处硬漂移：文档称「compose 未内置 healthcheck」，而 docker-compose.yml 实际已内置 healthcheck**，且文档样例与 compose 实测写法（HTTP/1.0 + grep 200）不一致；另有 3 处措辞级旧残留（「需 C 编译器」「sqlite/telemetry 数据」「named volume 时 sqlite 文件名不变」）与 .dockerignore 的 `data/*.sqlite*` 历史残留互相呼应，均为残留表述、非行为差异。
- **config.example.json 与 src/config.rs Config 字段全集一致**：27 个字段全部对应，`rate_limit_max_keys`（默认 4096）与 `direct_fallback_quota`（默认 10）均已覆盖；新增字段 `max_concurrent_requests`（默认 64）与 `redact_logs`（默认 true）也已同步。无缺失字段。
- **部署三文档拓扑/端口/版本/凭据占位符**：NGINX_DEPLOY 为**当前真实生产拓扑**（v0.1.12 运行中，域名/证书路径真实，凭据已占位）；SERVER_DEPLOYMENT **当前无真实凭据残留**（全部 `<SERVER_IP>` / `sk-to-<YOUR_API_KEY>` / `<YOUR_UI_PASSWORD>` 占位符；真实 key 只存在于 git 历史提交 63226a8/296e391，工作区已脱敏——仅报告，不处理）；DEPLOYMENT_SOP 有 1 处**陈旧版本残留（SQLite 内嵌）**和 1 处文档错行（DOCKER.md 环境变量表）。
- **docs/PROTOCOL.md 与 src/upstream.rs 解析器不一致**：解析器仍按 09-24 快照支持 `supportsReasoning`/`supportsThinking`、`messageLimit`、`cheaperFallbackId`，**但 2026-09-26 实测上游 13 个 chunk 全部不含这三个字段**（messageLimit 仅剩 UI 展示文案，无任何模型记录携带该键）；当前模型目录为 **22 个**（非文档声称的 24 个），kimi-k3 上游 `pricePerMTok` 已从 15 变为 **8.5**（OpenRouter 侧 completion 8.5）。解析器对「已不存在字段」的兼容无害（flag/field 缺失即 false/None），但文档快照已过期，且**静态目录 kimi 价格 15 未随上游更新**。

## 一致项

1. **版本**：Cargo.toml `version = "0.1.13"`；HEAD 提交信息 `v0.1.13`；README.md 含 v0.1.13（README.md 第 9 行版本要点）；README_en 无版本号（自身未声明，不算冲突）。
2. **测试数 57**：`rg '#[test]|#[tokio::test]' tests src -g '*.rs'` = 57；README.md:189 写 `cargo test # 57 个测试`，docs/ARCHITECTURE.md:67 也写 57。一致。（README_en 未提及，属缺失项见下。）
3. **端口 47831**：README.md（默认监听 127.0.0.1:47831）、README_en（Default listen: http://127.0.0.1:47831）、docs/DOCKER.md（47831）、docker-compose.yml（`"47831:47831"`）、src/config.rs `default_listen()` = `"127.0.0.1:47831"` 全部一致。
4. **DOCKER.md 阶段/镜像/ENTRYPOINT/VOLUME/config 挂载**：builder `rust:1.85-bookworm` + build-essential/pkg-config、runtime `debian:bookworm-slim` + ca-certificates + appuser、`ENTRYPOINT ["/app/tryingopen2api"]` + `CMD ["--config","/app/config.json"]`、`VOLUME ["/app/data"]`、`EXPOSE 47831`、compose 只读挂载 `./config.json:/app/config.json:ro` 与 `./data:/app/data` —— 文档与 Dockerfile/docker-compose.yml 逐项相符。
5. **DOCKER.md 环境变量表**：文档所列变量全部在 src/config.rs 中存在对应 `env::var`（CONFIG_PATH/LISTEN_ADDR/UPSTREAM_BASE_URL/API_KEYS/UI_PASSWORD/RATE_LIMIT_*/RATE_LIMIT_MAX_KEYS/DIRECT_FALLBACK_QUOTA/CB_*/METRICS_ENABLED），默认值（4096/10/开/5/30/开）与源码 default 函数一致。
6. **config.example.json 字段**：`src/config.rs` `pub struct Config` 共 27 个 pub 字段（含 rate_limit_max_keys/direct_fallback_quota/max_concurrent_requests/redact_logs），config.example.json 恰好 27 个键，名称全部一致，默认值与源码 `default_*()` 一致（listen 127.0.0.1:47831、timeout 120、catalog 30、free_proxy 30、hourly 20、max_attempts 3、cooldown "0,15,60,120,300"、rate 60/3600、cb 5/30、max_keys 4096、quota 10）。
7. **NGINX_DEPLOY 拓扑/端口**：nginx → 127.0.0.1:47831，域名 try.hwhcie.bond、IP 20.204.27.154、共用 imagefree 证书路径、`proxy_pass http://127.0.0.1:47831`、Upgrade/Connection 头、`proxy_buffering off`——文档自洽；README.md 生产拓扑段落（v0.1.12 systemd）与其一致。
8. **SERVER_DEPLOYMENT 无真实凭据残留**：工作区所有占位符均为 `<SERVER_IP>` / `sk-to-<YOUR_API_KEY>` / `<YOUR_UI_PASSWORD>`（行 10/11/25/28/29/35/39/40/41/45/84/107）；`rg` 全 docs 未命中 `gvee`/真实 sk- 串。真实密钥仅存在于 git 历史（.git/logs/HEAD 可见 296e391「服务器部署记录与鉴权凭据」、63226a8「凭据脱敏」），当前文件已脱敏。
9. **PROTOCOL.md 事件/消息/effort 结构**：start/start-step/reasoning-*/text-*/finish-step/finish/error/[DONE]、消息 parts、无 system 角色、无原生 tool_calls、`effort` balanced/deep——与 src/upstream.rs（`StreamRequest{id,trigger,messageId,model,effort,messages,stream}`）及 README.md 五节一致。
10. **sessions 5000 上限**：DEPLOYMENT_SOP「session map 已限 5000」与 src/session.rs:39 `if m.len() > 5000` 一致。

## 不一致项（文件+证据）

### A. README_en.md vs README.md（3 处实质漂移）

| # | 项 | README_en.md | README.md / 源码 | 判定 |
|---|---|---|---|---|
| A1 | 限流口径 | 第 5 行 "rate-limits **~20 requests per hour** per IP" | 全仓口径「每 IP **每日/24h UTC 日**约 20 次」（README.md:7/36/119/122，docs/PROTOCOL.md:10/68，src/config.rs 注释 `hourly_per_ip` = 每 24h UTC 日限流） | ❌ per hour 与 per day 冲突（若按字面执行会导致代理池语义误解） |
| A2 | 功能描述 | 仅"OpenAI/Anthropic 兼容、单二进制、代理池故障轮换" | v0.1.13 新增：模型能力字段透传（思考/消息数上限/降级建议）、429 按上游建议降级、每 key 用量统计 /api/usage、结构化日志、config.local.json 局部覆盖；代理池 44 源/4500 上限、工具调用、effort 透传、多模态、内置 12 静态+动态目录、/ui、/api/proxies 等 | ❌ README_en 完全缺 v0.1.13 能力面与端点清单 |
| A3 | 测试数 57 | 无任何测试信息 | README.md:189「57 个测试」；实测 57 | ❌ 缺 |
| A4 | 版本号 | 无版本声明 | README.md:9「v0.1.13」；Cargo.toml 0.1.13 | ⚠️ 缺（en 未声明版本，读者无法对齐 release） |

（端口 47831 双方一致，见一致项 3。）

### B. docs/DOCKER.md vs Dockerfile/docker-compose.yml

| # | 项 | 文档 | 实际 | 判定 |
|---|---|---|---|---|
| B1 | healthcheck | DOCKER.md:128「**compose 未内置 healthcheck**…建议按需启用」，样例为 `CMD bash -c …grep '"ok":true'` | docker-compose.yml 已内置 healthcheck：`CMD-SHELL bash -c '…printf "GET /healthz HTTP/1.0…" >&3 && head -1 <&3 | grep -q 200'` | ❌ 硬漂移（文档说没有、实际已有；且两处探测写法不同） |
| B2 | 编译器残留 | DOCKER.md:18「纯 Rust 依赖，无需 C 编译器（已移除 rusqlite）」 | 同句自相矛盾：builder 仍装 build-essential/pkg-config（Dockerfile 实际装）；Cargo.toml 无 rusqlite | ⚠️ 措辞残留（“已移除 rusqlite”为历史叙事，非当前行为差异） |
| B3 | sqlite 残留声称 | DOCKER.md:23/34「无 sqlite/telemetry 库」「无 sqlite/telemetry 数据」；:108「named volume 时 sqlite 等文件名不变」 | 源码无 rusqlite/sqlite/telemetry；但 `.dockerignore:10-12` 仍保留 `data/*.sqlite/.sqlite-shm/.sqlite-wal` 排除规则 | ⚠️ 文档方向正确但语句过时，且与 .dockerignore 的 sqlite 排除互为“旧物呼应” |
| B4 | compose 说明 | DOCKER.md:76「等价于…映射 47831…挂载 config 与 data…restart: unless-stopped」 | compose 实际还含 healthcheck + `deploy.resources.limits.memory: 512M` | ⚠️ 文档未提 memory limit（次要） |

### C. config.example.json vs src/config.rs

- ✅ **无缺失**：`rate_limit_max_keys`、`direct_fallback_quota` 均存在（src/config.rs:116/119，默认 4096/10；config.example.json 末两行）。
- 唯一差异（非缺失）：config.example.json 未列出 `max_concurrent_requests`（默认 64）与 `redact_logs`（默认 true）——这两个字段**同样存在于 Config**（config.rs:76/88），示例文件选择省略；README.md 配置表也未列。属“示例精简”而非字段缺失（P2 建议补齐）。注意：config.example.json 中 `listen_addr` 为 `0.0.0.0:47831`（容器/公网导向），而源码 default 为 `127.0.0.1:47831`——示例值≠默认值，但语义明确（README/DOCKER 均要求公网改 0.0.0.0），不算漂移。

### D. NGINX_DEPLOY / SERVER_DEPLOYMENT / DEPLOYMENT_SOP

| # | 项 | 证据 | 判定 |
|---|---|---|---|
| D1 | NGINX_DEPLOY 版本 | NGINX_DEPLOY.md:11「tryingopen2api, **v0.1.10**」 | 当前 HEAD v0.1.13；README.md:234 生产段同样写 **v0.1.12**。生产实际版本未知（无法远程核验，只读审计）；但文档两处版本（0.1.10/0.1.12）与 HEAD 0.1.13 不一致 | ⚠️ 版本向量漂移（需生产核对后同步） |
| D2 | SERVER_DEPLOYMENT 版本 | SERVER_DEPLOYMENT.md:77「v0.1.3」验收记录 | 与 HEAD 0.1.13 不一致（历史验收记录，日期 2026-09-25） | ⚠️ 陈旧验收段，建议标注“当时版本”或更新 |
| D3 | 凭据状态 | 全部占位符；无真实凭据（见一致项 8） | 真实 key 在 git 历史 63226a8/296e391（.git/logs/HEAD:32,47） | ✅ 当前文件无残留；仅报告历史事实 |
| D4 | DEPLOYMENT_SOP sqlite | DEPLOYMENT_SOP.md:5「无外部依赖（单二进制；**SQLite 内嵌**）」 | 源码无 rusqlite/sqlite 依赖（Cargo.toml），会话纯内存 | ❌ 陈旧残留 |
| D5 | DEPLOYMENT_SOP 环境变量 | 第 3 节「LISTEN_ADDR / … 等」 | 实际 env 全集还含 RATE_LIMIT_*、CB_*、METRICS_ENABLED、RATE_LIMIT_MAX_KEYS、DIRECT_FALLBACK_QUOTA、MAX_CONCURRENT_REQUESTS（config.rs:248-323） | ⚠️ “等”省略可接受；DOCKER.md 表格覆盖全（但见 B5 错行） |
| D6 | DOCKER.md 表格错行 | 第 122 行 `DIRECT_FALLBACK_QUOTA … 10 | 每 Key 限流 | 开 / 60 / 3600` | 单元格错位：`开/60/3600` 应为 RATE_LIMIT_* 的说明，被挤到 DIRECT_FALLBACK_QUOTA 行尾 | ❌ 表格排版错误（信息仍可读） |

### E. docs/PROTOCOL.md vs src/upstream.rs + 今日上游（2026-09-26 实测）

| # | 项 | 证据 | 判定 |
|---|---|---|---|
| E1 | 模型数 | PROTOCOL.md:69「2026-09-24 实时抓到 **24 个**」 | 今日实测 `tk_07-yippzga9hm.js`（`e.s(["AVAILABLE_OPEN_MODELS",…`）解析出 **22 个唯一模型** | ❌ 24 → 22（上游已变化） |
| E2 | messageLimit | PROTOCOL.md:68「可选字段 messageLimit（如 kimi-k3=5）」 | 今日 13 个 chunk **全部不含 `messageLimit:` 键**（tk_07 中 0 次；tk_0dab0 中 9 次均为 UI 展示文案 `i?.messageLimit`，无任何模型记录携带该字段） | ❌ 上游已移除该字段（09-24 快照已过期） |
| E3 | cheaperFallbackId | PROTOCOL.md:68「cheaperFallbackId（如 kimi-k3 → minimax/minimax-m3）」 | 今日全部 chunk **不含 `cheaperFallbackId`**（tk_07 0 次、tk_0ij 0 次） | ❌ 上游已移除该字段 |
| E4 | supportsReasoning | PROTOCOL.md 无显式字段行，但 src/upstream.rs:236 `reasoning: flag("supportsReasoning") \|\| flag("supportsThinking")` | 今日 13 个 chunk **全部不含 supportsReasoning/supportsThinking**（扫描 0 命中） | ❌ 解析器所依赖的上游字段已不存在（缺失→false，行为静默降级） |
| E5 | kimi-k3 价格 | PROTOCOL.md 未写价格；但 README.md:107 静态表 `kimi-k3 15`；src/models.rs:170 静态 `price_per_mtok: 15.0` | 今日上游 `{id:"moonshotai/kimi-k3",…pricePerMTok:8.5,maxOutputTokens:16e3}`；OpenRouter 供应商表 `completionPerMTok:8.5`（tk_0ijbeve-tu-gf.js） | ❌ 静态目录 15 vs 上游 8.5（动态抓取时会覆盖为 8.5；仅静态兜底场景价格过时） |
| E6 | 解析器 vs 快照 | src/upstream.rs:202-203 `re_ml = messageLimit:(\d+)`、`re_cf = cheaperFallbackId:"…"`、:236 双 flag；README.md 五节同样写 `{…messageLimit,cheaperFallbackId}` | 今日上游无这三字段 → 正则永不命中 | ⚠️ 解析器兼容性无害但成为死代码；README/PROTOCOL 字段表需更新 |
| E7 | 新模型清单 | PROTOCOL.md:69「新增 … anthropic/claude-sonnet-5、openai/gpt-5.6-terra 等」 | 今日 `tk_0dab0_rvm4uuk.js` 的 `yB=[{id:"anthropic/claude-sonnet-5",…},{id:"openai/gpt-5.6-terra",…}]` 存在，但**不在 22 个 `AVAILABLE_OPEN_MODELS` 内**（仅为 UI 抽屉里两个无价格/无能力字段的“coming”记录） | ⚠️ “新增”表述误导：claude-sonnet-5/gpt-5.6-terra 不是可对话模型记录 |
| E8 | 快照日期 | PROTOCOL.md:4「更新：2026-09-24」 | 2026-09-26 实测上游已变化（22 模型、kimi 8.5、无三字段） | ❌ 快照过期 2 天 |

## 建议动作

### P0（立即/正确性）

1. **同步部署文档版本向量**：NGINX_DEPLOY.md:11「v0.1.10」与 README.md:234「v0.1.12」需在生产侧核验实际部署版本（仅 2026-09-26 远程核对，无权远程执行；本地 HEAD=v0.1.13），随后统一为实际版本号。
2. **README_en.md 重写或标注过时**：限流口径改「~20 per day per IP (24h UTC window)」；补 v0.1.13 能力段（能力透传/cheaperFallback/用量统计/结构化日志/config.local）；补测试 57 与 /api/usage、/ui、/api/proxies 端点；或加「outdated, see README.md」横幅。
3. **PROTOCOL.md 更新为 2026-09-26 实测**：模型数 24→22、删除 messageLimit/cheaperFallbackId「可选字段」行或改注「09-24 曾出现、今日已移除」、kimi 价格注明 8.5、claude-sonnet-5/gpt-5.6-terra 标注为「UI coming 记录、非可对话模型」、快照日期改为 2026-09-26。

### P1（行为/一致性）

4. **静态目录 kimi-k3 价格 15.0 → 8.5**（src/models.rs:170）：静态兜底场景当前会向客户端报告过时价格（/v1/models 输出 price_per_mtok）。动态抓取时被覆盖，但静态 fallback 场景需修正；同步 README.md:107 静态表。
5. **upstream.rs 字段解析器与上游现状对齐**：删除或降级 `messageLimit/cheaperFallbackId/supportsReasoning/supportsThinking` 正则（死代码，E6）；保留能力字段透传结构（ModelMeta.message_limit/cheaper_fallback 可为 None），上游恢复字段时再启用。
6. **DOCKER.md B1 硬漂移**：删除「compose 未内置 healthcheck…建议按需启用」段落，改为「compose 已内置 healthcheck（bash /dev/tcp + HTTP/1.0 grep 200）」并同步文档样例与 compose 实际写法；顺手补 compose memory 512M 说明（B4）。
7. **DEPLOYMENT_SOP.md:5 删除「SQLite 内嵌」**，改「无外部依赖；会话内存态（上限 5000）」；DOCKER.md:122 表格错行修复（D6）。

### P2（清理/可选）

8. **config.example.json 补齐 `max_concurrent_requests`(64) 与 `redact_logs`(true)**（当前 27 字段为 25+2 精简版；README 配置表同步）。或显式注释“示例省略非核心字段”。
9. **DOCKER.md/.dockerignore sqlite 残留**：删除 .dockerignore 的 `data/*.sqlite*` 三行（无 sqlite 运行时）与 DOCKER.md:108「sqlite 等文件名不变」措辞；DOCKER.md「已移除 rusqlite」改为「无数据库依赖」。
10. **SERVER_DEPLOYMENT.md 历史版本标注**：验收段 v0.1.3 加「（2026-09-25 当时版本）」脚注；git 历史 63226a8/296e391 的真实密钥仍建议按既定清理计划处理（不在本报告处理范围）。

## 附：审计方法

- 全部只读命令：`git rev-parse HEAD`、`git status --porcelain`、`Get-Content`、`Select-String`、`rg`、`git show HEAD:<file>`。
- 上游现场抓取（2026-09-26）：首页 `curl --ssl-no-revoke`（HTTP 200, 35314B）→ 13 个 chunk 全量 `python urllib` 下载至 `%TEMP%\tk_*.js`；模型目录解析 `tk_07-yippzga9hm.js`（`AVAILABLE_OPEN_MODELS` = 22 条）；字段扫描（supportsReasoning/supportsThinking/messageLimit/cheaperFallbackId = 0 命中）；kimi 供应商表 `tk_0ijbeve-tu-gf.js`（completionPerMTok 8.5）。
- 测试数：`rg '#[test]|#[tokio::test]' tests src -g '*.rs'` = 57（tests/models_test 13 + tests/proxy_test 8 + src 36）。
- 基线说明：审计开始时工作区即存在 `Cargo.toml`/`src/upstream.rs` 未提交修改（read_timeout 统一、lto=fat/codegen-units=1/panic=abort，与文档审计无关）；未做任何 git 操作；未修改任何审计范围文件。
