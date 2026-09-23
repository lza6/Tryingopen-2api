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
        self.inner.write().await.insert(key.to_string(), binding);
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
        self.inner
            .write()
            .await
            .insert(key.to_string(), binding.clone());
        binding
    }

    pub async fn touch(&self, key: &str) {
        let mut map = self.inner.write().await;
        if let Some(b) = map.get_mut(key) {
            b.last_active = chrono::Utc::now().to_rfc3339();
        }
    }
}
