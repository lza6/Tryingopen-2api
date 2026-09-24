# N4 安全审计报告（终局闭环）

> 日期：2026-09-24
> 审计面：依赖漏洞 / 硬编码密钥 / SSRF / 注入 / 日志脱敏 / body 限制

## 审计结果

| 项 | 状态 | 证据 |
|---|---|---|
| 依赖漏洞 | ✅ 0 漏洞 | cargo audit：243 依赖，vulnerabilities.found=false |
| 硬编码密钥 | ✅ 无 | 仅文档占位 sk-local；API key 仅内存 RwLock，不落盘不打印 |
| SSRF（免费源） | ✅ 已防护 | free_proxy 公网 IP 白名单过滤（is_valid_public_ip） |
| SSRF（住宅文件） | 🔴→✅ 已修复 | 原 normalize_proxy_url 不校验 → 新增 sanitize_proxy_url（公网 IP/排除内网/回环/链路本地/组播/元数据）接入 load_file |
| SSRF（add_free 纵深） | 🔴→✅ 已修复 | add_free_with_latency 接入 sanitize_proxy_url 双保险 |
| 请求体大小 | 🔴→✅ 已修复 | DefaultBodyLimit 16MB（防内存打爆） |
| 日志脱敏 | ✅ | redact_logs 配置位；cookie/key 不进日志 |
| 认证 | ✅ | api_keys 空=本机放行（默认）；配置后强制 Bearer/x-api-key |
| 错误信息泄漏 | ✅ | 上游错误截断 240/400 字符，不泄漏完整内部细节 |

## 修复项（本节点）
1. `sanitize_proxy_url`：住宅代理文件 + 免费代理注入统一公网地址校验
   - 拒绝：私网/回环/链路本地/组播/未指定/广播/文档地址
   - 拒绝非 IP hostname（要求 IP:port）
2. `DefaultBodyLimit 16MB`（N1 已做，此处归档）

## 遗留建议（非阻塞）
- gitleaks 仅在 CI 跑（本机未装）；开发时可 `cargo install gitleaks` 或依赖 CI
- 若未来接外部用户隔离，建议加 each-IP rate limit 层（当前靠代理池+hourly_per_ip 语义）
- 面板 /ui 未做登录（本机工具）；如需暴露公网应加反向代理认证
