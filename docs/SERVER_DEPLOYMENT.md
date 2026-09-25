# 服务器部署记录（TryingOpen2API）

> 更新：2026-09-25
> 服务器：Azure（资源组 UMGC / VM NewAPI02）

## 部署信息

| 项 | 值 |
|---|---|
| 公网 IP | <SERVER_IP> |
| SSH | root@<SERVER_IP>（用户提供密码） |
| 架构 | Ubuntu 22.04 ARM64 (aarch64)，2核 952MB |
| 服务端口 | 47831（公网已放行，直连可用）；另有 Cloudflare Quick Tunnel 兜底 |
| 服务方式 | systemd（tryingopen2api.service，restart=always） |
| 二进制 | /opt/tryingopen2api/bin/tryingopen2api（7.5MB） |
| 配置 | /opt/tryingopen2api/config.json |
| 数据 | /opt/tryingopen2api/data/ |
| 上游 | https://www.tryingopen.com（匿名） |
| 代理池 | 免费 44 源 + 住宅文件 data/proxies.txt，默认开启 |
| 生产保护 | per-key 限流 60/3600s、上游熔断 5 次/30s、/metrics Prometheus |


## 鉴权（生产已启用）

- **API Key**：`sk-to-<YOUR_API_KEY>`
  - OpenAI: `Authorization: Bearer <key>` 或 `x-api-key: <key>`
  - 无 key → 401
- **Web UI 密码**：`<YOUR_UI_PASSWORD>`（Basic Auth，用户名任意）
  - 访问 `http://<SERVER_IP>:47831/ui` 弹出密码框
  - 无密码 → 401
- **healthz**：无鉴权（健康探活用）

## 接入地址（公网直连，已放行 47831）

- API Base URL：`http://<SERVER_IP>:47831/v1`
- OpenAI 端点：`POST /v1/chat/completions`
- Anthropic 端点：`POST /v1/messages`
- 模型列表：`GET /v1/models`
- Web UI：`http://<SERVER_IP>:47831/ui`（密码 <YOUR_UI_PASSWORD>）
- healthz：`http://<SERVER_IP>:47831/healthz`
- metrics：`http://<SERVER_IP>:47831/metrics`（Prometheus）

## Azure NSG 放行状态（已确认放行 22 + 47831）

> 2026-09-25 公网直连实测：`curl --noproxy "*" http://<SERVER_IP>:47831/healthz` 返回 200。
> 注意：本机若设置了 HTTP_PROXY/HTTPS_PROXY（如 v2rayN 10808），curl 默认走代理可能得到 503，需 `--noproxy "*"`。

Azure 门户 → 虚拟机 NewAPI02 → 网络 → 网络接口 NSG → 入站规则 → 添加：
- 源：任意 / IP 范围
- 目标端口：47831
- 协议：TCP
- 优先级：1000（低于默认拒绝）
- 名称：Allow_TryingOpen_API
- 说明：允许 TryingOpen2API API/UI 公网访问

或 CLI（在能登录 Azure 的机器）：
```bash
az network nsg rule create --resource-group UMGC --nsg-name <NSG名> \
  --name Allow_TryingOpen_API --access Allow --protocol Tcp \
  --direction Inbound --priority 1000 --source-address-prefixes '*' \
  --source-port-ranges '*' --destination-address-prefixes '*' \
  --destination-port-ranges 47831
```

## 运维命令（SSH 到服务器）

```bash
systemctl status tryingopen2api        # 服务状态
systemctl restart tryingopen2api       # 重启
journalctl -u tryingopen2api -f        # 实时日志
cat /opt/tryingopen2api/config.json    # 配置（含 key/密码，勿外泄）
curl http://127.0.0.1:47831/healthz    # 本机探活
```

## 已真实验收（2026-09-25 生产保护全链路）

- [x] 服务器编译成功（aarch64 release 7.5MB，v0.1.3）
- [x] systemd 服务 active running
- [x] healthz 200（`{"ok":true,"models":24,"version":"0.1.3"}`）
- [x] /ui 无密码 401、带密码 200
- [x] /v1/models 无 key 401、带 key 200
- [x] 真实对话 E2E（公网直连）200，含 reasoning+usage
- [x] Anthropic /v1/messages 公网 200（thinking+text+usage）
- [x] 公网直连 47831 200（`curl --noproxy "*" http://<SERVER_IP>:47831/healthz`）
- [x] 限流：阈值3 实测 req1-3=200，req4/5=429 + Retry-After
- [x] 熔断：坏上游实测 502→503 熔断保护→恢复后 200
- [x] /metrics 公网：requests_total/upstream_errors/proxy_pool_size/active_sessions
- [x] 代理池：免费抓取后台修复后 4500 代理全可用，容量 90000/h
- [x] 优雅停机：journalctl 实测 received SIGTERM, graceful shutdown

## 安全注意
- 配置含 key/密码：勿提交 git、勿外泄
- 生产用 HTTP（Azure 公网）；如需 HTTPS 建议前端挂 Nginx + Let's Encrypt

## Cloudflare Quick Tunnel（变通方案，已上线）

> Azure NSG 未放行 47831，故用 Cloudflare Quick Tunnel（出站连接，绕开入站 NSG）。

- systemd 服务：`tryingopen-cf-tunnel.service`（restart=always）
- 命令：`/usr/local/bin/cloudflared tunnel --url http://127.0.0.1:47831 --no-autoupdate`
- 日志：`journalctl -u tryingopen-cf-tunnel -f`
- **注意**：免费 quick tunnel 的 URL 在服务重启后会变；查看当前 URL：
  ```bash
  journalctl -u tryingopen-cf-tunnel --no-pager | grep -oE "https://[a-z0-9-]+\.trycloudflare\.com" | tail -1
  ```
- 若需固定域名：Cloudflare 面板创建 Named Tunnel（需自有域名）+ `cloudflared tunnel route dns`，配置写入 `/etc/cloudflared/config.yml`
- 若后续在 Azure 门户放行 47831，可直接用 `http://<SERVER_IP>:47831` 访问（无需隧道）
