//! Universal Feigenbaum fractal system for Claw runtime.

pub mod compact;
pub mod constants;
pub mod model_cascade;
pub mod rate_limiter;
pub mod task_graph;
pub mod telemetry;

#[cfg(test)]
mod tests;

pub use compact::fractal_compact_messages;
pub use constants::{
    asymmetric_sibling_budget, asymmetric_sibling_weight, bifurcation_ratio, is_atomic,
    level_budget, level_fraction, optimal_children, FractalBudget, CHAOS_THRESHOLD,
    FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA, MAX_FRACTAL_DEPTH,
};
pub use model_cascade::{
    default_cascade, select_model_for_budget, select_model_for_depth, ModelTier,
};
pub use rate_limiter::{FractalRateLimiter, LevelParams};
pub use task_graph::{FractalTaskGraph, FractalTaskNode, FractalTaskStatus};
pub use telemetry::BifurcationTelemetryReport;

