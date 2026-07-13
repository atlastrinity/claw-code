//! Unified API key rotation module for all providers.
//!
//! Supports two key sources that are merged into a single pool:
//!
//! 1. **Comma-separated keys** in a single env var:
//!    `GEMINI_API_KEY="key1,key2,key3"`
//!
//! 2. **Numbered env vars** (legacy fallback):
//!    `GEMINI_API_KEY="key1"`, `GEMINI_API_KEY2="key2"`, `GEMINI_API_KEY3="key3"`
//!
//! The pool is iterated round-robin via an atomic counter so concurrent
//! requests spread across all available keys.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::providers::dotenv_value;

/// A pool of API keys with atomic round-robin rotation.
#[derive(Debug)]
pub struct KeyPool {
    keys: Vec<String>,
    index: AtomicUsize,
}

impl KeyPool {
    /// Create a new pool from a list of keys.
    /// Panics if `keys` is empty — callers must validate beforehand.
    #[must_use]
    pub fn new(keys: Vec<String>) -> Self {
        assert!(!keys.is_empty(), "KeyPool requires at least one key");
        Self {
            keys,
            index: AtomicUsize::new(0),
        }
    }

    /// Return the next key using atomic round-robin.
    #[must_use]
    pub fn next_key(&self) -> &str {
        let idx = self.index.fetch_add(1, Ordering::Relaxed) % self.keys.len();
        &self.keys[idx]
    }

    /// Return a specific key by zero-based index. Returns `None` if out of bounds.
    #[must_use]
    pub fn key_at(&self, index: usize) -> Option<&str> {
        self.keys.get(index).map(String::as_str)
    }

    /// Total number of keys in the pool.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Return all keys as a slice.
    #[must_use]
    pub fn keys(&self) -> &[String] {
        &self.keys
    }
}

/// Read a single env var value, falling back to `.env` file.
fn read_env_or_dotenv(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) | Err(std::env::VarError::NotPresent) => dotenv_value(key),
        Err(_) => None,
    }
}

/// Parse all available API keys for a given env var name.
///
/// Resolution order:
/// 1. Read `env_var` — if the value contains commas, split and collect all
///    non-empty trimmed segments.
/// 2. Then scan numbered variants (`{env_var}2`, `{env_var}3`, …) up to
///    index 20 and append any found keys.
/// 3. Deduplicate while preserving order.
#[must_use]
pub fn parse_keys(env_var: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();

    // Step 1: primary env var (possibly comma-separated)
    if let Some(value) = read_env_or_dotenv(env_var) {
        if value.contains(',') {
            for segment in value.split(',') {
                let trimmed = segment.trim().to_string();
                if !trimmed.is_empty() {
                    keys.push(trimmed);
                }
            }
        } else {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                keys.push(trimmed);
            }
        }
    }

    // Step 2: numbered env vars ({ENV_VAR}2 .. {ENV_VAR}20)
    for i in 2..=20 {
        let numbered_key = format!("{env_var}{i}");
        if let Some(value) = read_env_or_dotenv(&numbered_key) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() && !keys.contains(&trimmed) {
                keys.push(trimmed);
            }
        }
    }

    keys
}

/// Build a `KeyPool` from the given env var, or return `None` if no keys
/// are found.
#[must_use]
pub fn pool_for_env(env_var: &str) -> Option<KeyPool> {
    let keys = parse_keys(env_var);
    if keys.is_empty() {
        None
    } else {
        Some(KeyPool::new(keys))
    }
}

/// Check whether there are multiple keys available for a given env var.
#[must_use]
pub fn has_multiple_keys(env_var: &str) -> bool {
    parse_keys(env_var).len() > 1
}

/// Check whether a key exists at a given 1-based index for the specified env var.
/// Index 1 is the first key in the pool.
#[must_use]
pub fn has_key_at_index(env_var: &str, one_based_index: usize) -> bool {
    if one_based_index == 0 {
        return false;
    }
    let keys = parse_keys(env_var);
    one_based_index <= keys.len()
}

