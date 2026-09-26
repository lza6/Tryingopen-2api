# N3 Benchmark 压测基线（可重复运行脚本）

> 日期：2026-09-26
> 脚本：`scripts/bench/bench.ps1`
> 输出目录：`scripts/bench/results/results-YYYYMMDD.json`
> 目标：本地 Rust(axum) 网关 `http://127.0.0.1:47831`

## 1. 脚本用途与边界

`bench.ps1` 是一个可重复运行的 PowerShell 压测脚本，只做**安全并发探测**：

- 默认并发探测公开端点：`/healthz`、`/v1/models`、`/api/proxies`；
- 提供 `-ApiKey` 时额外探测需要鉴权的 `/metrics`；
- 默认**绝不调用** `/v1/chat/completions`，不消耗 tryingopen.com 上游配额；
- 只有显式开启 `-StressChat` 时才做 **3 次**非流式 chat 请求，并明确提示会消耗上游配额
  （上游每 24h UTC 日约 20 次硬限流，禁止用默认压测猛打 `/v1/chat`）。

实现约束：

- 原生 PowerShell（`Invoke-WebRequest` + `ForEach-Object -Parallel`），无第三方模块依赖；
- 幂等、可重复：每次运行覆盖当天 `results-YYYYMMDD.json`，不追加脏数据；
- 退出码：任一端点出现 FAIL（状态码不在 200-299）返回 1，否则返回 0；
- 每个请求 30s 超时（chat 分支 180s），失败样例只展示前 3 条避免刷屏。

## 2. 参数说明

| 参数 | 默认值 | 说明 |
|---|---|---|
| `-BaseUrl` | `http://127.0.0.1:47831` | 网关基础地址 |
| `-Concurrency` | `5` | 每端点并发数 |
| `-Requests` | `20` | 每端点请求总数 |
| `-ApiKey` | 空 | 提供则额外压测 `/metrics`，否则跳过鉴权端点 |
| `-StressChat` | 关 | 开启后只跑 3 次非流式 chat（消耗上游配额） |
| `-ChatModel` | `gpt-4o-mini` | `-StressChat` 时的模型名 |

## 3. 使用方法

```powershell
# 基础探测（推荐，不消耗上游配额）
.\scripts\bench\bench.ps1

# 指定并发/请求量
.\scripts\bench\bench.ps1 -Concurrency 10 -Requests 50

# 带 API key 探测 /metrics
.\scripts\bench\bench.ps1 -ApiKey "sk-xxx"

# 显式开启 chat 压测（消耗上游配额，仅 3 次）
.\scripts\bench\bench.ps1 -StressChat -ApiKey "sk-xxx"

# 查看帮助
.\scripts\bench\bench.ps1 -?
```

输出：

- 人类可读摘要（每端点完成数、成功/失败数、状态码分布、p50/p95/max 耗时 ms、吞吐 req/s）；
- 一行 JSON 基线（含 rust 版本、时间戳、各端点结果）写入
  `scripts/bench/results/results-YYYYMMDD.json`；
- 进程退出码：有 FAIL 为 1，全部成功为 0。

## 4. 本地实跑记录（2026-09-26 15:48-15:51 CST）

**前提**：本地网关 `127.0.0.1:47831` **未启动**（连接被拒绝）。按任务要求，
用 1-2 个轻量命令验证脚本能明确报错并记录该情况，未伪造 200。

### 4.1 语法/可运行性验证

```powershell
$tokens=$null; $errors=$null
[System.Management.Automation.Language.Parser]::ParseFile(
  (Resolve-Path scripts\bench\bench.ps1), [ref]$tokens, [ref]$errors) | Out-Null
$errors.Count   # 0 -> PARSE OK
.\scripts\bench\bench.ps1 -?   # 显示参数语法
```

结果：`PARSE OK`；`-?` 正常打印参数列表。

### 4.2 小规模实跑（-Concurrency 2 -Requests 3，无 ApiKey）

```
=== tryingopen-2api 压测 ===
BaseUrl: http://127.0.0.1:47831 | Concurrency: 2 | Requests/端点: 3
--- 端点 healthz ---
  完成: 3 | 成功: 0 | FAIL: 3 | 状态码: 0x3
  失败样例: status=0 err=由于目标计算机积极拒绝，无法连接。 (127.0.0.1:47831)
--- 端点 v1_models ---
  完成: 3 | 成功: 0 | FAIL: 3
--- 端点 api_proxies ---
  完成: 3 | 成功: 0 | FAIL: 3
=== 摘要 ===
总端点: 3 | 总体 FAIL: True
JSON 基线已写入: scripts/bench/results/results-20260926.json
退出码: 1（符合预期：FAIL -> 1）
```

### 4.3 ApiKey 分支验证（-ApiKey "test-key-for-branch-check"）

4 个端点均进入探测流程（healthz / v1_models / api_proxies / **metrics**），
全部因服务未启动明确报 `连接被拒绝`；退出码 1。结论：鉴权端点分支可正常触发。

### 4.4 JSON 基线核验

- 文件：`scripts/bench/results/results-20260926.json`
- 内容可被 `ConvertFrom-Json` 解析（有效 JSON）；
- 字段：`tool`、`timestamp`、`base_url`、`concurrency`、`requests`、
  `stress_chat`、`rust`（`rustc 1.95.0 (59807616e 2026-04-14)`）、`endpoints[]`。

> 说明：上述小规模跑因服务未启动全部 FAIL，属于**预期环境状态**，
> 不是脚本缺陷；脚本已用两种参数组合验证可运行、可报错、可写基线、退出码正确。
> 未启动网关时的 JSON 基线会反映 FAIL，重复运行会被后续真实数据覆盖。

## 5. 后续生产压测建议

1. **先启动网关**（`start.bat` 或 cargo 二进制监听 47831），再跑
   `.\scripts\bench\bench.ps1`；首次跑前建议单次 `curl /healthz` 确认端口已监听。
2. **配额红线**：上游 tryingopen.com 每 24h UTC 约 20 次硬限流。
   - 日常回归只用默认参数（不碰 `/v1/chat`）；
   - `-StressChat` 必须人工显式开启，且只做 3 次非流式，切勿放大到循环脚本；
   - 建议把 chat 压测安排在上游配额重置后，并配合真实 API key 观察 429。
3. **指标解读**：
   - 公开端点应在本地返回 200，p50 < 100ms（此前 N3 报告 healthz avg 48ms）；
   - 若 `/v1/models`、`/api/proxies` 偶发 5xx，优先检查代理池/上游预检逻辑，再谈并发参数；
   - 吞吐计算以“批次总耗时”为分母，低请求量下数值仅供参考。
4. **规模化压测**：需要真实压测时，从 `-Concurrency 5 -Requests 20` 起步，
   逐步升到 10/50、20/200，每档记录 JSON 基线；观察内存/日志/连接池，
   不要在同一天叠加多档 chat 测试（会烧配额）。
5. **CI 化建议**：把 `bench.ps1`（无 -StressChat）接入本地回归，退出码 1 即失败；
   `results-*.json` 可做趋势比对（p50/p95 环比）。
6. **已知边界**：脚本使用 `ForEach-Object -Parallel`，要求 PowerShell 7+；
   Windows PowerShell 5.1 不可用，需用 `pwsh` 运行。
