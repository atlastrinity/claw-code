//! Core Feigenbaum fractal constants.
//!
//! The first Feigenbaum constant (δ ≈ 4.6692016) describes the universal rate
//! of period-doubling bifurcations in chaotic dynamic systems.
//!
//! In the Claw system δ governs:
//! - Exponential-fractal escalation of the API circuit breakers
//!   (`fractal::rate_limiter`, wired into `api::providers`)
//! - Geometric thinning of removed session history (`fractal::compact`,
//!   wired into `runtime::compact`)
//! - Model cascade tier fallback (`fractal::model_cascade`, wired into the
//!   CLI alias resolver)

/// First Feigenbaum constant δ (period-doubling bifurcation ratio).
pub const FEIGENBAUM_DELTA: f64 = 4.669_201_609_102_991;

/// Second Feigenbaum constant α (orbit scaling factor).
pub const FEIGENBAUM_ALPHA: f64 = 2.502_907_875_095_893;
