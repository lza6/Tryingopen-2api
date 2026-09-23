# TryingOpen2API v0.1.0

Rust (axum) 版 TryingOpen 免费模型 OpenAI / Anthropic 兼容本地 API 网关。单二进制、零外部依赖，可对接 Claude Code、Codex、Cursor、LobeChat、NextChat 等任意 OpenAI/Claude 客户端。

## 主要特性
- **匿名网关**：`POST /api/open` 匿名 SSE（reasoning/text 增量），无需 Cookie / 登录 / API Key
- **代理池故障轮换**：住宅代理文件 + 免费代理抓取（13 源）双源；429 冷却、出口轮换、健康分、粘滞、直连兜底
- **OpenAI / Anthropic 双协议桥接**：`/v1/chat/completions` 与 `/v1/messages` 双向转换，内置控制面板 `/ui`
- **动态模型目录**：12 个静态兜底 + 启动/定时抓取上游 JS chunk 刷新为当前在线模型（2026-09-24 实测 24 个）
- **随附证据**：抓包 HAR 与站点 JS 位于 `源代码、网络数据包/`；协议逆向笔记见 `docs/PROTOCOL.md`

## 快速开始
1. 运行：`tryingopen2api.exe --config config.json`，默认监听 `http://127.0.0.1:47831`
2. OpenAI 兼容客户端：Base URL `http://127.0.0.1:47831/v1`，API Key `sk-local`（未配置 api_keys 时任意填写）
3. Claude Code：`ANTHROPIC_BASE_URL=http://127.0.0.1:47831`、`ANTHROPIC_API_KEY=sk-local`
4. 可选：编辑 `config.json` 开启免费代理抓取（`free_proxy_enabled: true`）或填写住宅代理文件 `data/proxies.txt`

## 校验
- 资产 `tryingopen2api.exe` SHA-256：`D3E42268AD1B89FE9B881A7CEB8BC15E7E0C5E3C2E04D9C4AE41503580F6A87D`
- 单元测试：`cargo test --lib` / `cargo test --tests` 全部通过
