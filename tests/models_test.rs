//! 模型注册表 + 代理池 单元测试

use std::sync::Arc;

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
    // 解析兜底（传入可配置 fallbacks）
    assert_eq!(
        r.resolve("nope/nope", &[]).await,
        "deepseek/deepseek-v4-flash-0731"
    );
    // 自定义 fallback 生效
    let custom = vec!["z-ai/glm-5.2".to_string()];
    assert_eq!(r.resolve("nope/nope", &custom).await, "z-ai/glm-5.2");
}

#[tokio::test]
async fn replace_from_parsed_preserves_static_fallback_meta() {
    // 动态目录整体替换静态目录时，若上游记录不含 message_limit / cheaper_fallback，
    // 必须保留静态兜底元数据（kimi messageLimit=5 / cheaper=minimax/mimimax-m3），
    // 否则 docs/PROTOCOL.md「静态目录仍保留 kimi messageLimit=5/cheaper 兜底元数据」失守。
    let r = ModelRegistry::new();

    // 静态兜底存在：kimi 有 message_limit=Some(5)、cheaper_fallback=Some(minimax/mimimax-m3)
    let static_kimi = r.meta("moonshotai/kimi-k3").await.unwrap();
    assert_eq!(static_kimi.message_limit, Some(5));
    assert!(static_kimi.cheaper_fallback.is_some());

    // 上游动态记录：只带基础能力字段，不含 messageLimit/cheaperFallbackId（2026-09-26 实测）
    let dynamic = vec![
        crate_meta("qwen/qwen3.8-27b", 262 * 1024, None, None),
        crate_meta("moonshotai/kimi-k3", 1024 * 1024, None, None),
    ];
    let replaced = r.replace_from_parsed(dynamic).await;
    assert_eq!(replaced, 2);

    let kimi = r.meta("moonshotai/kimi-k3").await.unwrap();
    assert_eq!(
        kimi.message_limit,
        Some(5),
        "动态替换不得清空静态 message_limit 兜底"
    );
    assert!(
        kimi.cheaper_fallback.is_some(),
        "动态替换不得清空静态 cheaper_fallback 兜底"
    );

    // 上游若显式提供新值，则用新值覆盖（优先级：动态 > 静态兜底）
    let dynamic_override = vec![crate_meta(
        "moonshotai/kimi-k3",
        1024 * 1024,
        Some(3),
        Some("minimax/minimax-m3".to_string()),
    )];
    r.replace_from_parsed(dynamic_override).await;
    let kimi2 = r.meta("moonshotai/kimi-k3").await.unwrap();
    assert_eq!(kimi2.message_limit, Some(3), "动态显式值应覆盖静态兜底");
}

#[tokio::test]
async fn replace_from_parsed_empty_records_keeps_static() {
    // 空记录（上游抓取失败）→ 短路返回现状长度，保留静态目录不被清空
    let r = ModelRegistry::new();
    let before = r.all().await.len();
    let n = r.replace_from_parsed(vec![]).await;
    assert_eq!(n, before, "空记录应返回现有目录长度");
    let after = r.all().await.len();
    assert_eq!(after, before, "空记录不得清空静态目录");
    // 且静态兜底仍可查
    assert!(r.meta("moonshotai/kimi-k3").await.is_some());
}

