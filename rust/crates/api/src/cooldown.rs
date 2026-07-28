use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

/// Default cooldown duration when an API key hits rate limit or server overload.
pub const DEFAULT_COOLDOWN_DURATION: Duration = Duration::from_secs(60);

/// Thread-safe global key cooldown tracker for all LLM providers.
pub static GLOBAL_KEY_COOLDOWN: LazyLock<KeyCooldownTracker> = LazyLock::new(KeyCooldownTracker::new);

#[derive(Debug, Clone)]
pub struct KeyCooldownTracker {
    cooldowns: Arc<RwLock<HashMap<(String, usize), Instant>>>,
}

impl Default for KeyCooldownTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyCooldownTracker {
    pub fn new() -> Self {
        Self {
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Mark a key for a model as cooling down for a given duration.
    pub fn mark_cooldown(&self, model: &str, key_index: usize, duration: Duration) {
        let until = Instant::now() + duration;
        let mut map = self.cooldowns.write().unwrap_or_else(|e| e.into_inner());
        map.insert((model.to_string(), key_index), until);
        eprintln!(
            "❄️ API key index {} for model '{}' marked in {}s cooldown",
            key_index, model, duration.as_secs()
        );
    }

    /// Check if a key is currently in cooldown.
    pub fn is_in_cooldown(&self, model: &str, key_index: usize) -> bool {
        self.remaining_cooldown(model, key_index).is_some()
    }

    /// Return remaining cooldown duration for a key, if active.
    pub fn remaining_cooldown(&self, model: &str, key_index: usize) -> Option<Duration> {
        let map = self.cooldowns.read().unwrap_or_else(|e| e.into_inner());
        if let Some(&until) = map.get(&(model.to_string(), key_index)) {
            let now = Instant::now();
            if until > now {
                return Some(until - now);
            }
        }
        None
    }

    /// Find the best next key index that is NOT in cooldown.
    /// Returns `(selected_key_index, shortest_wait_if_all_cooling_down)`.
    pub fn find_available_key(
        &self,
        model: &str,
        total_keys: usize,
        start_index: usize,
    ) -> (usize, Option<Duration>) {
        if total_keys <= 1 {
            let remaining = self.remaining_cooldown(model, 1);
            return (1, remaining);
        }

        // 1. Try to find the first key starting from start_index that is not cooling down
        for i in 0..total_keys {
            let idx = ((start_index - 1 + i) % total_keys) + 1;
            if !self.is_in_cooldown(model, idx) {
                return (idx, None);
            }
        }

        // 2. If all keys are in cooldown, find the key with the shortest remaining cooldown
        let mut min_wait: Option<Duration> = None;
        let mut best_key = start_index;

        for k in 1..=total_keys {
            if let Some(remaining) = self.remaining_cooldown(model, k) {
                match min_wait {
                    None => {
                        min_wait = Some(remaining);
                        best_key = k;
                    }
                    Some(current_min) if remaining < current_min => {
                        min_wait = Some(remaining);
                        best_key = k;
                    }
                    _ => {}
                }
            }
        }

        (best_key, min_wait)
    }

    /// Check if an error indicates a rate-limit, overload, or quota issue that warrants cooldown.
    pub fn should_trigger_cooldown(error_text: &str, status_code: u16) -> bool {
        if status_code == 429 || status_code == 503 || status_code == 504 {
            return true;
        }

        let lower = error_text.to_lowercase();
        lower.contains("rate limit")
            || lower.contains("too many requests")
            || lower.contains("overloaded")
            || lower.contains("1305")
            || lower.contains("quota")
            || lower.contains("resource_exhausted")
            || lower.contains("capacity")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_cooldown_lifecycle() {
        let tracker = KeyCooldownTracker::new();
        assert!(!tracker.is_in_cooldown("glm-4.7-flash", 1));

        tracker.mark_cooldown("glm-4.7-flash", 1, Duration::from_secs(10));
        assert!(tracker.is_in_cooldown("glm-4.7-flash", 1));
        assert!(!tracker.is_in_cooldown("glm-4.7-flash", 2));

        let (best, wait) = tracker.find_available_key("glm-4.7-flash", 2, 1);
        assert_eq!(best, 2);
        assert!(wait.is_none());
    }

    #[test]
    fn test_all_keys_in_cooldown_returns_min_wait() {
        let tracker = KeyCooldownTracker::new();
        tracker.mark_cooldown("glm-4.7-flash", 1, Duration::from_secs(30));
        tracker.mark_cooldown("glm-4.7-flash", 2, Duration::from_secs(10));

        let (best, wait) = tracker.find_available_key("glm-4.7-flash", 2, 1);
        assert_eq!(best, 2);
        assert!(wait.is_some());
        assert!(wait.unwrap().as_secs() <= 10);
    }
}
