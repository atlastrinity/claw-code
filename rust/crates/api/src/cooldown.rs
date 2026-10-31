use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

/// Default cooldown duration when an API key hits rate limit or server overload.
pub const DEFAULT_COOLDOWN_DURATION: Duration = Duration::from_secs(60);

/// Thread-safe global key cooldown tracker for all LLM providers.
pub static GLOBAL_KEY_COOLDOWN: LazyLock<KeyCooldownTracker> =
    LazyLock::new(KeyCooldownTracker::new);

#[derive(Debug, Clone)]
pub struct KeyCooldownTracker {
    cooldowns: Arc<RwLock<HashMap<(String, usize), Instant>>>,
    /// When false, `mark_cooldown` keeps state in memory only and never touches
    /// the shared on-disk file. Unit tests build trackers via `new_empty`, so
    /// they must not write into the real `~/.claw/cooldowns.json` at runtime —
    /// otherwise test fixtures (e.g. a `glm-4.7-flash-test` model key) leak into
    /// a live session's cooldown state.
    persist: bool,
}

impl Default for KeyCooldownTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyCooldownTracker {
    pub fn new() -> Self {
        let tracker = Self {
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
            persist: true,
        };
        tracker.load_from_disk();
        tracker
    }

    pub fn new_empty() -> Self {
        Self {
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
            persist: false,
        }
    }

    fn get_persistence_path() -> Option<std::path::PathBuf> {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
        Some(home.join(".claw").join("cooldowns.json"))
    }

    fn save_to_disk(&self) {
        if !self.persist {
            return;
        }
        let Some(path) = Self::get_persistence_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let now = Instant::now();
        let map = self.cooldowns.read().unwrap_or_else(|e| e.into_inner());
        let entries: Vec<(String, usize, u64)> = map
            .iter()
            .filter_map(|((model, key_idx), &until)| {
                if until > now {
                    Some((model.clone(), *key_idx, (until - now).as_secs()))
                } else {
                    None
                }
            })
            .collect();
        if let Ok(json) = serde_json::to_string(&entries) {
            let _ = std::fs::write(path, json);
        }
    }

    fn load_from_disk(&self) {
        let Some(path) = Self::get_persistence_path() else {
            return;
        };
        if !path.exists() {
            return;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(entries) = serde_json::from_str::<Vec<(String, usize, u64)>>(&content) {
                let now = Instant::now();
                let mut map = self.cooldowns.write().unwrap_or_else(|e| e.into_inner());
                for (model, key_idx, remaining_secs) in entries {
                    if remaining_secs > 0 {
                        map.insert((model, key_idx), now + Duration::from_secs(remaining_secs));
                    }
                }
            }
        }
    }

    /// Cooldown length for a key that hit a throttle, honoring the server's
    /// `Retry-After` when present. A 429 may carry a concrete "back off N
    /// seconds" hint; using a blind fixed duration either parks the key far
    /// too long (when the server only wanted a few seconds) or far too short
    /// (when it asked for minutes and the next rotation just re-hits the limit).
    /// Clamped to a sane band so a huge or zero hint can neither idle the pool
    /// for an hour nor cause a hot rotate loop.
    pub fn cooldown_duration(retry_after: Option<Duration>) -> Duration {
        const FLOOR: Duration = Duration::from_secs(5);
        const CAP: Duration = Duration::from_secs(120);
        match retry_after {
            Some(after) => after.clamp(FLOOR, CAP),
            None => DEFAULT_COOLDOWN_DURATION,
        }
    }

