//! 模型注册表 + 代理池 单元测试

use tryingopen2api::models::{catalog, parse_ctx, ModelRegistry};
use tryingopen2api::proxy_pool::{parse_cooldown_map, safe_host_port, ProxyPool};

#[test]
fn catalog_has_12_models() {
    let c = catalog();
    assert!(
        c.len() >= 12,
        "static catalog should have >= 12 models, got {}",
        c.len()
    );
    assert!(c.iter().any(|m| m.id == "qwen/qwen3.8-27b"));
    assert!(c.iter().any(|m| m.id == "moonshotai/kimi-k3"));
}

#[test]
fn context_parse() {
    assert_eq!(parse_ctx("128k"), 128 * 1024);
    assert_eq!(parse_ctx("1M"), 1024 * 1024);
    assert_eq!(parse_ctx("262k"), 262 * 1024);
    assert_eq!(parse_ctx(""), 0);
}

#[tokio::test]
async fn normalize_and_resolve() {
    let r = ModelRegistry::new();
    assert_eq!(r.normalize("qwen/qwen3.8-27b").await, "qwen/qwen3.8-27b");
    // 裸 surface → 补前缀
    let n = r.normalize("deepseek-v4-flash-0731").await;
    assert_eq!(n, "deepseek/deepseek-v4-flash-0731");
    // 未知 → 原样
    assert_eq!(r.normalize("nope/nope").await, "nope/nope");
    // 解析兜底
    assert_eq!(
        r.resolve("nope/nope").await,
        "deepseek/deepseek-v4-flash-0731"
    );
}

#[tokio::test]
async fn proxy_pool_cooldown_and_rotation() {
    let p = ProxyPool::new();
    assert_eq!(p.len().await, 0);
    assert_eq!(p.acquire(None, 20, &[0, 15]).await, None);

    p.add_free(vec![
        "http://1.2.3.4:8080".into(),
        "http://5.6.7.8:8080".into(),
    ])
    .await;
    assert_eq!(p.len().await, 2);
    let u1 = p.acquire(None, 20, &[0, 15]).await.unwrap();
    let u2 = p.acquire(None, 20, &[0, 15]).await.unwrap();
    assert_ne!(u1, u2, "two unused proxies should rotate");
    // 两个都用过后全冷却 → acquire 仍返回（权宜），但不报错
    let _u3 = p.acquire(None, 20, &[0, 15]).await;
    // 失败标记 + 冷却
    p.mark_failure(&u1, true, &[0, 15]).await;
    p.mark_success(&u2).await;
    let snap = p.snapshot(20).await;
    assert_eq!(snap["total"].as_i64(), Some(2));
}

#[test]
fn cooldown_map_parse() {
    assert_eq!(
        parse_cooldown_map("0,15,60,120,300"),
        vec![0, 15, 60, 120, 300]
    );
    assert_eq!(parse_cooldown_map(""), Vec::<u32>::new());
}

#[test]
fn host_port_masked() {
    assert_eq!(
        safe_host_port("http://user:pass@1.2.3.4:8080"),
        "1.2.3.4:8080"
    );
    assert_eq!(safe_host_port("socks5://1.2.3.4:1080"), "1.2.3.4:1080");
}
