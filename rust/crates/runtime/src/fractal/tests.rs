//! Unit tests for the Rust fractal module.

use std::time::{Duration, Instant};

use super::compact::*;
use super::constants::*;
use super::model_cascade::*;
use super::rate_limiter::*;

/// Maximum absolute deviation the ±1/α jitter can add to a δ-scaled pause.
fn jitter_bound() -> Duration {
    Duration::from_secs_f64(1.0 / FEIGENBAUM_ALPHA)
}

fn within(value: Duration, expected: Duration) -> bool {
    let bound = jitter_bound();
    value <= expected + bound && expected <= value + bound
}

#[test]
fn test_feigenbaum_delta_constant() {
    assert!((FEIGENBAUM_DELTA - 4.669_201_6).abs() < 0.001);
}

#[test]
fn test_model_cascade_alias_table() {
    let cascade = default_cascade();
    assert_eq!(cascade.len(), 4);
    assert_eq!(
        cascade.iter().map(|t| t.tier).collect::<Vec<_>>(),
        vec![0, 1, 2, 3],
        "tier index is the cascade order the CLI alias resolver relies on"
    );
    for tier in &cascade {
        assert!(!tier.alias.is_empty() && !tier.model_id.is_empty());
        assert_eq!(
            cascade.iter().filter(|other| other.alias == tier.alias).count(),
            1,
            "alias {} must be unique",
            tier.alias
        );
    }
}

#[test]
fn test_fractal_sample_indices() {
    assert!(fractal_sample_indices(0).is_empty());
    assert_eq!(fractal_sample_indices(1), vec![0]);
    // δ-geometric thinning of 18 items: newest, then ~δ back.
    assert_eq!(fractal_sample_indices(18), vec![12, 17]);
    // Sample size grows logarithmically (log_δ).
    assert!(fractal_sample_indices(10_000).len() <= 6);
}

#[test]
fn test_fractal_rate_limiter_escalation() {
    let mut limiter = FractalRateLimiter::new(1.0, 4);
    assert_eq!(limiter.current_level, 0);

    limiter.on_failure();
    assert_eq!(limiter.current_level, 1);
    assert!((limiter.current_pause().as_secs_f64() - FEIGENBAUM_DELTA).abs() < 0.01);

    limiter.on_success();
    assert_eq!(limiter.current_level, 0);
}

#[test]
fn test_jitter_stays_within_alpha_bound() {
    let limiter = FractalRateLimiter::new(1.0, 4);
    let unperturbed = limiter.current_pause();
    for _ in 0..32 {
        let jittered = limiter.current_pause_with_jitter();
        assert!(jittered.as_secs_f64() > 0.0);
        assert!(
            within(jittered, unperturbed),
            "jittered pause {jittered:?} drifted beyond ±1/α of {unperturbed:?}"
        );
    }
}

#[test]
fn test_reserve_pressure_pause_is_zero_while_healthy() {
    let mut limiter = FractalRateLimiter::new(1.0, 4);
    assert_eq!(
        limiter.reserve_pressure_pause(Instant::now(), MAX_BREAKER_PAUSE),
        Duration::ZERO,
        "a closed breaker must never delay a request"
    );
}

#[test]
fn test_reserve_pressure_pause_escalates_then_caps() {
    let mut limiter = FractalRateLimiter::new(1.0, 4);
    let now = Instant::now();
    limiter.reserve_pressure_pause(now, MAX_BREAKER_PAUSE); // healthy: stamps the window

    limiter.on_failure(); // L1 → δ^1 ≈ 4.67 s
    let first = limiter.reserve_pressure_pause(now, MAX_BREAKER_PAUSE);
    assert!(
        within(first, limiter.current_pause()),
        "L1 pause should be δ^1 ± jitter, got {first:?}"
    );

    limiter.on_failure();
    limiter.on_failure(); // L3 → δ^3 ≈ 101.8 s, above the cap
    let capped = limiter.reserve_pressure_pause(now, MAX_BREAKER_PAUSE);
    assert_eq!(
        capped, MAX_BREAKER_PAUSE,
        "escalation must saturate at the cap instead of demanding minutes of sleep"
    );

    // Once the reserved window has fully elapsed, no extra delay is demanded.
    assert_eq!(
        limiter.reserve_pressure_pause(now + Duration::from_secs(120), MAX_BREAKER_PAUSE),
        Duration::ZERO,
        "waiting past the window must not compound the pause"
    );
}

#[test]
fn test_breaker_escalates_to_chaos_point_and_counts_trips() {
    let mut limiter = FractalRateLimiter::new(1.0, 4);
    for expected in 1..=4 {
        limiter.on_failure();
        assert_eq!(limiter.current_level, expected);
    }
    assert!(limiter.is_at_chaos_point());
    limiter.on_failure();
    assert_eq!(limiter.current_level, 4, "level must clamp at max");
    limiter.record_breaker_trip();
    assert_eq!(limiter.breaker_trips, 1);
    assert!(limiter.bifurcation_summary().contains("trips=1"));
}

#[test]
fn test_breaker_apply_decay_resets_after_quiet_period() {
    let mut limiter = FractalRateLimiter::new(1.0, 4);
    for _ in 0..3 {
        limiter.on_failure();
    }
    assert_eq!(limiter.current_level, 3);

    // Immediately after the last failure: no decay.
    limiter.apply_decay(Instant::now());
    assert_eq!(limiter.current_level, 3);

    // Simulate a full quiet period passing.
    let future = Instant::now() + BREAKER_DECAY_PERIOD;
    limiter.apply_decay(future);
    assert_eq!(limiter.current_level, 0);
    assert_eq!(limiter.consecutive_failures, 0);
}

#[test]
fn test_level_schedule_shows_capped_pauses() {
    let limiter = FractalRateLimiter::new(1.0, 4);
    let levels = limiter.all_levels();
    assert_eq!(levels.len(), 5);
    assert_eq!(levels[0].pause_secs, 1.0);
    assert!((levels[1].pause_secs - 4.7).abs() < 0.1);
    assert_eq!(
        levels[3].pause_secs, 30.0,
        "displayed schedule must match what the breaker actually demands"
    );
    assert!(limiter
        .bifurcation_summary()
        .contains("L4: pause=  30.0s 💥 SHED"));
}
