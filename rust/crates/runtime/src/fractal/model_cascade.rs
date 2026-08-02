//! Automatic model tier selection driven by δ ≈ 4.669.

use serde::{Deserialize, Serialize};
use super::constants::FractalBudget;

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

/// Select a model tier for a task at *depth* considering rate-limiter pressure.
///
/// Inverted strategy:
/// - Root tasks (depth 0) → heaviest model (highest tier)
/// - Deep atomic tasks → lightest model (tier 0)
/// - Rate-limiter escalation → shifts selection toward lighter models
#[must_use]
pub fn select_model_for_depth(depth: usize, cascade: &[ModelTier], limiter_level: usize) -> ModelTier {
    if cascade.is_empty() {
        return ModelTier::new("quick", "default-fallback", 0);
    }
    let max_tier = cascade.len() - 1;
    let ideal_tier = max_tier.saturating_sub(depth.min(max_tier));
    let adjusted_tier = ideal_tier.saturating_sub(limiter_level);
    cascade[adjusted_tier].clone()
}

/// Select a model tier using a `FractalBudget`.
#[must_use]
pub fn select_model_for_budget(budget: FractalBudget, cascade: &[ModelTier], limiter_level: usize) -> ModelTier {
    select_model_for_depth(budget.depth, cascade, limiter_level)
}
