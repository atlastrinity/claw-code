//! Adaptive circuit breaker governed by the Feigenbaum constant δ ≈ 4.669.
//!
//! Used as a per-client breaker in the API providers: consecutive exhausted
//! retry chains escalate the level (pause scales by δ^L), successes
//! de-escalate, and `is_at_chaos_point()` sheds load instead of hammering a
//! failing provider. `reserve_pressure_pause()` is the non-blocking API meant
//! for async callers; the pause is capped so a degraded provider delays
//! requests without locking the session out, and `apply_decay()` clears the
//! episode once the provider has been quiet long enough.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::constants::{FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA};

/// Default pause after the last failure after which the breaker cools back
/// down to level 0 (half-open → closed).
pub const BREAKER_DECAY_PERIOD: Duration = Duration::from_secs(60);

/// Ceiling on the δ-scaled inter-chain pause: escalation is capped rather than
/// allowed to reach δ^L seconds, because a waiting CLI is indistinguishable
/// from a hung one.
pub const MAX_BREAKER_PAUSE: Duration = Duration::from_secs(30);

/// Level parameters for diagnostic reporting.
#[derive(Debug, Clone, Serialize)]
pub struct LevelParams {
    pub level: usize,
    pub pause_secs: f64,
}

/// Adaptive breaker with fractal (δ-scaled) escalation.
#[derive(Debug, Clone)]
pub struct FractalRateLimiter {
    pub base_pause_secs: f64,
    pub max_level: usize,

    pub current_level: usize,
    pub consecutive_failures: usize,
    pub total_requests: usize,
    pub total_failures: usize,
    /// Times the breaker opened (`is_at_chaos_point()` fast-fail decisions).
    pub breaker_trips: usize,
    last_request_time: Option<Instant>,
    last_failure_time: Option<Instant>,
    decay_period: Duration,
}

impl FractalRateLimiter {
    #[must_use]
    pub fn new(base_pause_secs: f64, max_level: usize) -> Self {
        Self {
            base_pause_secs,
            max_level,
            current_level: 0,
            consecutive_failures: 0,
            total_requests: 0,
            total_failures: 0,
            breaker_trips: 0,
            last_request_time: None,
            last_failure_time: None,
            decay_period: BREAKER_DECAY_PERIOD,
        }
    }

    #[must_use]
    pub fn current_pause(&self) -> Duration {
        let factor = FEIGENBAUM_DELTA.powi(self.current_level as i32);
        Duration::from_secs_f64(self.base_pause_secs * factor)
    }

    /// δ-scaled pause plus stochastic ±1/α jitter, so concurrent workers and
    /// separate sessions do not re-collide on the same wall-clock instant.
    #[must_use]
    pub fn current_pause_with_jitter(&self) -> Duration {
        let base_secs = self.current_pause().as_secs_f64();
        let unit = 1.0 / FEIGENBAUM_ALPHA;
        let jitter = (next_jitter_sample() * 2.0 - 1.0) * unit;
        Duration::from_secs_f64((base_secs + jitter).max(0.1))
    }

    /// Minimum spacing the breaker demands between request chains.
    ///
    /// The returned duration is how long the caller must wait before starting
    /// the next chain; the method also records the *planned* start, so clones
    /// of the same client queue against each other instead of all observing an
    /// already-elapsed window and firing together. The result never exceeds
    /// `cap`, and a closed (level 0) breaker never delays.
    pub fn reserve_pressure_pause(&mut self, now: Instant, cap: Duration) -> Duration {
        if self.current_level == 0 {
            self.last_request_time = Some(now);
            return Duration::ZERO;
        }
        let target = self.current_pause_with_jitter().min(cap);
        let wait = self
            .last_request_time
            .map(|planned| planned + target)
            .unwrap_or(now)
            .checked_duration_since(now)
            .unwrap_or(Duration::ZERO)
            .min(cap);
        self.last_request_time = Some(now + wait);
        wait
    }

