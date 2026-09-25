//! TryingOpen2API — TryingOpen 免费模型 OpenAI/Anthropic 兼容 API 网关（Rust axum 版）
//!
//! 上游：https://www.tryingopen.com（完全匿名，单 IP 每 24h UTC 日约 20 次限流）
//! 协议逆向来源：抓包数据包（源代码、网络数据包/www.tryingopen.com.har）+ 站点 JS chunk。
//!
//! 支持端点：
//! - POST /v1/chat/completions   OpenAI 聊天（流式/非流式）
//! - POST /v1/messages           Claude 聊天（双向转换）
//! - GET  /v1/models             模型列表（静态目录 + 动态同步）
//! - GET  /healthz               健康检查
//! - /api/*                      代理池/面板/指南
//! - /ui                         内嵌控制面板

pub mod api;
pub mod config;
pub mod errors;
pub mod free_proxy;
pub mod models;
pub mod prod_guard;
pub mod protocol;
pub mod proxy_pool;
pub mod session;
pub mod upstream;
pub mod web;

pub const APP_NAME: &str = "tryingopen2api";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
