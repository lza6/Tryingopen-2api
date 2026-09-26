# TryingOpen2API

> Rust(axum) 版。上游协议逆向笔记: [docs/PROTOCOL.md](docs/PROTOCOL.md)

TryingOpen2API 把 [tryingopen.com](https://www.tryingopen.com) 免费层的开源模型（内置 12 个 + 动态目录）逆向为 **OpenAI 兼容**与 **Anthropic 兼容** 的本地 API 网关。单二进制、零外部依赖，可在任意 OpenAI/Claude 客户端（Claude Code、Codex、Cursor、LobeChat、NextChat 等）中使用 TryingOpen 免费模型。

**完全匿名**：tryingopen.com 的所有对话端点不需要 Cookie / 登录 / API Key。站点按「每 IP 每日约 20 次」限流（代理池轮换出口缓解），网关内置 **代理池自动故障轮换**（住宅代理文件 + 免费代理抓取双源，429 自动冷却换出口，指数退避重试，直连兜底）。

> 本项目是把 `imagefree-2ai` 里的 tryingopen 提供商 + 代理池单独抽出，按 `tokenharbor-2api` 的架构重写的独立网关。抓包与站点 JS 已随附在 `源代码、网络数据包/`。\n>\n > **v0.1.16（协议对齐 + 错误可操作化）**：模型能力字段透传（思考/消息数上限/降级建议）、429 按上游建议模型自动降级、每 key 用量统计（/api/usage）、请求日志结构化 JSON、config.local.json 局部覆盖。此前已含：代理池 44 源（免费源抓取，单轮预检注入上限 4500）、低延迟优先 + 并发门控、工具调用转换、思考解析、effort 透传、多模态、模型下线自动降级、UI 容量实时显示。

---

## 一、快速开始（3 步）

### 第 1 步：编译

```bash
# Windows
build.bat

# 或手动
cargo build --release
./target/release/tryingopen2api.exe --config config.json
```

默认监听 `http://127.0.0.1:47831`（与 tokenharbor2api 的 47830 错开）。

### 第 2 步：可选配置代理池

编辑 `config.json`：

```jsonc
{
  "proxy_file": "data/proxies.txt",        // 住宅/自备代理，每行 http://user:pass@host:port
  "free_proxy_enabled": true,              // 开启免费代理抓取（默认 true，44 源）
  "hourly_per_ip": 20,                     // tryingopen 单 IP 每日限流
  "max_attempts": 3,                       // 每请求最多换几个出口
  "direct_fallback": true                  // 全部代理失败后直连兜底
}
```

不配代理也能直接用（本机 IP 每日 20 次额度）。

### 第 3 步：接入客户端

**OpenAI SDK（Python）**

```python
from openai import OpenAI
client = OpenAI(base_url="http://127.0.0.1:47831/v1", api_key="sk-local")
resp = client.chat.completions.create(
    model="qwen/qwen3.8-27b",
    messages=[{"role": "user", "content": "你好"}],
)
print(resp.choices[0].message.content)
```

**Claude Code**（Anthropic 协议）

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:47831
export ANTHROPIC_API_KEY=sk-local   # 未配置 api_keys 时随意填
```

**Cursor / Continue / 任意 OpenAI 兼容客户端**

```
Base URL: http://127.0.0.1:47831/v1
API Key:  sk-local（或面板生成）
模型:     从 /v1/models 里选
```

---

## 二、API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI 聊天（流式 / 非流式） |
| `/v1/messages` | POST | Claude 聊天（双向转换，流式为 Anthropic 事件流） |
| `/v1/models` | GET | 可用模型列表（静态目录 + 启动/定时动态同步） |
| `/healthz` | GET | 健康检查 |
| `/api/proxies` | GET | 代理池快照（脱敏 host:port / 健康分 / 冷却） |
| `/api/proxies/refresh-free` | POST | 手动抓取一轮免费代理 |
| `/api/catalog/refresh` | POST | 手动同步上游模型目录 |
| `/api/guide` | GET | 接入信息 |
| `/api/usage` | GET | 每 API Key 用量统计（需鉴权） |
| `/api/config/api-key` | POST | 运行时生成 / 设置 / 清除下游 API Key |
| `/ui` | GET | 内置控制面板 |

---

## 三、模型目录

网关内置 **12 个静态模型**，启动时自动抓取上游首页 JS chunk 刷新为**当前在线模型**（2026-09-24 实测抓到 24 个）。

### 静态兜底（12 个）

| 模型 ID | 名称 | 上下文 | 价格/M |
|---------|------|--------|--------|
| `qwen/qwen3.8-27b` | Qwen3.8 27B | 262k | 3.2 |
| `nvidia/nemotron-3.5-lightning` | Nemotron 3.5 Lightning | 262k | 0.25 |
| `deepseek/deepseek-v4-flash-0731` | DeepSeek V4 Flash | 1M | 0.18 |
| `deepseek/deepseek-v4-pro-0813` | DeepSeek V4 Pro | 1M | 1.98 |
| `google/gemma-4-31b-it` | Gemma 4 31B | 256k | 0.4 |
| `google/gemma-4-26b-a4b-it` | Gemma 4 26B | 256k | 0.4 |
| `openai/gpt-oss-120b` | GPT-OSS 120B | 128k | 0.6 |
| `meta/muse-glimmer-30b` | Muse Glimmer 30B | 131k | 1.5 |
| `moonshotai/kimi-k3` | Kimi K3 | 1M | 8.5 |
| `minimax/minimax-m3` | MiniMax M3 | 1M | 1.2 |
| `thinkingmachines/inkling-small` | Inkling Small | 524k | 1.2 |
| `z-ai/glm-5.2` | GLM 5.2 | 1M | 1.54 |

---

## 四、代理池（核心能力）

- **双源**：住宅代理文件（`data/proxies.txt`，每行一个 `http://user:pass@host:port`，优先）+ 免费代理（44 个公开源后台抓取，量大兜底）
- **每 IP 限流语义**：按 `hourly_per_ip`（默认 20）控制每个出口的使用次数；24h 窗口重置
- **故障轮换**：429 / 网络错误 → `mark_failure` 冷却该出口 + 健康分 EWMA 下调 + 指数退避（2s/4s/8s）→ 下一轮换新出口
- **优先策略**：24h 内未用过的 IP 优先；全部用过一轮后按健康分 + 冷却最早结束排序
- **直连兜底**：`max_attempts` 轮代理全部失败后直连本机 IP 一次（本机也有每日 20 次配额）
- **脱敏**：面板/API 只暴露 `host:port`，不泄漏住宅代理 `user:pass`
- **粘滞**：同下游会话优先复用同出口（300s 窗口），避免触发上游 IP 跳变风控

---

## 五、上游协议（逆向摘要）

TryingOpen 站点（Next.js + Turbopack，完全匿名）：

- `POST /api/open` — SSE 对话流（无 Cookie）
  - 请求体：`{id, trigger:"submit-message", messageId, model, effort:"balanced", messages:[{id,role,parts:[{type:"text"|"file",text?,mediaType?,url?}]}], stream:true}`
  - SSE 事件：`start` / `start-step` / `reasoning-start` / `reasoning-delta` / `reasoning-end` / `text-start` / `text-delta` / `text-end` / `finish-step` / `finish`（含 `finishReason` + `messageMetadata.inputTokens/outputTokens/totalTokens/reasoningTokens/costUsd`）/ `error` / `[DONE]`
- `GET /` — 首页 HTML（含 `/_next/static/chunks/*.js` 引用）
- `GET /_next/static/chunks/*.js` — 模型目录（`{id,name,maker,context,supportsTools,supportsImages,pricePerMTok,messageLimit,cheaperFallbackId}`）

网关实现的转换：

- **OpenAI 流**：`reasoning-delta` → `delta.reasoning_content`；`text-delta` → `delta.content`；`finish` → `finish_reason` + `[DONE]`
- **Anthropic 流**：`reasoning-delta` → `content_block_delta thinking_delta`；`text-delta` → `content_block_delta text_delta`

完整字段说明见 [docs/PROTOCOL.md](docs/PROTOCOL.md)。

---

## 六、能力说明

- 多模态：支持 `image_url`（OpenAI）/ `image`（Anthropic）→ 上游 `parts[].file`（data URL / mediaType）
- 系统提示：上游无 system 角色，网关把 system 拼进第一条 user 的 `[SYSTEM INSTRUCTIONS]`
- 工具调用：`/api/open` 无原生 tool_calls，上游模型按纯文本 JSON 输出，客户端可自行解析（tryingopen 提供商标注 supportsTools）
- 目录同步：启动 + 每 `catalog_refresh_min` 分钟刷新一次，失败保留静态目录
- 下游 API Key：配置 `api_keys` 后客户端必须带；空 = 仅本机放行

---

## 七、配置（config.json）

| 字段 | 默认 | 说明 |
|------|------|------|
| `listen_addr` | `127.0.0.1:47831` | 监听地址 |
| `upstream_base_url` | `https://www.tryingopen.com` | 上游 |
| `api_keys` | `[]` | 下游 API Key（空=放行） |
| `default_model` | `qwen/qwen3.8-27b` | 默认模型 |
| `fallback_models` | deepseek/glm/minimax | 降级链 |
| `request_timeout_sec` | `120` | 请求超时 |
| `catalog_refresh_min` | `30` | 目录刷新周期 |
| `proxy_file` | `data/proxies.txt` | 住宅代理文件 |
| `free_proxy_enabled` | `true` | 免费代理抓取开关（默认开，44 源） |
| `free_proxy_refresh_min` | `30` | 免费代理刷新周期 |
| `hourly_per_ip` | `20` | 每 IP 每日限流 |
| `max_attempts` | `3` | 最大出口尝试轮数 |
| `cooldown_map` | `0,15,60,120,300` | 递增冷却秒数 |
| `direct_fallback` | `true` | 直连兜底 |
| `direct_fallback_quota` | `10` | 直连兜底每窗口配额（防共享匿名配额打满） |
| `rate_limit_enabled` / `rate_limit_requests` / `rate_limit_window_sec` | `true`/`60`/`3600` | 每 Key 限流 |
| `rate_limit_max_keys` | `4096` | 限流 map 上限（防无界内存） |
| `circuit_breaker_enabled` / `cb_failure_threshold` / `cb_timeout_sec` | `true`/`5`/`30` | 上游熔断 |
| `metrics_enabled` | `true` | /metrics（需 API key） |
| `ui_password` | `` | 面板 Basic Auth |

环境变量覆盖：`LISTEN_ADDR` / `UPSTREAM_BASE_URL` / `API_KEYS` / `DEFAULT_MODEL` / `PROXY_FILE` / `FREE_PROXY_ENABLED` / `HOURLY_PER_IP` / `MAX_ATTEMPTS` / `COOLDOWN_MAP` / `DIRECT_FALLBACK` 等。

---

## 八、测试

```bash
cargo test   # 57 个测试：模型/协议/代理池/限流/熔断/metrics/用量统计 等
```

> 注：本机 rustdoc.exe 缺失导致 `cargo test --doc` 失败（chocolatey 安装问题），与代码无关；`cargo test --lib` / `cargo test --tests` 全部通过。

## 九、目录结构

```
src/
├── main.rs          # 入口：加载配置、构建客户端/代理池、启动 axum
├── lib.rs           # 模块声明
├── api.rs           # axum 路由 + OpenAI/Anthropic 桥接 + 代理轮换重试
├── config.rs        # 配置解析（config.json + 环境变量）
├── models.rs        # 模型目录（12 静态 + 动态同步）+ 归一化/降级
├── upstream.rs      # TryingOpen HTTP 客户端（对话/目录抓取）
├── proxy_pool.rs    # 代理池（住宅+免费双源，冷却/轮换/健康分）
├── free_proxy.rs    # 免费代理抓取器（44 源 + 公网 IP 过滤 + TCP 延迟预检）
├── session.rs       # 会话绑定（线程 ↔ 模型）
├── errors.rs        # OpenAI/Anthropic 兼容错误
├── prod_guard.rs    # 生产保护：限流/熔断/metrics/用量统计
├── web.rs           # 内置控制面板（单 HTML）
└── protocol/
    ├── openai_sse.rs      # 上游 SSE → OpenAI SSE
    ├── openai_sse_helper.rs  # 非流式 OpenAI 组装
    ├── anthropic_sse.rs   # 上游 SSE → Anthropic SSE
    ├── responses.rs       # /v1/responses 桥接
    ├── stream.rs          # reqwest bytes → tokio AsyncRead 适配
    └── mod.rs             # 协议模块声明
tests/
├── models_test.rs   # 模型目录/归一化/解析/代理池
└── proxy_test.rs    # 免费代理解析
源代码、网络数据包/   # 抓包 HAR + 站点 JS（imagefree-2ai 搬来）
docs/
├── PROTOCOL.md      # 上游协议逆向笔记
└── ARCHITECTURE.md  # 架构说明
```


## 生产部署 / 访问（2026-09-26）

### 生产环境拓扑

```
客户端 → https://try.hwhcie.bond:443
         → nginx (20.204.27.154, sites-enabled/tryingopen)
         → http://127.0.0.1:47831 (tryingopen2api systemd, v0.1.12)
```

- 服务器：`20.204.27.154`（Azure Ubuntu 22.04，nginx 1.18 + systemd）
- 域名：`try.hwhcie.bond`（Let's Encrypt 证书，SAN 含 try）
- 同机还跑 imagefree：`imagefree/api/admin.hwhcie.bond` → `127.0.0.1:8100`
- 完整 nginx 配置与排障见 `docs/NGINX_DEPLOY.md`

### 生产访问

| 用途 | 地址 |
|---|---|
| OpenAI 兼容 API | `https://try.hwhcie.bond/v1` |
| Anthropic 兼容 | `https://try.hwhcie.bond/v1/messages` |
| Web UI | `https://try.hwhcie.bond/ui` |
| 健康检查 | `https://try.hwhcie.bond/healthz` |

> 凭据（API Key / UI 密码）已轮换，不写在此；见服务器 `/opt/tryingopen2api/config.json`
> 或部署者保管的密钥记录。

### 部署流水线

- push main → GitHub Actions CI（fmt/clippy/test/security）→ CD-Deploy 自动部署到服务器
- CD 流程：打包源码 → paramiko 上传 → 服务器 cargo build --release（nice 低优先级）→ 停服替换重启 → healthz 验证
- Secrets：`SSH_HOST` / `SSH_USER` / `SSH_PASS`
