# try.hwhcie.bond — nginx HTTPS 反代配置（tryingopen-2api）

> 2026-09-26 上线。域名 `try.hwhcie.bond` 解析到 `20.204.27.154`，
> nginx 443 反向代理到本机 `127.0.0.1:47831`（tryingopen2api systemd 服务）。

## 拓扑

```
客户端 → https://try.hwhcie.bond:443
         → nginx (20.204.27.154, sites-enabled/tryingopen)
         → http://127.0.0.1:47831 (tryingopen2api, v0.1.10)
```

同机还跑着 imagefree（听风AI）：`imagefree/api/admin.hwhcie.bond` → `127.0.0.1:8100`，
证书与 tryingopen 共用 `/etc/letsencrypt/live/imagefree.hwhcie.bond/`。

## 配置（/etc/nginx/sites-enabled/tryingopen）

```nginx
server {
    listen 80;
    server_name try.hwhcie.bond;
    location /.well-known/acme-challenge/ { root /var/www/certbot; }
    location / { return 301 https://$host$request_uri; }
}

server {
    listen 443 ssl http2;
    server_name try.hwhcie.bond;

    ssl_certificate     /etc/letsencrypt/live/imagefree.hwhcie.bond/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/imagefree.hwhcie.bond/privkey.pem;

    client_max_body_size 50m;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";

    location / {
        proxy_pass http://127.0.0.1:47831;
        proxy_read_timeout 300s;
        proxy_buffering off;
    }
}
```

## 证书扩容命令（第一次要跑）

```bash
certbot certonly --webroot -w /var/www/certbot \
  -d imagefree.hwhcie.bond -d api.hwhcie.bond -d admin.hwhcie.bond -d try.hwhcie.bond \
  --cert-name imagefree.hwhcie.bond --force-renewal --non-interactive
```

## 验证（从任意公网机器）

```bash
curl -sk https://try.hwhcie.bond/healthz
curl -sk -u admin:<UI密码> https://try.hwhcie.bond/ui
curl -sk -H "Authorization: Bearer <API_KEY>" https://try.hwhcie.bond/v1/models
```

## 故障排查

- 本机（Windows + v2rayN）带 SNI 访问 443 可能被透明代理 reset，属本机链路问题；
  服务器本机 `curl -sk --resolve try.hwhcie.bond:443:20.204.27.154 ...` 与第三方机器均正常。
- `imagefree.hwhcie.bond` 与 `try.hwhcie.bond` 共用证书，证书 SAN 已含 try。
