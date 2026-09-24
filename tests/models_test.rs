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

#[tokio::test]
async fn add_free_dedupes_by_host_port() {
    let p = ProxyPool::new();
    // 不同 user:pass / scheme 同 host:port → 只算一个
    let added = p
        .add_free(vec![
            "http://1.2.3.4:8080".into(),
            "http://user:pass@1.2.3.4:8080".into(),
            "socks5://1.2.3.4:8080".into(),
            "http://5.6.7.8:1080".into(),
        ])
        .await;
    assert_eq!(added, 2);
    assert_eq!(p.len().await, 2);
}

#[tokio::test]
async fn latency_sort_low_latency_wins() {
    let p = ProxyPool::new();
    // 未用过且 inflight=0 时按 latency 升序优先
    p.add_free_with_latency(vec![
        ("http://1.1.1.1:8080".into(), 500),
        ("http://2.2.2.2:8080".into(), 50),
        ("http://3.3.3.3:8080".into(), 0), // 未知延迟排最后
    ])
    .await;
    let first = p.acquire(None, 20, &[0, 15]).await.unwrap();
    assert_eq!(host_port_of(&first), "2.2.2.2:8080");
    let second = p.acquire(None, 20, &[0, 15]).await.unwrap();
    assert_eq!(host_port_of(&second), "1.1.1.1:8080");
    let third = p.acquire(None, 20, &[0, 15]).await.unwrap();
    assert_eq!(host_port_of(&third), "3.3.3.3:8080");
}

#[tokio::test]
async fn snapshot_capacity_fields() {
    let p = ProxyPool::new();
    p.add_free(vec![
        "http://1.2.3.4:8080".into(),
        "http://5.6.7.8:8080".into(),
        "http://9.9.9.9:8080".into(),
    ])
    .await;
    let snap = p.snapshot(20).await;
    // 3 可用 × 20 hourly = 60
    assert_eq!(snap["capacity"]["capacity_total"].as_u64(), Some(60));
    assert_eq!(snap["capacity"]["capacity_used"].as_u64(), Some(0));
    assert_eq!(snap["capacity"]["capacity_remaining"].as_u64(), Some(60));
    // 用一次后 used=1
    let _ = p.acquire(None, 20, &[0, 15]).await.unwrap();
    let snap2 = p.snapshot(20).await;
    assert_eq!(snap2["capacity"]["capacity_used"].as_u64(), Some(1));
    assert_eq!(snap2["capacity"]["capacity_remaining"].as_u64(), Some(59));
    // 被冷却的排除：available 不含冷却项
    assert!(snap2["capacity"]["capacity_remaining"].as_u64().unwrap() < 60);
}

#[tokio::test]
async fn concurrent_gate_limits_inflight() {
    let p = ProxyPool::new();
    p.set_limits(Some(2)).await; // 全局并发上限 2
    p.add_free(vec![
        "http://1.2.3.4:8080".into(),
        "http://5.6.7.8:8080".into(),
        "http://9.9.9.9:8080".into(),
    ])
    .await;
    let u1 = p.acquire(None, 20, &[0, 15]).await.unwrap();
    let u2 = p.acquire(None, 20, &[0, 15]).await.unwrap();
    assert_ne!(u1, u2, "two concurrent slots use different egress");
    let snap = p.snapshot(20).await;
    assert_eq!(snap["inflight"].as_u64(), Some(2), "inflight=2 while held");
    p.mark_success(&u1).await;
    p.mark_success(&u2).await;
    let snap2 = p.snapshot(20).await;
    assert_eq!(snap2["inflight"].as_u64(), Some(0), "released after mark");
}

#[tokio::test]
async fn demote_bad_lowers_health() {
    let p = ProxyPool::new();
    p.add_free(vec!["http://1.2.3.4:8080".into()]).await;
    let u = "http://1.2.3.4:8080";
    for _ in 0..3 {
        p.mark_failure(u, false, &[0, 15]).await;
    }
    let demoted = p.demote_bad(3, 5000).await;
    assert_eq!(demoted, 1);
    let snap = p.snapshot(20).await;
    assert!(
        snap["items"][0]["health_score"].as_f64().unwrap() < 1.0,
        "health should be lowered"
    );
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

fn host_port_of(url: &str) -> String {
    safe_host_port(url)
}
