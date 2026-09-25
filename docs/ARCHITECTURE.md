# 架构说明

## 模块

```
src/
├── main.rs          # 入口：加载配置、构建客户端/代理池/注册表、启动 axum
├── lib.rs           # 模块声明
├── api.rs           # axum 路由 + OpenAI/Anthropic 桥接 + 代理轮换重试
├── config.rs        # 配置解析（config.json + 环境变量）
├── models.rs        # 模型目录（12 静态 + 动态同步）+ 归一化/降级
├── upstream.rs      # TryingOpen HTTP 客户端（/api/open 对话 + 目录抓取）
├── proxy_pool.rs    # 代理池（住宅+免费双源，冷却/轮换/健康分/粘滞）
├── free_proxy.rs    # 免费代理抓取器（44 源 + 公网 IP 过滤 + TCP 预检）
├── prod_guard.rs   # 生产保护：限流/熔断/metrics
├── session.rs       # 会话绑定（下游线程 ↔ 模型）
├── errors.rs        # OpenAI/Anthropic 兼容错误
├── web.rs           # 内置控制面板（单 HTML）
└── protocol/
    ├── responses.rs    # /v1/responses 桥接
├── openai_sse.rs      # 上游 SSE → OpenAI SSE
    ├── anthropic_sse.rs   # 上游 SSE → Anthropic SSE
    └── stream.rs          # reqwest bytes → tokio AsyncRead 适配
tests/
├── models_test.rs   # 模型目录/归一化/解析/代理池
└── proxy_test.rs    # 免费代理解析
```

## 请求路径

```
客户端 → /v1/chat/completions
  → check_api_key（api_keys 未配置则放行）
  → registry.resolve(model)（归一化 + 降级链）
  → build_upstream_request（OpenAI/Anthropic → 上游 messages）
  → try_rounds（代理池轮换）
      ├─ proxy_pool.acquire("residential") → 免费兜底 → 直连兜底
      ├─ upstream.stream(POST /api/open)
      ├─ 429/网络错误 → mark_failure + 指数退避 → 换下一出口
      └─ 成功 → mark_success
  → openai_sse/anthropic_sse 转换 → SSE 回客户端
```

## 代理池

- `load_file`：住宅代理文件（每行 `http://user:pass@host:port` 或 `socks5://`，优先）
- `add_free`：免费代理抓取注入（`source="free"`，量大兜底）
- `acquire`：优先 24h 未用 IP → 健康分降序 + 冷却最早结束 → 全冷却时权宜
- `mark_failure`：EWMA 健康分下调；429 递增冷却（cooldown_map），其它 30s
- `mark_success`：健康分回升
- `get_sticky`：同会话 300s 内复用同出口（防 IP 跳变风控）
- `snapshot`：只暴露 host:port（脱敏 user:pass）

## 免费代理抓取

- 44 个公开源（proxyscrape / geonode / proxifly / thespeedx 等；单轮预检注入上限 4500，池随活性清退）
- 公网 IP 白名单过滤（拒绝内网/回环/保留/组播）
- TCP 连通性预检（3s 超时）
- 注入超 3h 且 30min 未用自动剔除

## SSE 转换

上游 `reasoning-delta/text-delta/finish` → 目标协议对应事件。思考增量单独走 `reasoning_content`（OpenAI）/ `thinking_delta`（Anthropic）。

## 测试

`cargo test --lib --tests`：46 个测试（模型目录/归一化、协议 SSE 转换、代理池冷却/轮换/脱敏、限流/熔断/metrics、日志脱敏等）。
