# 服务器部署记录（TryingOpen2API）

> 更新：2026-09-25
> 服务器：Azure（资源组 UMGC / VM NewAPI02）

## 部署信息

| 项 | 值 |
|---|---|
| 公网 IP | 20.204.27.154 |
| SSH | root@20.204.27.154（用户提供密码） |
| 架构 | Ubuntu 22.04 ARM64 (aarch64)，2核 952MB |
| 服务端口 | 47831（Azure NSG 需放行后公网可访问） |
| 服务方式 | systemd（tryingopen2api.service，restart=always） |
| 二进制 | /opt/tryingopen2api/bin/tryingopen2api（7.5MB） |
| 配置 | /opt/tryingopen2api/config.json |
| 数据 | /opt/tryingopen2api/data/ |
| 上游 | https://www.tryingopen.com（匿名） |
| 代理池 | 免费 44 源 + 住宅文件 data/proxies.txt，默认开启 |

## 鉴权（生产已启用）

- **API Key**：`sk-to-bqsprd1mg4f07i6uywz8le3ho5tj2anx`
  - OpenAI: `Authorization: Bearer <key>` 或 `x-api-key: <key>`
  - 无 key → 401
- **Web UI 密码**：`qg9ozarmn1ikh5`（Basic Auth，用户名任意）
  - 访问 `http://20.204.27.154:47831/ui` 弹出密码框
  - 无密码 → 401
- **healthz**：无鉴权（健康探活用）

## 接入地址（NSG 放行后）

- API Base URL：`http://20.204.27.154:47831/v1`
- OpenAI 端点：`POST /v1/chat/completions`
- Anthropic 端点：`POST /v1/messages`
- 模型列表：`GET /v1/models`
- Web UI：`http://20.204.27.154:47831/ui`（密码 qg9ozarmn1ikh5）
- healthz：`http://20.204.27.154:47831/healthz`

## Azure NSG 放行步骤（必做，否则公网 503）

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

## 已真实验收（2026-09-25）

- [x] 服务器编译成功（aarch64 release 7.5MB）
- [x] systemd 服务 active running
- [x] healthz 200（`{"ok":true,"models":24,"version":"0.1.2"}`）
- [x] /ui 无密码 401、带密码 200
- [x] /v1/models 无 key 401、带 key 200
- [x] 真实对话 E2E（服务器→上游）200，含 reasoning+usage
- [x] SSH 隧道全链路 E2E 200（healthz + chat）
- [ ] 公网直连 47831（**待 NSG 放行**）

## 安全注意
- 配置含 key/密码：勿提交 git、勿外泄
- 生产用 HTTP（Azure 公网）；如需 HTTPS 建议前端挂 Nginx + Let's Encrypt
