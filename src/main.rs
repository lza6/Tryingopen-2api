//! TryingOpen2API 入口：加载配置 → 构建上游客户端/代理池/注册表 → 启动 axum

use std::sync::Arc;
use std::time::Duration;
use tryingopen2api::api::{build_router, AppState};
use tryingopen2api::config::Config;
use tryingopen2api::free_proxy;
use tryingopen2api::models::ModelRegistry;
use tryingopen2api::proxy_pool::ProxyPool;
use tryingopen2api::session::SessionMap;
use tryingopen2api::upstream::UpstreamClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,tryingopen2api=debug")
            }),
        )
        .with_target(false)
        .init();

    let config_path = Config::resolve_config_path();
    let cfg = Config::load(config_path.as_deref())?;
    let listen_addr = cfg.listen_addr.clone();
    let upstream = cfg.upstream_base_url.clone();
    tracing::info!("TryingOpen2API v{} 启动", env!("CARGO_PKG_VERSION"));
    tracing::info!("监听 {}", listen_addr);
    tracing::info!(
        "上游 {}（完全匿名，单 IP 每小时约 {} 次）",
        upstream,
        cfg.hourly_per_ip
    );

    let proxy_opt = None::<String>; // 首个客户端直连（健康检查/目录抓取用）
    let client = Arc::new(UpstreamClient::new(
        &upstream,
        proxy_opt.as_deref(),
        Duration::from_secs(cfg.request_timeout_sec),
    )?);

    if !cfg.skip_upstream_check {
        match client.check_health().await {
            Ok(_) => tracing::info!("上游健康检查通过"),
            Err(e) => tracing::warn!("上游健康检查失败（continue）: {e}"),
        }
    }

    let registry = Arc::new(ModelRegistry::new());
    {
        // 启动时尝试一次动态目录（失败保留静态 12 模型，不影响启动）
        match client.fetch_catalog().await {
            Ok(records) => {
                let n = registry.replace_from_parsed(records).await;
                tracing::info!("动态模型目录已加载 {n} 个");
            }
            Err(e) => tracing::warn!("动态目录抓取失败（保留静态目录）: {e}"),
        }
    }

    // 代理池
    let pool = Arc::new(ProxyPool::new());
    pool.set_limits(Some(cfg.max_concurrent_requests)).await;
    let n_res = pool.load_file(&cfg.proxy_file).await;
    if n_res > 0 {
        tracing::info!("代理池加载住宅代理 {n_res} 个");
    }
    {
        let pool2 = pool.clone();
        let enabled = cfg.free_proxy_enabled;
        let refresh_min = cfg.free_proxy_refresh_min;
        let (tx, rx) = tokio::sync::watch::channel(false);
        let _tx = tx;
        if enabled {
            let p = pool2.clone();
            let t = tokio::spawn(async move {
                free_proxy::run_loop(p, refresh_min, rx).await;
            });
            drop(t); // 显式 detach 后台任务（run_loop 随进程生命周期运行）
            tracing::info!("免费代理抓取后台已启动（每 {} 分钟刷新）", refresh_min);
        }
    }

    let sessions = Arc::new(SessionMap::new());
    let api_keys = Arc::new(std::sync::RwLock::new(cfg.api_keys.clone()));
    if cfg.api_keys.is_empty() {
        tracing::info!("未配置 api_keys：仅本机可访问（面板可一键生成）");
    }

    // 目录定时刷新任务
    {
        let registry2 = registry.clone();
        let client2 = client.clone();
        let minutes = cfg.catalog_refresh_min;
        if minutes > 0 {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(minutes * 60));
                interval.tick().await;
                loop {
                    interval.tick().await;
                    match client2.fetch_catalog().await {
                        Ok(records) => {
                            let n = registry2.replace_from_parsed(records).await;
                            tracing::info!("动态模型目录已刷新 {n} 个");
                        }
                        Err(e) => tracing::warn!("目录刷新失败: {e}"),
                    }
                }
            });
        }
    }

    let state = AppState {
        cfg: Arc::new(cfg),
        client,
        pool,
        registry,
        sessions,
        api_keys,
    };

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("HTTP 服务已启动: http://{listen_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
