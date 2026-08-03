//! Core Feigenbaum fractal constants and derived mathematical helper functions.
//!
//! The first Feigenbaum constant (δ ≈ 4.6692016) describes the universal rate
//! of period-doubling bifurcations in chaotic dynamic systems.
//!
//! In the Claw system δ governs:
//! - Budget decay across nested sub-task depth levels
//! - Exponential-fractal backoff on rate-limit failures
//! - Model cascade tier allocation

/// First Feigenbaum constant δ (period-doubling bifurcation ratio).
pub const FEIGENBAUM_DELTA: f64 = 4.669_201_609_102_990_7;

/// Second Feigenbaum constant α (orbit scaling factor).
pub const FEIGENBAUM_ALPHA: f64 = 2.502_907_875_095_893;

/// Fraction of the root budget below which a node is considered atomic (1%).
pub const CHAOS_THRESHOLD: f64 = 0.01;

/// Maximum depth before forced chaos boundary termination.
pub const MAX_FRACTAL_DEPTH: usize = 4;

/// Calculate token budget available at *depth* levels below the root.
#[must_use]
pub fn level_budget(total: usize, depth: usize) -> usize {
    if depth == 0 {
        return total;
    }
    let scaled = (total as f64) / FEIGENBAUM_DELTA.powi(depth as i32);
    (scaled.floor() as usize).max(1)
}

/// Fraction of the root budget available at *depth*.
#[must_use]
pub fn level_fraction(depth: usize) -> f64 {
    1.0 / FEIGENBAUM_DELTA.powi(depth as i32)
}

/// Check if *depth* has crossed the chaos threshold (< 1% of total budget).
#[must_use]
pub fn is_atomic(total: usize, depth: usize) -> bool {
    let fraction = level_fraction(depth);
    fraction < CHAOS_THRESHOLD || (total > 0 && level_budget(total, depth) < (total as f64 * CHAOS_THRESHOLD) as usize)
}

/// Maximum depth limit for massive enterprise-scale tasks (100k+ tokens).
pub const MAX_ENTERPRISE_FRACTAL_DEPTH: usize = 10;

/// Token threshold above which enterprise depth scaling activates.
pub const ENTERPRISE_SCALE_THRESHOLD: usize = 10_000;

/// Dynamically scale maximum fractal depth for massive enterprise tasks.
#[must_use]
pub fn dynamic_max_depth(total_tokens: usize) -> usize {
    if total_tokens <= ENTERPRISE_SCALE_THRESHOLD {
        MAX_FRACTAL_DEPTH
    } else {
        let ratio = (total_tokens as f64) / (ENTERPRISE_SCALE_THRESHOLD as f64);
        if !ratio.is_finite() || ratio <= 0.0 {
            return MAX_FRACTAL_DEPTH;
        }
        let extra = (ratio.ln() / FEIGENBAUM_DELTA.ln()).floor().max(0.0) as usize;
        (MAX_FRACTAL_DEPTH + extra).min(MAX_ENTERPRISE_FRACTAL_DEPTH)
    }
}



/// Check if *depth* has crossed the dynamically computed chaos threshold.
#[must_use]
pub fn is_atomic_dynamic(total: usize, depth: usize) -> bool {
    depth >= dynamic_max_depth(total) || is_atomic(total, depth)
}


/// Calculate optimal number of child tasks at a given *depth*.
#[must_use]
pub fn optimal_children(depth: usize, cap: usize) -> usize {
    let raw = (FEIGENBAUM_DELTA / (depth as f64 + 1.0)).floor() as usize;
    raw.clamp(2, cap)
}

/// Ratio of this level's budget to its parent's.
#[must_use]
pub fn bifurcation_ratio(depth: usize) -> f64 {
    if depth == 0 {
        1.0
    } else {
        1.0 / FEIGENBAUM_DELTA
    }
}

/// Calculate the asymmetric weight factor for sibling *idx* among *total_siblings*
/// using the second Feigenbaum constant α.
#[must_use]
pub fn asymmetric_sibling_weight(sibling_idx: usize, total_siblings: usize) -> f64 {
    if total_siblings <= 1 {
        return 1.0;
    }
    let raw = 1.0 / FEIGENBAUM_ALPHA.powi(sibling_idx as i32);
    let sum: f64 = (0..total_siblings)
        .map(|i| 1.0 / FEIGENBAUM_ALPHA.powi(i as i32))
        .sum();
    raw / sum
}

/// Calculate asymmetric budget allocated to sibling *idx* given parent budget.
#[must_use]
pub fn asymmetric_sibling_budget(parent_budget: usize, sibling_idx: usize, total_siblings: usize) -> usize {
    let weight = asymmetric_sibling_weight(sibling_idx, total_siblings);
    let val = ((parent_budget as f64) * weight).floor() as usize;
    val.max(1)
}


/// Immutable budget descriptor for a single fractal node depth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractalBudget {
    pub total_tokens: usize,
    pub depth: usize,
}

impl FractalBudget {
    #[must_use]
    pub const fn new(total_tokens: usize, depth: usize) -> Self {
        Self { total_tokens, depth }
    }

    #[must_use]
    pub fn tokens(&self) -> usize {
        level_budget(self.total_tokens, self.depth)
    }

    #[must_use]
    pub fn fraction(&self) -> f64 {
        level_fraction(self.depth)
    }

    #[must_use]
    pub fn is_atomic(&self) -> bool {
        is_atomic(self.total_tokens, self.depth)
    }

    #[must_use]
    pub fn max_subtasks(&self, cap: usize) -> usize {
        optimal_children(self.depth, cap)
    }

    #[must_use]
    pub const fn descend(&self) -> Self {
        Self {
            total_tokens: self.total_tokens,
            depth: self.depth + 1,
        }
    }

    #[must_use]
    pub const fn ascend(&self) -> Self {
        Self {
            total_tokens: self.total_tokens,
            depth: if self.depth > 0 { self.depth - 1 } else { 0 },
        }
    }
}
