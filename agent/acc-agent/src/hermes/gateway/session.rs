use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::super::conversation::ConversationHistory;

/// Maximum messages retained per session before oldest turns are pruned.
const MAX_HISTORY_MESSAGES: usize = 60;

pub struct SessionStore {
    base_dir: PathBuf,
    cache: Arc<Mutex<HashMap<String, Vec<Value>>>>,
}

impl SessionStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        std::fs::create_dir_all(&base_dir).ok();
        Self { base_dir, cache: Arc::new(Mutex::new(HashMap::new())) }
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        let safe: String = key.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
        self.base_dir.join(format!("{safe}.json"))
    }

    pub async fn load_history(&self, key: &str) -> ConversationHistory {
        let mut cache = self.cache.lock().await;
        if let Some(msgs) = cache.get(key) {
            return ConversationHistory::from_turns(msgs);
        }
        let path = self.key_to_path(key);
        let messages: Vec<Value> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        cache.insert(key.to_string(), messages.clone());
        ConversationHistory::from_turns(&messages)
    }

    pub async fn save_history(&self, key: &str, history: &ConversationHistory) {
        let mut messages = history.messages.clone();
        // Keep only the most recent messages to bound disk and context size.
        if messages.len() > MAX_HISTORY_MESSAGES {
            messages = messages.split_off(messages.len() - MAX_HISTORY_MESSAGES);
        }
        let path = self.key_to_path(key);
        {
            let mut cache = self.cache.lock().await;
            cache.insert(key.to_string(), messages.clone());
        }
        if let Ok(json) = serde_json::to_string_pretty(&messages) {
            let tmp = path.with_extension("tmp");
            let _ = std::fs::write(&tmp, &json);
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    pub async fn clear(&self, key: &str) {
        let path = self.key_to_path(key);
        self.cache.lock().await.remove(key);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn round_trips_history() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let key = "test:session:1";

        let mut h = store.load_history(key).await;
        assert!(h.messages.is_empty());

        h.push_user_text("hello");
        store.save_history(key, &h).await;

        let h2 = store.load_history(key).await;
        assert_eq!(h2.messages.len(), 1);
        assert_eq!(h2.messages[0]["role"], "user");
    }

    #[tokio::test]
    async fn clear_removes_session() {
        let dir = tempdir().unwrap();
        let store = SessionStore::new(dir.path());
        let key = "test:clear:1";

        let mut h = store.load_history(key).await;
        h.push_user_text("hi");
        store.save_history(key, &h).await;
        store.clear(key).await;

        let h2 = store.load_history(key).await;
        assert!(h2.messages.is_empty());
    }
}
