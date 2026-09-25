//! 会话绑定：下游线程/会话 key →（当前模型）。TryingOpen 上游 /api/open 无会话概念
//! （请求体自带完整 messages 历史），这里只做下游线程的模型粘滞与最后活动记录。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    pub model: String,
    pub created_at: String,
    pub last_active: String,
}

#[derive(Debug, Clone, Default)]
pub struct SessionMap {
    inner: Arc<RwLock<HashMap<String, SessionBinding>>>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, key: &str) -> Option<SessionBinding> {
        self.inner.read().await.get(key).cloned()
    }

    pub async fn insert(&self, key: &str, binding: SessionBinding) {
        let mut m = self.inner.write().await;
        m.insert(key.to_string(), binding);
        Self::sweep(&mut m);
    }

    /// 防无界增长：超过阈值清理最旧（按 last_active 字典序近似）
    /// 由 insert / ensure / touch 共同调用，保证主请求路径也触发清理
    fn sweep(m: &mut HashMap<String, SessionBinding>) {
        if m.len() > 5000 {
            let mut keys: Vec<(String, String)> = m
                .iter()
                .map(|(k, v)| (k.clone(), v.last_active.clone()))
                .collect();
            keys.sort_by(|a, b| a.1.cmp(&b.1));
            let remove_n = m.len() - 4000;
            for (k, _) in keys.into_iter().take(remove_n) {
                m.remove(&k);
            }
        }
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    /// 绑定（或复用）下游 key → 模型
    pub async fn ensure(&self, key: &str, model: &str) -> SessionBinding {
        if let Some(b) = self.get(key).await {
            if b.model == model {
                return b;
            }
        }
        let binding = SessionBinding {
            model: model.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_active: chrono::Utc::now().to_rfc3339(),
        };
        {
            let mut m = self.inner.write().await;
            m.insert(key.to_string(), binding.clone());
            Self::sweep(&mut m);
        }
        binding
    }

    pub async fn touch(&self, key: &str) {
        let mut map = self.inner.write().await;
        if let Some(b) = map.get_mut(key) {
            b.last_active = chrono::Utc::now().to_rfc3339();
        }
        Self::sweep(&mut map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(active: &str) -> SessionBinding {
        SessionBinding {
            model: "m".into(),
            created_at: active.into(),
            last_active: active.into(),
        }
    }

    #[tokio::test]
    async fn sweep_bounds_map_to_4000() {
        let m = SessionMap::new();
        for i in 0..5100u32 {
            m.insert(
                &format!("k-{i}"),
                binding(&format!("2026-09-26T00:{:02}:00Z", i % 60)),
            )
            .await;
        }
        assert!(
            m.len().await <= 5000,
            "sweep 后应有界 ≤5000, got {}",
            m.len().await
        );
    }

    #[tokio::test]
    async fn insert_touch_keeps_model() {
        let m = SessionMap::new();
        m.insert("t", binding("2026-09-26T00:00:00Z")).await;
        m.touch("t").await;
        let got = m.get("t").await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().model, "m");
        assert_eq!(m.len().await, 1);
    }
}
