//! Universal Feigenbaum fractal system for Claw runtime.
//!
//! Live integrations: δ-geometric session-history sampling (`compact`),
//! δ-scaled API circuit breakers (`rate_limiter`, wired into
//! `api::providers`), and the model cascade aliases (`model_cascade`).

pub mod compact;
pub mod constants;
pub mod model_cascade;
pub mod rate_limiter;

#[cfg(test)]
mod tests;

pub use compact::fractal_sample_indices;
pub use constants::{FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA};
pub use model_cascade::{default_cascade, ModelTier};
pub use rate_limiter::{
    FractalRateLimiter, LevelParams, BREAKER_DECAY_PERIOD, MAX_BREAKER_PAUSE,
};
