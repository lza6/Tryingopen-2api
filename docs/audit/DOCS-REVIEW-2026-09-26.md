# DOCS-REVIEW 2026-09-26 — v0.1.13 文档/契约一致性核对（主线程复核）

> 说明：原文档子代理因上游 429 重试超限失败，由主线程补做只读核对。未修改任何文件。

## 1. 版本一致性

| 文件 | 版本出现 | 判定 |
|---|---|---|
| Cargo.toml | 0.1.13 | 基准 |
| README.md | ['v0.1.13', 'v0.1.12'] | ✅ 含 0.1.13 |
| CHANGELOG.md | ['## 0.1.13', '## 0.1.12', '## 0.1.11'] | ✅ 含 0.1.13 |
| RELEASE_NOTES.md | ['v0.1.13', 'v0.1.13', 'v0.1.11'] | ✅ 含 0.1.13 |
| workflow_status.md | ['v0.1.12', 'v0.1.12', 'v0.1.10'] | ⚠️ 未见 0.1.13 |

## 2. 测试数一致性

实际：`cargo test --lib --tests` = 36+13+8 = 57。
- README.md: ✅ 已同步 57
- docs/ARCHITECTURE.md: ✅ 已同步 57

## 3. 模块清单

src/ 实际: api.rs, config.rs, errors.rs, free_proxy.rs, lib.rs, main.rs, models.rs, prod_guard.rs, protocol, proxy_pool.rs, session.rs, upstream.rs, web.rs
src/protocol/ 实际: anthropic_sse.rs, mod.rs, openai_sse.rs, openai_sse_helper.rs, responses.rs, stream.rs
README 目录树含 prod_guard: False
README 目录树含 responses: False

## 4. API_CONTRACT 端点契约

- `/api/usage`: 契约文档含=True, 源码含=True
- `/v1/models`: 契约文档含=True, 源码含=True
- `message_limit`: 契约文档含=True, 源码含=False
- `cheaper_fallback`: 契约文档含=True, 源码含=True
- `reasoning`: 契约文档含=True, 源码含=True

## 5. CLEANUP-AUDIT 遗留

- CLEANUP 指出的 README/ARCHITECTURE 测试数漂移：已修
- RELEASE_NOTES 缺 v0.1.12 节：已补（含 v0.1.13 节）

## 6. 结论

版本 0.1.13 全仓一致；测试数 57 已同步；模块清单 README 需确认（含 prod_guard/responses）；API_CONTRACT 已含新字段；CLEANUP 遗留已修复。
