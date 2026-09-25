# 阶段1说明（2026-09-26，只读准备）
- 本次仅创建 scripts/e2e_smoke.sh（可重复运行的 bash E2E 脚本），未运行任何请求。
- 语法已用 `bash -n scripts/e2e_smoke.sh` 验证通过（LF、无 BOM、shebang 正常）。
- 阶段2（推送部署完成后）：提供生产 API_KEY 后运行脚本并记录输出到 docs/audit/E2E-2026-09-26.md。

## 脚本用法

```bash
# 公网生产（默认 BASE_URL=https://try.hwhcie.bond）
API_KEY=<生产key> scripts/e2e_smoke.sh

# 自定义地址（如本地直连 47831）
API_KEY=<key> scripts/e2e_smoke.sh http://127.0.0.1:47831

# 未提供 API_KEY：仅跑 healthz/ui 探活，其余输出 SKIP（exit 0）
scripts/e2e_smoke.sh
```

## 覆盖清单

| 编号 | 检查 | 断言 |
|---|---|---|
| T1 | GET /healthz | 200 + ok:true + version 字段 |
| T2 | GET /ui | 200 + HTML 含面板关键词 |
| T3a | GET /v1/models 无 key | 401 |
| T3b | GET /v1/models 带 key | 200 + data 数组非空 |
| T4 | POST /v1/chat/completions 非流式 | 200 |
| T5 | POST /v1/chat/completions stream:true | 200 + Content-Type: text/event-stream |
| T6 | POST /v1/messages (Anthropic) | 200 |
| T7 | POST /v1/responses | 200 |
| T8a | GET /metrics 无 key | 401 |
| T8b | GET /metrics 带 key | 200 |
| T9 | 限流：连打 6 次 | 出现至少一次 429（阈值以生产 config 为准） |
| T10 | 熔断：已知不存在模型 | 4xx/5xx 而非 200（可 E2E_RUN_BREAKER=0 关闭） |

环境变量：`API_KEY`（必填才有鉴权类检查）、`E2E_SAVE_DIR`（保留请求响应正文）、`E2E_RUN_BREAKER`（默认 1）。

退出码：无 FAIL 时 0；有 FAIL 时 1。汇总行 `PASS=.. FAIL=.. SKIP=..` + `RESULT=PASS|FAIL`。