fn crate_meta(
    id: &str,
    context_window: i64,
    limit: Option<u32>,
    cheaper: Option<String>,
) -> tryingopen2api::models::ModelMeta {
    tryingopen2api::models::ModelMeta {
        id: id.into(),
        label: id.into(),
        family: "Test".into(),
        context: String::new(),
        context_window,
        price_per_mtok: 0.0,
        tools: true,
        vision: true,
        reasoning: false,
        message_limit: limit,
        cheaper_fallback: cheaper,
        source: "dynamic".into(),
    }
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

#[test]
fn catalog_new_capability_fields() {
    let c = catalog();
    let kimi = c
        .iter()
        .find(|m| m.id == "moonshotai/kimi-k3")
        .expect("kimi-k3");
    // 上游 messageLimit / cheaperFallbackId 透传
    assert_eq!(kimi.message_limit, Some(5));
    assert_eq!(kimi.cheaper_fallback.as_deref(), Some("minimax/minimax-m3"));
    // 推理能力默认标记（reasoning 模型）
    let qwen = c.iter().find(|m| m.id == "qwen/qwen3.8-27b").expect("qwen");
    assert!(qwen.reasoning);
    // OpenAI 形状带新字段
    let objs = tryingopen2api::models::openai_models(&c);
    let km = objs
        .iter()
        .find(|m| m.id == "moonshotai/kimi-k3")
        .expect("kimi obj");
    assert_eq!(km.message_limit, Some(5));
    assert_eq!(km.cheaper_fallback.as_deref(), Some("minimax/minimax-m3"));
    assert!(km.reasoning);
}

#[test]
fn upstream_chunk_parses_capability_fields() {
    let chunk = r#"{id:"moonshotai/kimi-k3",name:"Kimi K3",maker:"Moonshot AI",context:"1M",pricePerMTok:15.0,supportsTools:!0,supportsImages:!0,supportsReasoning:!0,messageLimit:5,cheaperFallbackId:"minimax/minimax-m3"}"#;
    let parsed = tryingopen2api::upstream::parse_catalog_chunk(chunk);
    eprintln!("PARSED {:?}", parsed);
    assert_eq!(parsed.len(), 1);
    let m = &parsed[0];
    assert_eq!(m.id, "moonshotai/kimi-k3");
    assert_eq!(m.message_limit, Some(5));
    assert_eq!(m.cheaper_fallback.as_deref(), Some("minimax/minimax-m3"));
    assert!(m.reasoning);
}

#[tokio::test]
async fn replace_from_parsed_refresh_restores_offline_model() {
    // H2 自动恢复：目录刷新成功（非空 records）= 上游最新状态，
    // 「重新出现在新目录且刷新成功」的模型应自动 unmark 下线，恢复可查。
    let r = ModelRegistry::new();
    let id = "moonshotai/kimi-k3";

    // 模拟上游 model-not-found：标记下线 → /v1/models 隐藏、meta 不可查
    r.mark_offline(id).await;
    assert!(
        r.offline_ids().await.iter().any(|x| x == id),
        "mark_offline 后应处于离线集合"
    );
    assert!(r.meta(id).await.is_none(), "离线模型 meta 应不可查");

    // 目录刷新成功：非空 records 且含该模型
    let records = vec![
        crate_meta(id, 1024 * 1024, None, None),
        crate_meta("qwen/qwen3.8-27b", 262 * 1024, None, None),
    ];
    r.replace_from_parsed(records).await;

    // 自动恢复：离线标记清除、meta 可查到（is_offline false 的可见结果）
    assert!(
        !r.offline_ids().await.iter().any(|x| x == id),
        "刷新成功后仍在目录的模型应自动解除下线"
    );
    let m = r.meta(id).await;
    assert!(m.is_some(), "自动恢复后 meta 应可查询");
}

#[tokio::test]
async fn replace_from_parsed_keeps_offline_for_removed_models() {
    // H2 保留语义：目录刷新成功后，仅清除「仍在新目录」的离线标记；
    // 「仍不在新目录」的 id 必须保留 offline（真·被上游移除）。
    let r = ModelRegistry::new();
    let removed = "google/gemma-4-31b-it";
    let kept = "moonshotai/kimi-k3";

    r.mark_offline(removed).await;
    r.mark_offline(kept).await;

    // 刷新：records 只含 kept，不含 removed
    let records = vec![
        crate_meta(kept, 1024 * 1024, None, None),
        crate_meta("qwen/qwen3.8-27b", 262 * 1024, None, None),
    ];
    r.replace_from_parsed(records).await;

    assert!(
        r.offline_ids().await.iter().any(|x| x == removed),
        "仍不在新目录的模型应保留 offline 标记"
    );
    assert!(
        !r.offline_ids().await.iter().any(|x| x == kept),
        "出现在新目录的模型应解除 offline 标记"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replace_from_parsed_no_abba_deadlock() {
    // H1 锁序契约：全仓统一 forced_offline → inner，任何时刻最多持一个锁。
    // 若 replace_from_parsed 嵌套持锁（inner → forced_offline），与 all()/normalize()
    // （forced_offline → inner）形成 AB-BA 等待环 → tokio 多线程调度下死锁挂起，
    // 本压测在 15s 内无法收敛即 panic。
    let r = Arc::new(ModelRegistry::new());
    let mut handles = Vec::new();
    for i in 0..8 {
        let r = r.clone();
        handles.push(tokio::spawn(async move {
            for round in 0..200 {
                if (i + round) % 2 == 0 {
                    // 目录刷新路径：写 inner → 写 forced_offline（修复前嵌套）
                    let records = vec![crate_meta("qwen/qwen3.8-27b", 262 * 1024, None, None)];
                    let _ = r.replace_from_parsed(records).await;
                } else {
                    // 请求路径：读 forced_offline → 读 inner
                    let _ = r.all().await;
                    let _ = r.normalize("deepseek-v4-flash-0731").await;
                }
            }
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        for h in handles {
            h.await.unwrap();
        }
    })
    .await
    .expect("AB-BA 死锁：all/normalize 与 replace_from_parsed 交替应在 15s 内完成");
}
