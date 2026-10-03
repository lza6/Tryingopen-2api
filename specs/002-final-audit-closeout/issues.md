# 终局审计问题清单（4 代理汇总 + 主控验证）

> 生成：2026-09-27 · 来源：a22daaf（code-review）、a88e5b92（前后端衔接）、a07db6a5（调用方契约）、ad4fe755（边界并发）、a36b6344（文档盲点）+ 主控浏览器/门禁实测
> 状态标记：🔄 修复中 / ✅ 已修 / ⏳ 待修 / ⚠️ 外部受限

## P0（阻塞级）

| ID | 问题 | 文件 | 状态 |
|---|---|---|---|
| C1 | 熔断 HalfOpen 探测请求非 5xx（4xx/429）→ 既不 success 也不 failure → 永久卡死 503，只能重启 | prod_guard.rs + api.rs 1108/1873 | ✅ 主控已修（超时自动回 Open）+ 2 回归测试 |
| C2 | Dockerfile `rust:1.85` 无法编译 `is_multiple_of`（需 ≥1.87）→ docker build 必失败 | Dockerfile:2 + prod_guard.rs:59 | 🔄 代理 B |
| C3 | **发布物漂移**：release 二进制仍是 v0.1.16（上轮安全修复后未重建） | target/release | ✅ 主控已重建 v0.1.17 |

## P1（高优先级）

| ID | 问题 | 文件 | 状态 |
|---|---|---|---|
| H1 | models.rs 锁序死锁（all/normalize: forced_offline→inner；replace: inner→forced_offline，AB-BA） | models.rs | 🔄 代理 A |
| H2 | mark_offline 单向永久下线（unmark_offline 零调用；上游临时 400 永久隐藏模型） | models.rs + api.rs 1104/1871/2087 | 🔄 代理 A |
| H3 | config.example.json 默认 0.0.0.0 + api_keys 空 = 照示例部署公网匿名开放 | config.example.json | 🔄 代理 B |
| H4 | 匿名模式面板会话 key 无界累积（每次打开 push 一个，永不清理） | api.rs handle_dashboard | 🔄 代理 C |
| H5 | 代理表「容量剩余」列恒显 `-`（行级无 capacity_remaining 字段，假列） | web.rs + proxy_pool.rs | 🔄 代理 C |
| H6 | 面板会话 key 自举把服务从「空=放行」翻转为「需鉴权」，与 README 文档化行为冲突（调用方按文档用 sk-local 全 401） | api.rs handle_dashboard + README | 🔄 代理 C + 主控文档 |

## P2（中优先级）

| ID | 问题 | 文件 | 状态 |
|---|---|---|---|
| M1 | sticky 表头被 sticky header 遮挡（both top:0，header z 更高） | web.rs | 🔄 代理 C |
| M2 | 44px 触摸目标未覆盖 nav 按钮 / primary | web.rs | 🔄 代理 C |
| M3 | 代理表加载占位 colspan=6 ≠ 8 列 | web.rs | 🔄 代理 C |
| M4 | 接入指南 key 占位文案与实际状态不符 | web.rs | 🔄 代理 C |
| M5 | favicon 404（浏览器真实路径） | api.rs | ✅ 主控已修（内联 SVG 路由） |
| M6 | base_url 依赖 listen_addr，0.0.0.0 时复制命令不可用 | web.rs/api.rs | 🔄 代理 C 评估 |
| M7 | 对话截断阈值文档 12000 vs 代码 16000 | docs | ⏳ 主控文档 |
| M8 | 免费代理源数 44 vs 43 | docs | ⏳ 主控文档 |

## P3（增强）

| ID | 问题 | 状态 |
|---|---|---|
| L1 | 无 API 层契约测试（端点级） | 建议补 |
| L2 | /metrics 无鉴权时请求数统计含 /ui 面板流量 | 记录 |
| L3 | 生产 /ui 401（有 ui_password，属预期） | 已确认 |
| L4 | README_en 与 README 漂移 | 主控文档 |

## 主控真实验证补充（浏览器/门禁）
- 真实浏览器 UI 验收 v2：6/6 PASS（含 favicon 修复后无 404）
- 真实浏览器交互验收 v3：8/8 PASS（健康自检/生成 Key/移动端）
- 门禁：fmt/clippy/65 tests（C1 修复后）

## 外部受限（诚实披露）
- 真实上游 200 对话：当日配额耗尽（每 24h 约 20 次），以可操作 502 等价验证，待次日复验
- Docker 真构建：本机无 docker，以「源码在当前工具链可编译 + Dockerfile 版本修正」间接验证
