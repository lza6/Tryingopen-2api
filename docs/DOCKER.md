# TryingOpen2API Docker 部署指南

> 适用版本：tryingopen2api（当前版本以 /healthz 或 Cargo.toml 为准）（Rust/axum，端口 47831）。
> 交付文件：`Dockerfile`、`.dockerignore`、`docker-compose.yml`、本指南。

## 1. 交付内容与设计要点

| 文件 | 作用 |
|---|---|
| `Dockerfile` | 多阶段构建：builder 编译 release 二进制，runtime 只跑二进制 |
| `.dockerignore` | 把 `target/`、`.git/`、`data/*.txt`、本地配置、参考材料挡在构建上下文外 |
| `docker-compose.yml` | 一键 build/run，端口 + 配置/数据卷 + 自动重启 |
| `docs/DOCKER.md` | 本指南 |

关键设计：

- **builder 阶段**：`rust:1.85-bookworm`，只装 `build-essential` 和 `pkg-config`。
  - 纯 Rust 依赖，无需 C 编译器（已移除 rusqlite）。
  - `reqwest` 走 `rustls-tls`（见 `Cargo.toml`），不依赖系统 OpenSSL，所以不装 `libssl-dev`；若以后切回 native-tls 再补装。
- **runtime 阶段**：`debian:bookworm-slim`，只装 `ca-certificates`（HTTPS 出站需要），创建非 root 用户 `appuser`，只拷贝 `target/release/tryingopen2api` 一个文件。
- **配置与数据**：项目没有默认内置 config（用 `--config` 参数），容器默认执行
  `ENTRYPOINT ["/app/tryingopen2api"]` + `CMD ["--config", "/app/config.json"]`，
  config 由卷挂载，不打进镜像；`/app/data` 挂载持久化 `proxies.txt`；**会话为内存态**，重启即清（无 sqlite/telemetry 库）。
- 不需要 `CARGO_NET_GIT_FETCH_WITH_CLI`，本项目没有 git 依赖，也不使用 cross。

## 2. 前置条件

- Docker Engine 20.10+（或 Docker Desktop / 远端 daemon），网络能拉取 crates.io 与基础镜像。
- 宿主机准备两份文件：

  | 文件 | 说明 |
  |---|---|
  | `config.json` | 服务配置，必须把 `listen_addr` 改为 `0.0.0.0:47831`（否则容器内只监听 127.0.0.1，宿主机无法访问） |
  | `data/` 目录 | 存放 `proxies.txt`（无 sqlite/telemetry 数据） |

  `config.json` 未提供时，把仓库 `config.example.json` 复制为 `config.json` 并按需修改：

  ```bash
  cp config.example.json config.json
  # 确认或改为：
  # "listen_addr": "0.0.0.0:47831"
  ```

## 3. 构建镜像

```bash
docker build -t tryingopen2api:latest .
```

- 首次构建需下载基础镜像和全部 crates，耗时较长；第二次构建会命中依赖层缓存。
- 构建产物验证：

```bash
docker run --rm tryingopen2api:latest --help
docker image inspect tryingopen2api:latest | grep -i exposedports
```

## 4. 运行容器

### 4.1 直接 docker run

```bash
docker run -d --name tryingopen2api \
  -p 47831:47831 \
  -v "$PWD/config.json:/app/config.json:ro" \
  -v "$PWD/data:/app/data" \
  -e RUST_LOG=info \
  --restart unless-stopped \
  tryingopen2api:latest
```

### 4.2 docker compose（推荐）

```bash
docker compose up -d --build
```

等价于：构建当前目录镜像 → 映射 `47831:47831` → 挂载 `./config.json:/app/config.json:ro` 与 `./data:/app/data` → `restart: unless-stopped`。

## 5. 配置与数据卷

### 5.1 配置挂载

- 容器内固定路径：`/app/config.json`（只读挂载 `:ro`）。
- 宿主机侧修改 `config.json` 后重启容器生效：

```bash
docker compose restart tryingopen2api
# 或
docker restart tryingopen2api
```

- 也可以不挂载文件，改用环境变量覆盖（程序支持 `CONFIG_PATH` 或各字段环境变量）：

```bash
docker run ... -e CONFIG_PATH=/app/config.json tryingopen2api:latest
# 单个字段覆盖示例：
docker run ... -e LISTEN_ADDR=0.0.0.0:47831 -e UPSTREAM_BASE_URL=https://www.tryingopen.com ...
```