    /// Resize the escalation ceiling, keeping the current level inside the new
    /// range. Called when a client's retry policy changes so "shed after N
    /// failed chains" tracks the retry budget instead of a stale default.
    pub fn set_max_level(&mut self, max_level: usize) {
        self.max_level = max_level.max(1);
        if self.current_level > self.max_level {
            self.current_level = self.max_level;
        }
    }

    #[must_use]
    pub fn is_at_chaos_point(&self) -> bool {
        self.current_level >= self.max_level
    }

    /// Record a breaker trip (a request shed while at the chaos point).
    pub fn record_breaker_trip(&mut self) {
        self.breaker_trips += 1;
    }

    /// Cooling decay: if no failure has been recorded for a full
    /// [`BREAKER_DECAY_PERIOD`], the failing episode is over and the breaker
    /// returns to level 0.
    pub fn apply_decay(&mut self, now: Instant) {
        if self.current_level == 0 {
            return;
        }
        if let Some(last_failure) = self.last_failure_time {
            if now.duration_since(last_failure) >= self.decay_period {
                self.current_level = 0;
                self.consecutive_failures = 0;
            }
        }
    }

    pub fn on_success(&mut self) {
        self.total_requests += 1;
        self.consecutive_failures = 0;
        if self.current_level > 0 {
            self.current_level -= 1;
        }
    }

    pub fn on_failure(&mut self) {
        self.total_requests += 1;
        self.total_failures += 1;
        self.consecutive_failures += 1;
        self.last_failure_time = Some(Instant::now());
        if self.current_level < self.max_level {
            self.current_level += 1;
        }
    }

    /// The pause this level will actually demand (δ^L, capped at
    /// [`MAX_BREAKER_PAUSE`]) — what the breaker really does, not the raw
    /// geometric ramp.
    #[must_use]
    pub fn level_params(&self, level: usize) -> LevelParams {
        let factor = FEIGENBAUM_DELTA.powi(level as i32);
        let pause_secs = (self.base_pause_secs * factor).min(cap_pause_secs());
        LevelParams {
            level,
            pause_secs: (pause_secs * 10.0).round() / 10.0,
        }
    }

    #[must_use]
    pub fn all_levels(&self) -> Vec<LevelParams> {
        (0..=self.max_level)
            .map(|lvl| self.level_params(lvl))
            .collect()
    }

    #[must_use]
    pub fn bifurcation_summary(&self) -> String {
        let mut lines = vec![
            format!("Fractal circuit breaker (δ = {:.4})", FEIGENBAUM_DELTA),
            format!(
                "State: level={} failures={} total={} total_fail={} trips={}",
                self.current_level,
                self.consecutive_failures,
                self.total_requests,
                self.total_failures,
                self.breaker_trips
            ),
            String::new(),
        ];
        for params in self.all_levels() {
            let marker = if params.level == self.current_level {
                " ← ACTIVE"
            } else {
                ""
            };
            let chaos = if params.level >= self.max_level {
                " 💥 SHED"
            } else {
                ""
            };
            lines.push(format!(
                "  L{}: pause={:>6.1}s{}{}",
                params.level, params.pause_secs, marker, chaos
            ));
        }
        lines.push(format!(
            "  pause cap = {:.0}s · cooling decay = {:.0}s",
            cap_pause_secs(),
            self.decay_period.as_secs_f64()
        ));
        lines.join("\n")
    }
}

fn cap_pause_secs() -> f64 {
    MAX_BREAKER_PAUSE.as_secs_f64()
}

/// Splitmix64-derived sample in `[0, 1)`, decorrelated across callers by a
/// process-wide counter so consecutive calls never return the same value even
/// when the clock is coarser than the jitter step.
fn next_jitter_sample() -> f64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = nanos
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    state ^= state >> 30;
    state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    state ^= state >> 27;
    state = state.wrapping_mul(0x94D0_49BB_1331_11EB);
    state ^= state >> 31;
    (state >> 11) as f64 / (1u64 << 53) as f64
}
