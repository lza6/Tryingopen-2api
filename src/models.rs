//! 模型注册表：TryingOpen 静态目录（12 个开源模型）+ 上游首页 chunk 动态刷新
//!
//! 数据来源（2026-09-24 抓包 https://www.tryingopen.com HAR + 07cl9ce_x7idy.js）：
//! 全站完全匿名（无需 Cookie）；按「每 IP 每小时约 20 次」限流。
//! 上游请求 id 就是 `provider/model`（如 `qwen/qwen3.8-27b`）。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
pub const DEFAULT_MODEL: &str = "qwen/qwen3.8-27b";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMeta {
    /// 上游请求 id（provider/model）
    pub id: String,
    /// 展示名
    pub label: String,
    /// 家族（maker）
    pub family: String,
    /// 上下文窗口（"128k"/"1M" 原文）
    pub context: String,
    /// 上下文窗口数值（token 数）
    pub context_window: i64,
    /// 每 1M token 输入价（USD，站点 pricePerMTok）
    pub price_per_mtok: f64,
    /// 支持工具调用
    pub tools: bool,
    /// 支持图片输入
    pub vision: bool,
    /// 目录来源：dynamic（上游抓取）/ static（内置兜底；抓取失败时即 fallback 状态）
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "static".into()
}

/// 静态目录（与 07cl9ce_x7idy.js 快照一致；动态抓取失败时兜底）
pub fn catalog() -> Vec<ModelMeta> {
    let mut v = Vec::new();
    macro_rules! m {
        ($id:expr, $label:expr, $family:expr, $ctx:expr, $price:expr, $tools:expr, $vision:expr) => {
            v.push(ModelMeta {
                id: $id.into(),
                label: $label.into(),
                family: $family.into(),
                context: $ctx.into(),
                context_window: parse_ctx($ctx),
                price_per_mtok: $price,
                tools: $tools,
                vision: $vision,
                source: "static".into(),
            });
        };
    }
    m!(
        "qwen/qwen3.8-27b",
        "Qwen3.8 27B",
        "Alibaba",
        "262k",
        3.2,
        true,
        true
    );
    m!(
        "nvidia/nemotron-3.5-lightning",
        "Nemotron 3.5 Lightning",
        "NVIDIA",
        "262k",
        0.25,
        true,
        false
    );
    m!(
        "deepseek/deepseek-v4-flash-0731",
        "DeepSeek V4 Flash",
        "DeepSeek",
        "1M",
        0.18,
        true,
        false
    );
    m!(
        "deepseek/deepseek-v4-pro-0813",
        "DeepSeek V4 Pro",
        "DeepSeek",
        "1M",
        1.98,
        true,
        false
    );
    m!(
        "google/gemma-4-31b-it",
        "Gemma 4 31B",
        "Google",
        "256k",
        0.4,
        true,
        true
    );
    m!(
        "google/gemma-4-26b-a4b-it",
        "Gemma 4 26B",
        "Google",
        "256k",
        0.4,
        true,
        true
    );
    m!(
        "openai/gpt-oss-120b",
        "GPT-OSS 120B",
        "OpenAI",
        "128k",
        0.6,
        true,
        false
    );
    m!(
        "meta/muse-glimmer-30b",
        "Muse Glimmer 30B",
        "Meta",
        "131k",
        1.5,
        true,
        false
    );
    m!(
        "moonshotai/kimi-k3",
        "Kimi K3",
        "Moonshot AI",
        "1M",
        15.0,
        true,
        true
    );
    m!(
        "minimax/minimax-m3",
        "MiniMax M3",
        "MiniMax",
        "1M",
        1.2,
        true,
        true
    );
    m!(
        "thinkingmachines/inkling-small",
        "Inkling Small",
        "ThinkingMachines",
        "524k",
        1.2,
        true,
        true
    );
    m!(
        "z-ai/glm-5.2",
        "GLM 5.2",
        "Zhipu AI",
        "1M",
        1.54,
        true,
        false
    );
    v
}