    /// Mark a key for a model as cooling down for a given duration.
    pub fn mark_cooldown(&self, model: &str, key_index: usize, duration: Duration) {
        let until = Instant::now() + duration;
        {
            let mut map = self.cooldowns.write().unwrap_or_else(|e| e.into_inner());
            map.insert((model.to_string(), key_index), until);
        }
        self.save_to_disk();
        eprintln!(
            "❄️ API key index {} for model '{}' marked in {}s cooldown",
            key_index,
            model,
            duration.as_secs()
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

    /// Check if an error indicates a rate-limit, overload, quota, or timeout issue that warrants cooldown.
    pub fn should_trigger_cooldown(error_text: &str, status_code: u16) -> bool {
        if status_code == 429 || status_code == 503 || status_code == 504 || status_code == 408 {
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
            || lower.contains("timeout")
            || lower.contains("stalled")
            || lower.contains("idle")
            || lower.contains("decoding response body")
            || lower.contains("error decoding")
            || lower.contains("connection reset")
            || lower.contains("broken pipe")
            || lower.contains("http error")
            || lower.contains("network error")
    }

    /// True when the failure belongs to the endpoint or the network path rather
    /// than to the specific API key: the request never produced an HTTP
    /// response (`status_code == 0`) and the message names a transport condition
    /// (connect/send/decode/timeout/DNS). Every key in a rotation pool points at
    /// the same host, so such an error says nothing about any individual key —
    /// cooling one and rotating to its siblings only poisons a healthy pool.
    /// Callers must skip cooldown+rotation for these and fall through to the
    /// provider-wide backoff ladder instead.
    pub fn is_endpoint_or_network_error(error_text: &str, status_code: u16) -> bool {
        // A status code means the server answered; that is a real per-request
        // signal (rate limit, overload, gateway) that rotation is meant for.
        if status_code != 0 {
            return false;
        }
        let lower = error_text.to_lowercase();
        lower.contains("error sending request")
            || lower.contains("error send request")
            || lower.contains("error decoding response body")
            || lower.contains("error decoding")
            || lower.contains("connection")
            || lower.contains("broken pipe")
            || lower.contains("connect")
            || lower.contains("timed out")
            || lower.contains("timeout")
            || lower.contains("dns")
            || lower.contains("resolve")
            || lower.contains("network")
            || lower.contains("tcp")
            || lower.contains("unable to connect")
            || lower.contains("request error")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_cooldown_lifecycle() {
        let tracker = KeyCooldownTracker::new_empty();
        assert!(!tracker.is_in_cooldown("glm-4.7-flash-test", 1));

        tracker.mark_cooldown("glm-4.7-flash-test", 1, Duration::from_secs(10));
        assert!(tracker.is_in_cooldown("glm-4.7-flash-test", 1));
        assert!(!tracker.is_in_cooldown("glm-4.7-flash-test", 2));

        let (best, wait) = tracker.find_available_key("glm-4.7-flash-test", 2, 1);
        assert_eq!(best, 2);
        assert!(wait.is_none());
    }

    #[test]
    fn test_all_keys_in_cooldown_returns_min_wait() {
        let tracker = KeyCooldownTracker::new_empty();
        tracker.mark_cooldown("glm-4.7-flash-test", 1, Duration::from_secs(30));
        tracker.mark_cooldown("glm-4.7-flash-test", 2, Duration::from_secs(10));

        let (best, wait) = tracker.find_available_key("glm-4.7-flash-test", 2, 1);
        assert_eq!(best, 2);
        assert!(wait.is_some());
        assert!(wait.unwrap().as_secs() <= 10);
    }

    #[test]
    fn test_should_trigger_cooldown_detects_network_and_decoding_errors() {
        assert!(KeyCooldownTracker::should_trigger_cooldown(
            "http error: error decoding response body",
            0
        ));
        assert!(KeyCooldownTracker::should_trigger_cooldown(
            "connection reset by peer",
            0
        ));
        assert!(KeyCooldownTracker::should_trigger_cooldown(
            "rate limit exceeded",
            429
        ));
    }

    #[test]
    fn transport_errors_are_endpoint_scoped_not_key_scoped() {
        // The exact failure from a live z.ai blip: no HTTP status, send error.
        assert!(KeyCooldownTracker::is_endpoint_or_network_error(
            "api failed after 3 attempts: http error: error sending request for url \
             (https://api.z.ai/api/paas/v4/chat/completions)",
            0,
        ));
        assert!(KeyCooldownTracker::is_endpoint_or_network_error(
            "connection reset by peer",
            0
        ));
        assert!(KeyCooldownTracker::is_endpoint_or_network_error(
            "error decoding response body",
            0
        ));
    }

    #[test]
    fn cooldown_duration_honors_retry_after_within_band() {
        // Server hint inside the band is used verbatim.
        assert_eq!(
            KeyCooldownTracker::cooldown_duration(Some(Duration::from_secs(20))),
            Duration::from_secs(20)
        );
        // Too-small hint is floored, so rotation cannot hot-loop.
        assert_eq!(
            KeyCooldownTracker::cooldown_duration(Some(Duration::from_secs(1))),
            Duration::from_secs(5)
        );
        // Absurdly large hint is capped, so one response cannot idle a key for
        // an hour.
        assert_eq!(
            KeyCooldownTracker::cooldown_duration(Some(Duration::from_secs(3_600))),
            Duration::from_secs(120)
        );
        // No hint keeps the historical default.
        assert_eq!(
            KeyCooldownTracker::cooldown_duration(None),
            DEFAULT_COOLDOWN_DURATION
        );
    }
}
