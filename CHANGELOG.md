## 0.1.2 (2026-09-24) — 终局闭环审计修复

- 修复免费代理后台周期刷新失效（watch channel → 无限循环）
- 修复 truncate UTF-8 中文边界 panic（semaphore permit 泄漏风险）
- 修复代理池 permits 单槽覆盖（并发同代理 permit 提前释放）
- 修复 acquire 全冷却仍返回冷却代理（尊重每 IP 20/h 语义）
- Anthropic 流补 message_start + stop_reason 映射（tool_calls→tool_use）
- 修复 Anthropic 非流 usage 键名（input_tokens/output_tokens）
- session map 无界增长防护（>5000 清理）
- 工具模式正文前缀保留（preamble）
- SSRF 纵深防护：住宅/免费代理统一公网地址校验
- 请求体限制 16MB（DefaultBodyLimit）
- 新增 docs：API_CONTRACT / DEPLOYMENT_SOP / 审计报告 / ADR / 产品头脑风暴
- CI/CD 全绿验证（fmt/clippy/test×2/security/release）## 0.1.1 (2026-09-24)

- 代理池拉满：免费代理源 13 → 44（全部真实可达验证），默认开启；实测 4500+ 真实代理进池
- 并发预检 + 低延迟优先：每个代理真实 HTTP 延迟测量，acquire 按 inflight=0 → latency 升序 → health 降序
- 全局并发门控（Semaphore，max_concurrent_requests 默认 64）+ 每出口 inflight 软上限
- 容量实时计算：capacity_total/used/remaining（可用代理数 × 每 IP 每小时 20 次 − 已用）
- host:port 去重（O(n) HashSet）、失效降权、免费代理 3h+30min 保留策略
- 工具调用完整转换：OpenAI delta.tool_calls 增量帧 / Anthropic tool_use block（流式实测）
- 思考内容解析：非流式 reasoning_content / thinking block + usage（input/output/reasoning tokens）
- effort 透传：请求体 effort 字段（balanced/deep/low）直通上游
- 多模态修复：mediaType 驼峰字段，1x1 PNG 实测被 qwen 识别
- 模型下线自动降级：上游 model-not-found 自动标记 offline + fallback 链
- UI：剩余可用次数实时显示、代理延迟列、effort 选择器、15s 自动刷新# Changelog

## 0.1.0 (2026-09-24)

- 从 imagefree-2ai 抽出 tryingopen 提供商 + 代理池，按 tokenharbor-2api 架构重写为独立 Rust 网关
- 上游协议重写：POST /api/open（匿名 SSE，reasoning/text 增量）+ 首页 JS chunk 动态模型目录
- 代理池：住宅文件 + 免费代理抓取（13 源）双源，429 冷却/轮换/健康分/粘滞/直连兜底
- OpenAI / Anthropic 双协议桥接 + 内置控制面板
- 抓包与站点 JS 已搬运至 `源代码、网络数据包/`
- 真实 E2E 验证：匿名对话、流式 SSE、Anthropic 协议、坏代理故障轮换、免费代理真实抓取（52 个）


