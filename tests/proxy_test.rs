//! 免费代理解析 单元测试

use tryingopen2api::free_proxy::{parse_geonode_json, parse_ipport_text};

#[test]
fn parse_ipport_ok() {
    let out = parse_ipport_text("# comment\n1.2.3.4:8080\nsocks5://5.6.7.8:1080\nnot-a-proxy\n10.0.0.1:80\n");
    assert_eq!(out.len(), 2, "public ip:port only, socks stripped: {out:?}");
    assert!(out.contains(&"http://1.2.3.4:8080".to_string()));
    assert!(out.contains(&"http://5.6.7.8:1080".to_string()));
}

#[test]
fn parse_geonode_ok() {
    let text = r#"{"data":[{"ip":"8.8.8.8","port":"8080"},{"ip":"192.168.1.1","port":"80"}]}"#;
    let out = parse_geonode_json(text);
    assert_eq!(out, vec!["http://8.8.8.8:8080".to_string()]);
}
