//! Universal Feigenbaum fractal system for Claw runtime.
//!
//! Live integrations: δ-geometric session-history sampling (`compact`),
//! δ-scaled API circuit breakers (`rate_limiter`, wired into
//! `api::providers`), the model cascade aliases (`model_cascade`), and the
//! cognitive bifurcation ladder (`bifurcation`, wired into the conversation
//! loop and the middleware chain).

pub mod bifurcation;
pub mod compact;
pub mod constants;
pub mod model_cascade;
pub mod rate_limiter;

#[cfg(test)]
mod tests;

pub use bifurcation::{
    BifurcationEngine, CascadeThresholds, Escape, OrbitSample, Refusal, Regime, StepRecord,
    CHAOS_HALT_STEPS, FIRST_BIFURCATION_PRESSURE, MAX_LEDGER_ENTRIES, MIN_ATTEMPTS_BEFORE_REFUSAL,
    NOVEL_STEP_PRESSURE, PRESSURE_GAP, REGRESSION_PRESSURE, REPEAT_STEP_PRESSURE,
    TOMBSTONE_MIN_OUTPUT_CHARS, TRANSIENT_CATEGORIES,
};
pub use compact::fractal_sample_indices;
pub use constants::{FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA};
pub use model_cascade::{default_cascade, ModelTier};
pub use rate_limiter::{FractalRateLimiter, LevelParams, BREAKER_DECAY_PERIOD, MAX_BREAKER_PAUSE};
