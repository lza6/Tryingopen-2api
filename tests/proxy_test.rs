//! 免费代理解析 + 代理池 单元测试（全部不联网：纯逻辑 / 伪测）

use tryingopen2api::free_proxy::{parse_geonode_json, parse_ipport_text, parse_ipport_text_scheme};
use tryingopen2api::proxy_pool::{
    add_free_dedupe_helper, capacity_calc, host_port_key, normalize_proxy_url,
};

#[test]
fn parse_ipport_ok() {
    let out = parse_ipport_text(
        "# comment\n1.2.3.4:8080\nsocks5://5.6.7.8:1080\nnot-a-proxy\n10.0.0.1:80\n",
    );
    assert_eq!(out.len(), 2, "public ip:port only, socks stripped: {out:?}");
    assert!(out.contains(&"http://1.2.3.4:8080".to_string()));
    assert!(out.contains(&"http://5.6.7.8:1080".to_string()));
}

#[test]
fn parse_ipport_scheme_preserved() {
    // socks5 源按 socks5:// 前缀解析，HTTP 源按 http://（协议信息保留供 reqwest 使用）
    let socks = parse_ipport_text_scheme("5.6.7.8:1080\n1.2.3.4:8080\n", "socks5://");
    assert_eq!(
        socks,
        vec![
            "socks5://5.6.7.8:1080".to_string(),
            "socks5://1.2.3.4:8080".to_string()
        ]
    );
    let http = parse_ipport_text_scheme("5.6.7.8:8080\n", "http://");
    assert_eq!(http, vec!["http://5.6.7.8:8080".to_string()]);
}

#[test]
fn parse_geonode_ok() {
    let text = r#"{"data":[{"ip":"8.8.8.8","port":"8080"},{"ip":"192.168.1.1","port":"80"}]}"#;
    let out = parse_geonode_json(text);
    assert_eq!(out, vec!["http://8.8.8.8:8080".to_string()]);
}

#[test]
fn host_port_dedupe_key() {
    // 不同 user:pass / scheme 同 host:port → 同一个 key
    assert_eq!(
        host_port_key("http://user:pass@1.2.3.4:8080"),
        host_port_key("socks5://1.2.3.4:8080")
    );
    assert_eq!(host_port_key("http://1.2.3.4:8080"), "1.2.3.4:8080");
    assert_eq!(host_port_key("socks5://1.2.3.4:1080"), "1.2.3.4:1080");
}

#[test]
fn normalize_url() {
    assert_eq!(normalize_proxy_url("1.2.3.4:8080"), "http://1.2.3.4:8080");
    assert_eq!(
        normalize_proxy_url("socks5://1.2.3.4:1080"),
        "socks5://1.2.3.4:1080"
    );
    assert_eq!(
        normalize_proxy_url("http://user:pw@1.2.3.4:5"),
        "http://user:pw@1.2.3.4:5"
    );
}

#[test]
fn capacity_calc_basic() {
    // capacity = 可用代理数 × hourly_per_ip − 当日已用
    assert_eq!(capacity_calc(10, 20, 5), (200u64, 5u64, 195u64));
    assert_eq!(capacity_calc(10, 20, 999), (200u64, 999u64, 0u64)); // 下限 0
    assert_eq!(capacity_calc(0, 20, 0), (0u64, 0u64, 0u64));
}

#[test]
fn dedupe_helper_merges_host_port() {
    let out = add_free_dedupe_helper(vec![
        "http://1.2.3.4:8080".into(),
        "http://user:pass@1.2.3.4:8080".into(),
        "socks5://1.2.3.4:8080".into(),
        "5.6.7.8:1080".into(),
    ]);
    assert_eq!(out.len(), 2);
    assert!(out.contains(&"http://1.2.3.4:8080".to_string()));
    assert!(out.contains(&"http://5.6.7.8:1080".to_string()));
}

#[test]
fn parse_ipport_rejects_invalid() {
    let out = parse_ipport_text(
        "not-a-proxy\n10.0.0.1:80\n172.16.0.1:80\n127.0.0.1:8080\ngarbage", // 私网/回环都拒
    );
    assert_eq!(out.len(), 0);
}