pub fn parse_ctx(ctx: &str) -> i64 {
    let s = ctx.trim().to_ascii_lowercase().replace(' ', "");
    let (num, unit) = match s.chars().last() {
        Some('k') => (&s[..s.len() - 1], 1024i64),
        Some('m') => (&s[..s.len() - 1], 1024 * 1024),
        _ => (s.as_str(), 1),
    };
    num.parse::<f64>()
        .map(|n| (n * unit as f64) as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct ModelRegistry {
    inner: Arc<RwLock<Vec<ModelMeta>>>,
    /// 上游已移除/报 model-not-found 的模型（请求时自动走 fallback、/v1/models 隐藏）
    forced_offline: Arc<RwLock<HashSet<String>>>,
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(catalog())),
            forced_offline: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// 模型是否被强制下线（上游移除 / model-not-found）
    async fn is_offline(&self, id: &str) -> bool {
        self.forced_offline.read().await.contains(id)
    }

    /// 标记模型下线（上游 4xx model-not-found）
    pub async fn mark_offline(&self, id: &str) {
        self.forced_offline.write().await.insert(id.to_string());
    }

    /// 解除下线（目录刷新带回来等）
    pub async fn unmark_offline(&self, id: &str) {
        self.forced_offline.write().await.remove(id);
    }

    /// 当前离线 id 集合（测试/审计用）
    pub async fn offline_ids(&self) -> Vec<String> {
        self.forced_offline.read().await.iter().cloned().collect()
    }

    /// 目录来源标记：设置为 "fallback"（动态抓取失败时由调用方标记）
    pub async fn mark_static_fallback(&self) {
        let mut list = self.inner.write().await;
        for m in list.iter_mut() {
            if m.source == "static" {
                m.source = "fallback".into();
            }
        }
    }

    pub async fn all(&self) -> Vec<ModelMeta> {
        let offline = self.forced_offline.read().await;
        let list = self.inner.read().await;
        list.iter()
            .filter(|m| !offline.contains(&m.id))
            .cloned()
            .collect()
    }

    pub async fn meta(&self, id: &str) -> Option<ModelMeta> {
        if self.is_offline(id).await {
            return None;
        }
        let list = self.inner.read().await;
        list.iter().find(|m| m.id == id).cloned()
    }

    pub async fn has_model(&self, id: &str) -> bool {
        if self.is_offline(id).await {
            return false;
        }
        self.inner.read().await.iter().any(|m| m.id == id)
    }

    /// 归一化：带上 provider 前缀的完整 id 原样；裸 model 名优先匹配目录
    pub async fn normalize(&self, requested: &str) -> String {
        let r = requested.trim();
        if r.is_empty() {
            return DEFAULT_MODEL.to_string();
        }
        if self.has_model(r).await {
            return r.to_string();
        }
        // 裸名（如 deepseek-v4-flash-0731）→ 补 provider 前缀（跳过离线模型）
        let offline = self.forced_offline.read().await;
        let list = self.inner.read().await;
        let matched = list.iter().find_map(|m| {
            if offline.contains(&m.id) {
                return None;
            }
            if let Some(surface) = m.id.rsplit('/').next() {
                if surface == r {
                    return Some(m.id.clone());
                }
            }
            if m.label.eq_ignore_ascii_case(r) {
                return Some(m.id.clone());
            }
            None
        });
        matched.unwrap_or_else(|| r.to_string())
    }

    /// 降级链：请求模型不在目录/不可用时按 fallback 顺序取
    /// fallbacks 来自 config.fallback_models（可配置）；为空时用内置默认
    pub async fn resolve(&self, requested: &str, fallbacks: &[String]) -> String {
        let norm = self.normalize(requested).await;
        if self.has_model(&norm).await {
            return norm;
        }
        let list: Vec<String> = if fallbacks.is_empty() {
            vec![
                "deepseek/deepseek-v4-flash-0731".into(),
                "z-ai/glm-5.2".into(),
                "minimax/minimax-m3".into(),
            ]
        } else {
            fallbacks.to_vec()
        };
        for fb in list {
            if self.has_model(&fb).await {
                return fb;
            }
        }
        DEFAULT_MODEL.to_string()
    }

    /// 用上游首页/JS chunk 解析结果替换静态目录（抓不到则保留静态）
    pub async fn replace_from_parsed(&self, records: Vec<ModelMeta>) -> usize {
        if records.is_empty() {
            return self.inner.read().await.len();
        }
        let mut list = self.inner.write().await;
        *list = records;
        // 剪除 offline 标记中已不在新目录的 id（目录更新 = 上游最新状态）
        {
            let mut offline = self.forced_offline.write().await;
            offline.retain(|id| list.iter().any(|m| &m.id == id));
        }
        list.len()
    }
}

/// OpenAI /v1/models 形状（含能力 meta，供 UI 真实展示工具/视觉/上下文/价格）
#[derive(Serialize, Deserialize)]
pub struct OpenAIModelObject {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
    /// 展示名
    pub label: String,
    /// 上下文窗口数值（token 数）
    pub context_window: i64,
    /// 上下文原文（"128k"）
    pub context: String,
    /// 每 1M token 输入价（USD）
    pub price_per_mtok: f64,
    /// 支持工具调用
    pub tools: bool,
    /// 支持图片输入
    pub vision: bool,
}

/// Anthropic /v1/models 形状
#[derive(Serialize, Deserialize)]
pub struct AnthropicModelObject {
    pub id: String,
    pub name: String,
    pub created: i64,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub context_window: i64,
}

pub fn openai_models(list: &[ModelMeta]) -> Vec<OpenAIModelObject> {
    list.iter()
        .map(|m| OpenAIModelObject {
            id: m.id.clone(),
            object: "model".into(),
            created: 0,
            owned_by: m.family.clone(),
            label: m.label.clone(),
            context_window: m.context_window,
            context: m.context.clone(),
            price_per_mtok: m.price_per_mtok,
            tools: m.tools,
            vision: m.vision,
        })
        .collect()
}

pub fn anthropic_models(list: &[ModelMeta]) -> Vec<AnthropicModelObject> {
    list.iter()
        .map(|m| AnthropicModelObject {
            id: m.id.clone(),
            name: m.label.clone(),
            created: 0,
            input_modalities: if m.vision {
                vec!["text".into(), "image".into()]
            } else {
                vec!["text".into()]
            },
            output_modalities: vec!["text".into()],
            context_window: m.context_window,
        })
        .collect()
}