/// Get the key at a given 1-based index for the specified env var.
/// Index 1 is the first key in the pool.
#[must_use]
pub fn key_at_index(env_var: &str, one_based_index: usize) -> Option<String> {
    if one_based_index == 0 {
        return None;
    }
    let keys = parse_keys(env_var);
    keys.into_iter().nth(one_based_index - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keys_splits_comma_separated_values() {
        std::env::set_var("TEST_KR_COMMA", "key1, key2 ,key3");
        let keys = parse_keys("TEST_KR_COMMA");
        assert_eq!(keys, vec!["key1", "key2", "key3"]);
        std::env::remove_var("TEST_KR_COMMA");
    }

    #[test]
    fn parse_keys_single_key() {
        std::env::set_var("TEST_KR_SINGLE", "only-one-key");
        let keys = parse_keys("TEST_KR_SINGLE");
        assert_eq!(keys, vec!["only-one-key"]);
        std::env::remove_var("TEST_KR_SINGLE");
    }

    #[test]
    fn parse_keys_with_numbered_vars() {
        std::env::set_var("TEST_KR_NUM", "primary");
        std::env::set_var("TEST_KR_NUM2", "second");
        std::env::set_var("TEST_KR_NUM3", "third");
        let keys = parse_keys("TEST_KR_NUM");
        assert_eq!(keys, vec!["primary", "second", "third"]);
        std::env::remove_var("TEST_KR_NUM");
        std::env::remove_var("TEST_KR_NUM2");
        std::env::remove_var("TEST_KR_NUM3");
    }

    #[test]
    fn parse_keys_comma_plus_numbered_deduplicates() {
        std::env::set_var("TEST_KR_DEDUP", "key1,key2");
        std::env::set_var("TEST_KR_DEDUP2", "key2"); // duplicate
        std::env::set_var("TEST_KR_DEDUP3", "key3"); // new
        let keys = parse_keys("TEST_KR_DEDUP");
        assert_eq!(keys, vec!["key1", "key2", "key3"]);
        std::env::remove_var("TEST_KR_DEDUP");
        std::env::remove_var("TEST_KR_DEDUP2");
        std::env::remove_var("TEST_KR_DEDUP3");
    }

    #[test]
    fn parse_keys_empty_env_returns_empty() {
        std::env::remove_var("TEST_KR_MISSING");
        let keys = parse_keys("TEST_KR_MISSING");
        assert!(keys.is_empty());
    }

    #[test]
    fn key_pool_round_robin() {
        let pool = KeyPool::new(vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(pool.next_key(), "a");
        assert_eq!(pool.next_key(), "b");
        assert_eq!(pool.next_key(), "c");
        assert_eq!(pool.next_key(), "a"); // wraps around
    }

    #[test]
    fn has_key_at_index_works() {
        std::env::set_var("TEST_KR_IDX", "k1,k2,k3");
        assert!(has_key_at_index("TEST_KR_IDX", 1));
        assert!(has_key_at_index("TEST_KR_IDX", 2));
        assert!(has_key_at_index("TEST_KR_IDX", 3));
        assert!(!has_key_at_index("TEST_KR_IDX", 4));
        assert!(!has_key_at_index("TEST_KR_IDX", 0));
        std::env::remove_var("TEST_KR_IDX");
    }

    #[test]
    fn key_at_index_works() {
        std::env::set_var("TEST_KR_AT", "first,second");
        assert_eq!(key_at_index("TEST_KR_AT", 1), Some("first".to_string()));
        assert_eq!(key_at_index("TEST_KR_AT", 2), Some("second".to_string()));
        assert_eq!(key_at_index("TEST_KR_AT", 3), None);
        std::env::remove_var("TEST_KR_AT");
    }
}
