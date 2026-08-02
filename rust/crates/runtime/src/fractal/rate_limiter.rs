//! Adaptive rate limiter governed by the Feigenbaum constant δ ≈ 4.669.

use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

use super::constants::{FEIGENBAUM_DELTA, MAX_FRACTAL_DEPTH};

/// Level parameters for diagnostic reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelParams {
    pub level: usize,
    pub pause_secs: f64,
    pub timeout_secs: f64,
    pub tpm: usize,
}

/// Adaptive rate limiter with fractal (δ-scaled) backoff.
#[derive(Debug, Clone)]
pub struct FractalRateLimiter {
    pub base_pause_secs: f64,
    pub base_timeout_secs: f64,
    pub base_tpm: usize,
    pub max_level: usize,

    pub current_level: usize,
    pub consecutive_failures: usize,
    pub total_requests: usize,
    pub total_failures: usize,
    last_request_time: Option<Instant>,
}

impl Default for FractalRateLimiter {
    fn default() -> Self {
        Self::new(1.0, 30.0, 80_000, MAX_FRACTAL_DEPTH)
    }
}

impl FractalRateLimiter {
    #[must_use]
    pub fn new(base_pause_secs: f64, base_timeout_secs: f64, base_tpm: usize, max_level: usize) -> Self {
        Self {
            base_pause_secs,
            base_timeout_secs,
            base_tpm,
            max_level,
            current_level: 0,
            consecutive_failures: 0,
            total_requests: 0,
            total_failures: 0,
            last_request_time: None,
        }
    }

    #[must_use]
    pub fn current_pause(&self) -> Duration {
        let factor = FEIGENBAUM_DELTA.powi(self.current_level as i32);
        Duration::from_secs_f64(self.base_pause_secs * factor)
    }

    /// Return pause duration scaled by δ, plus stochastic jitter ±(jitter_factor / α)
    /// to avoid thundering-herd collisions across concurrent workers.
    #[must_use]
    pub fn current_pause_with_jitter(&self, jitter_factor: f64) -> Duration {
        use super::constants::FEIGENBAUM_ALPHA;
        let base_secs = self.current_pause().as_secs_f64();
        let jitter = (jitter_factor.clamp(-1.0, 1.0)) / FEIGENBAUM_ALPHA;
        let final_secs = (base_secs + jitter).max(0.1);
        Duration::from_secs_f64(final_secs)
    }


    #[must_use]
    pub fn current_timeout(&self) -> Duration {
        let factor = FEIGENBAUM_DELTA.powi(self.current_level as i32);
        Duration::from_secs_f64(self.base_timeout_secs * factor)
    }

    #[must_use]
    pub fn current_tpm(&self) -> usize {
        let factor = FEIGENBAUM_DELTA.powi(self.current_level as i32);
        let val = (self.base_tpm as f64 / factor).floor() as usize;
        val.max(100)
    }

    #[must_use]
    pub fn is_at_chaos_point(&self) -> bool {
        self.current_level >= self.max_level
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
        if self.current_level < self.max_level {
            self.current_level += 1;
        }
    }

    pub fn reset(&mut self) {
        self.current_level = 0;
        self.consecutive_failures = 0;
    }

    pub fn wait_if_needed(&mut self) -> Duration {
        let now = Instant::now();
        let target_pause = self.current_pause();
        let waited = if let Some(last) = self.last_request_time {
            let elapsed = now.duration_since(last);
            if elapsed < target_pause {
                let needed = target_pause - elapsed;
                std::thread::sleep(needed);
                needed
            } else {
                Duration::ZERO
            }
        } else {
            Duration::ZERO
        };
        self.last_request_time = Some(Instant::now());
        waited
    }

    #[must_use]
    pub fn level_params(&self, level: usize) -> LevelParams {
        let factor = FEIGENBAUM_DELTA.powi(level as i32);
        let pause_secs = (self.base_pause_secs * factor * 10.0).round() / 10.0;
        let timeout_secs = (self.base_timeout_secs * factor * 10.0).round() / 10.0;
        let tpm = ((self.base_tpm as f64 / factor).floor() as usize).max(100);
        LevelParams {
            level,
            pause_secs,
            timeout_secs,
            tpm,
        }
    }

    #[must_use]
    pub fn all_levels(&self) -> Vec<LevelParams> {
        (0..=self.max_level).map(|lvl| self.level_params(lvl)).collect()
    }

    #[must_use]
    pub fn bifurcation_summary(&self) -> String {
        let mut lines = vec![
            format!("Fractal Rate Limiter (δ = {:.4})", FEIGENBAUM_DELTA),
            format!(
                "State: level={} failures={} total={} total_fail={}",
                self.current_level, self.consecutive_failures, self.total_requests, self.total_failures
            ),
            String::new(),
        ];
        for params in self.all_levels() {
            let marker = if params.level == self.current_level { " ← ACTIVE" } else { "" };
            let chaos = if params.level >= self.max_level { " 💥 CHAOS" } else { "" };
            lines.push(format!(
                "  L{}: pause={:>6.1}s  timeout={:>6.1}s  tpm={:>6}{}{}",
                params.level, params.pause_secs, params.timeout_secs, params.tpm, marker, chaos
            ));
        }
        lines.join("\n")
    }
}
