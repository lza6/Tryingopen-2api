# ---- builder 阶段：编译 Rust 二进制 ----
FROM rust:1.85-bookworm AS builder
WORKDIR /build

# 无 C 编译依赖（纯 Rust 依赖）；reqwest 用 rustls-tls，不装系统 openssl
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# 先拷贝清单，利用 Docker 层缓存（Cargo.lock 必须一起拷贝）
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin tryingopen2api

# ---- runtime 阶段：只保留运行所需最小内容 ----
FROM debian:bookworm-slim
LABEL org.opencontainers.image.title="tryingopen2api" \
      org.opencontainers.image.description="TryingOpen 免费模型 OpenAI/Anthropic 兼容 API 网关（Rust/axum）" \
      org.opencontainers.image.licenses="MIT"

# HTTPS 出站（上游目录/请求）需要 CA 证书
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 非 root 运行
RUN useradd --system --create-home --uid 10001 appuser
USER appuser
WORKDIR /app

COPY --from=builder /build/target/release/tryingopen2api /app/tryingopen2api

# 数据（proxies.txt（会话内存态））与 config 均由 volume 挂载，镜像内不内置
VOLUME ["/app/data"]

EXPOSE 47831

# 默认使用挂载的 /app/config.json；也可用 -e CONFIG_PATH=/app/config.json 或覆盖 ENTRYPOINT
ENTRYPOINT ["/app/tryingopen2api"]
CMD ["--config", "/app/config.json"]