### 5.2 数据卷

- `./data` 挂载到 `/app/data`，保存：
  - `data/proxies.txt`（代理池文件）
  - `data/`（proxies.txt；会话内存态）
  - （无；遥测走 /metrics 内存计数）
- 容器重建/升级后 proxies.txt 保留在宿主机 `./data`；配置在 `config.json`（卷挂载），**升级前建议备份 `config.json` 与 `data/proxies.txt`**；会话数据在内存中，重启即清空。
- 想换目录：把 compose 里的 `./data` 改为 `/绝对/路径/数据目录`，或换成 named volume（`proxies.txt` 数据仍在 volume 内）。

### 5.3 常见环境变量（程序内 `src/config.rs` 支持）

| 变量 | 说明 | 默认 |
|---|---|---|
| `RUST_LOG` | 日志级别（如 `info`、`debug`、`tryingopen2api=debug`） | 无 |
| `CONFIG_PATH` | 配置文件路径 | `config.json` |
| `LISTEN_ADDR` | 监听地址 | `127.0.0.1:47831` |
| `UPSTREAM_BASE_URL` | 上游地址 | `https://www.tryingopen.com` |
| `API_KEYS` | 逗号分隔的 API Key | 空 |
| `UI_PASSWORD` | Web 面板 Basic Auth 密码（公网务必设置） | 空 |
| `RATE_LIMIT_ENABLED` / `RATE_LIMIT_REQUESTS` / `RATE_LIMIT_WINDOW_SEC` |
| `RATE_LIMIT_MAX_KEYS` | 限流 map 上限（防无界内存） | 4096 |
| `DIRECT_FALLBACK_QUOTA` | 直连兜底每窗口配额 | 10 | 每 Key 限流 | 开 / 60 / 3600 |
| `CIRCUIT_BREAKER_ENABLED` / `CB_FAILURE_THRESHOLD` / `CB_TIMEOUT_SEC` | 上游熔断 | 开 / 5 / 30 |
| `METRICS_ENABLED` | `/metrics` Prometheus 端点 | 开 |

## 6. 健康检查

程序提供 `GET /healthz`（返回 `{"ok":true,...}`）。compose 已内置 healthcheck（bash + `/dev/tcp`，无需 curl/wget）：

```yaml
    healthcheck:
      test: ["CMD", "bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/47831 && echo -e 'GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' >&3 && cat <&3 | grep -q '\"ok\":true'"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s
```

> 说明：镜像刻意不装 curl/wget（保持 runtime 最小化）；上面的 healthcheck 用 bash + `/dev/tcp`，无需额外安装。

宿主机侧手动探测：

```bash
curl -fsS http://127.0.0.1:47831/healthz
docker logs --tail 50 tryingopen2api
```

## 7. 升级步骤（无数据丢失）

1. 备份数据（config.json + data/proxies.txt；会话内存态无需备份）：

   ```bash
   cp -r data "data.bak-$(date +%Y%m%d%H%M%S)"
   ```

2. 拉新代码并重建：

   ```bash
   git pull
   docker compose up -d --build
   ```

3. 容器替换后自检：

   ```bash
   docker compose ps
   curl -fsS http://127.0.0.1:47831/healthz
   docker logs --tail 50 tryingopen2api
   ```

4. 回滚：旧镜像还在本地时

   ```bash
   docker compose down
   docker run ... tryingopen2api:<旧tag>   # 或 git checkout 旧版本后重新 build
   ```

## 8. 常见问题

- **宿主机访问不到 47831**：`config.json` 的 `listen_addr` 必须为 `0.0.0.0:47831`（或 `LISTEN_ADDR=0.0.0.0:47831`）。
- **HTTPS 失败 / CA 错误**：runtime 已装 `ca-certificates`；若仍报证书错，检查容器时间与代理环境。
- **构建失败找不到 `cc`**：builder 已装 `build-essential`；若改了 Cargo.toml 引入 native 依赖，需按报错补 `pkg-config`/对应 dev 包。
- **数据未持久化**：确认 compose 的 `./data:/app/data` 挂载存在。
- **权限**：非 root 用户 `appuser` 需要能写 `/app/data`；使用宿主机目录挂载时，如遇权限拒绝可 `chmod -R 777 data` 或改用 named volume。