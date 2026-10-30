//! Built-in model alias tiers resolved by the CLI when a user passes
//! `--model quick|stable|mega|reasoner`. The tier index is ordering only —
//! fallback selection stays explicit and user-driven, never automatic.
//!
//! The δ/α constants govern the breaker escalation and compaction sampling
//! elsewhere in this module; they deliberately do not choose models here,
//! since silently swapping a user's model would be surprising behaviour.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelTier {
    pub alias: String,
    pub model_id: String,
    pub tier: usize,
}

impl ModelTier {
    #[must_use]
    pub fn new(alias: impl Into<String>, model_id: impl Into<String>, tier: usize) -> Self {
        Self {
            alias: alias.into(),
            model_id: model_id.into(),
            tier,
        }
    }
}

/// Default built-in cascade.
#[must_use]
pub fn default_cascade() -> Vec<ModelTier> {
    vec![
        ModelTier::new("quick", "google/gemma-4-31b-it:free", 0),
        ModelTier::new("stable", "nvidia/nemotron-3-super-120b-a12b:free", 1),
        ModelTier::new("mega", "meta-llama/llama-3.3-70b-instruct:free", 2),
        ModelTier::new("reasoner", "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free", 3),
    ]
}
