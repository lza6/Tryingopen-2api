# Changelog

## 0.1.0 (2026-09-24)

- 从 imagefree-2ai 抽出 tryingopen 提供商 + 代理池，按 tokenharbor-2api 架构重写为独立 Rust 网关
- 上游协议重写：POST /api/open（匿名 SSE，reasoning/text 增量）+ 首页 JS chunk 动态模型目录
- 代理池：住宅文件 + 免费代理抓取（13 源）双源，429 冷却/轮换/健康分/粘滞/直连兜底
- OpenAI / Anthropic 双协议桥接 + 内置控制面板
- 抓包与站点 JS 已搬运至 `源代码、网络数据包/`
- 真实 E2E 验证：匿名对话、流式 SSE、Anthropic 协议、坏代理故障轮换、免费代理真实抓取（52 个）
