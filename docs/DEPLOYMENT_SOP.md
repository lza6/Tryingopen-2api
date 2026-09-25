# 部署与运维 SOP（N6 产出）

> 目标：新环境按本 SOP 可独立部署、启动、监控、排障、交接。

## 1. 环境要求
- Windows / Linux（Rust 交叉编译；本项目默认 Windows 产物）
- Rust 工具链（stable，>=1.85）
- 无外部依赖（单二进制；SQLite 内嵌）

## 2. 构建
```bash
# Windows
build.bat                      # 或 cargo build --release
# 产物
target/release/tryingopen2api.exe
```
> 注意：build.bat 旧注释的 `-- --config` 双横线是笔误；正确为 `--config config.json`。

## 3. 配置（config.json）
| 字段 | 默认 | 生产建议 |
|---|---|---|
| listen_addr | 127.0.0.1:47831 | 内网/本机；公网暴露经反向代理 |
| api_keys | [] | 暴露公网必须配置；否则任意 key 可访问 |
| upstream_base_url | https://www.tryingopen.com | 固定 |
| free_proxy_enabled | true | 生产建议 true（代理池拉满） |
| hourly_per_ip | 20 | 按上游实际限流调整 |
| max_attempts | 3 | 代理轮换轮数 |
| direct_fallback | true | 直连兜底（消耗本机配额） |
| proxy_file | data/proxies.txt | 住宅代理（每行 http://user:pass@host:port） |
| request_timeout_sec | 120 | 上游读超时 |
| catalog_refresh_min | 30 | 模型目录刷新周期 |

环境变量覆盖：LISTEN_ADDR / UPSTREAM_BASE_URL / API_KEYS / PROXY_FILE / FREE_PROXY_ENABLED / HOURLY_PER_IP / MAX_ATTEMPTS 等。

## 4. 启动
```bash
# 前台（调试）
target/release/tryingopen2api.exe --config config.json

# 后台（Windows）
start /b target/release/tryingopen2api.exe --config config.json > server.log 2>&1

# systemd（Linux，示例）
# [Service] ExecStart=/opt/tryingopen2api --config /etc/tryingopen2api/config.json Restart=always
```

## 5. 健康检查与监控
- `GET /healthz` → `{"ok":true,"app":"tryingopen2api","version": 以实际 /healthz 输出为准,"models":N,"proxies":N}`（探活）
- `GET /api/proxies` → 代理池容量/健康分/延迟（容量监控）
- `GET /v1/models` → 模型列表（目录同步状态）
- 日志：RUST_LOG=info（默认 info+tryingopen2api=debug）；关键事件：
  - `免费代理抓取后台已启动（每 30 分钟刷新）` → 后台正常
  - `免费代理刷新注入 N 个` → 抓取成功
  - `上游健康检查失败` → 上游可达性（WARN，继续运行）
  - `TryingOpen 上游调用失败（已轮换 N 个出口 + 直连兜底）` → 全出口失败（需关注）

## 6. 容量语义（生产关键）
- 每 IP 每日约 20 次（上游，UTC 24h 窗口）；代理池自动轮换出口
- `capacity_remaining = 可用代理数 × 20 − 已用`；UI 实时显示
- 直连兜底消耗本机 IP 配额；高并发优先保证代理池健康

## 7. 排障手册
| 症状 | 排查 |
|---|---|
| 请求 429 | 代理池耗尽/上游限流 → 看 /api/proxies 冷却项；调 max_attempts/hourly_per_ip；补住宅代理 |
| 502 upstream_error | 上游不可达/模型无效 → 看日志错误详情；等上游恢复 |
| 代理池 0 代理 | free_proxy_enabled=false 或抓取失败 → 手动 POST /api/proxies/refresh-free；查网络 |
| 模型列表少了 | 上游目录变化 → POST /api/catalog/refresh 手动同步 |
| UI 打不开 | 服务未起/端口占用 → healthz 探活；改 listen_addr |
| 内存增长 | 正常（session map 已限 5000）；异常看 /api/proxies inflight 是否泄漏 |
| 慢请求 | 上游生成慢（4-58s 正常）→ 客户端用 stream=true |

## 8. 交接清单
- [ ] README.md 快速开始
- [ ] docs/API_CONTRACT.md 契约
- [ ] docs/PROTOCOL.md 上游协议
- [ ] docs/audit/* 审计结论
- [ ] config.example.json（无敏感信息）
- [ ] workflow_status.md（任务/验证记录）
- [ ] CI 状态（main push 全绿）
- [ ] Release 资产（以当前版本 tag 为准）

## 9. 安全注意
- 公网暴露：必须配 api_keys + 反向代理（HTTPS）
- 住宅代理文件含凭据：勿提交到 git（.gitignore 已排除 data/proxies.txt）
- 日志脱敏默认开（redact_logs:true）
- 定期 cargo audit + CI gitleaks
