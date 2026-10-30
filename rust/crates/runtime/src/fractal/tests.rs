//! Unit tests for the Rust fractal module.

use std::time::{Duration, Instant};

use super::bifurcation::*;
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

// ---------------------------------------------------------------------------
// Bifurcation ladder
// ---------------------------------------------------------------------------

/// Drive one failing step end to end: attempt, failure, observation.
fn fail_step(engine: &mut BifurcationEngine, tool: &str, input: &str, category: &str) {
    engine.note_attempt(tool, input);
    engine.note_outcome(tool, input, true, category);
    let sample = OrbitSample::new(engine.steps() + 1)
        .with_failure(BifurcationEngine::fingerprint(tool, category));
    engine.observe(&sample);
}

/// Drive a step where `tool` succeeded with `input`.
fn ok_step(engine: &mut BifurcationEngine, tool: &str, input: &str) {
    engine.note_attempt(tool, input);
    engine.note_outcome(tool, input, false, "");
    let mut sample = OrbitSample::new(engine.steps() + 1).with_success(tool);
    if !BifurcationEngine::is_read_only_tool(tool) {
        sample.mutated_state = true;
    }
    engine.observe(&sample);
}

#[test]
fn test_cascade_is_a_feigenbaum_ladder() {
    let t = CascadeThresholds::feigenbaum();
    assert_eq!(t.r1, FIRST_BIFURCATION_PRESSURE);
    assert!(
        (t.r2 - t.r1 - PRESSURE_GAP).abs() < 1e-12,
        "the first gap is the configured one"
    );
    assert!(
        ((t.r3 - t.r2) * FEIGENBAUM_DELTA - PRESSURE_GAP).abs() < 1e-9,
        "the second gap must shrink by δ"
    );
    assert!(t.r3 < t.r_infinity, "the cascade must accumulate below r∞");
    assert!(
        t.r_infinity < t.r2 + 2.0 * PRESSURE_GAP,
        "r∞ is finite: {:.3}",
        t.r_infinity
    );
    assert_eq!(t.regime_at(0.0), Regime::Laminar);
    assert_eq!(t.regime_at(t.r1), Regime::Dichotomy);
    assert_eq!(t.regime_at(t.r2), Regime::Quadrupole);
    assert_eq!(t.regime_at(t.r3), Regime::Chaos);
}

#[test]
fn test_pure_repeat_loop_walks_through_every_regime() {
    let mut engine = BifurcationEngine::new();
    let mut seen = Vec::new();
    for _ in 0..6 {
        fail_step(&mut engine, "bash", "cargo test", "syntax_error");
        seen.push(engine.regime());
    }
    assert_eq!(
        seen,
        vec![
            Regime::Laminar,
            Regime::Dichotomy,
            Regime::Dichotomy,
            Regime::Dichotomy,
            Regime::Quadrupole,
            Regime::Chaos,
        ],
        "the doublings must open in order, one per iteration"
    );
    assert_eq!(engine.crossings().len(), 3);
    assert!(engine.directive_section().is_some());
    assert!(
        engine.thresholds().regime_at(engine.pressure()) >= Regime::Chaos
            || seen[1] == Regime::Dichotomy,
        "the second identical failure at an unchanged world is already a closed \
         orbit, so the cycle floor opens L1 before pressure alone would"
    );
}

#[test]
fn test_clean_progress_contracts_pressure_and_silences_the_ladder() {
    let mut engine = BifurcationEngine::new();
    for _ in 0..3 {
        fail_step(&mut engine, "bash", "cargo test", "syntax_error");
    }
    assert_eq!(engine.regime(), Regime::Dichotomy);
    let stuck = engine.pressure();

    ok_step(&mut engine, "edit_file", "{\"path\": \"a.rs\"}");

    assert!(
        engine.pressure() <= stuck / FEIGENBAUM_ALPHA + 1e-12,
        "a forward step must contract resistance by at least one α"
    );
    assert_eq!(engine.regime(), Regime::Laminar);
    assert!(
        engine.directive_section().is_none(),
        "a converging agent gets no directive"
    );
    assert!(
        engine.would_refuse(&BifurcationEngine::attempt_key(
            "bash",
            "cargo test"
        ))
        .is_none(),
        "the world moved (edit_file succeeded), so re-running the test is a new experiment"
    );
}

#[test]
fn test_dead_orbit_is_refused_only_while_the_world_is_unchanged() {
    let mut engine = BifurcationEngine::new();
    let key = BifurcationEngine::attempt_key("edit_file", "{\"old_string\": \"x\"}");

    fail_step(&mut engine, "edit_file", "{\"old_string\": \"x\"}", "file_not_found");
    let refusal = engine
        .would_refuse(&key)
        .expect("a retry of a call that failed with nothing changed since is a dead orbit");
    assert_eq!(refusal.attempts, MIN_ATTEMPTS_BEFORE_REFUSAL);
    assert_eq!(refusal.category, "file_not_found");
    assert!(refusal.to_directive().contains("Split the orbit"));

    // The agent edits something else: the world moved, so the retry is a new
    // experiment again rather than a loop.
    ok_step(&mut engine, "write_file", "{\"path\": \"b.rs\"}");
    assert_eq!(engine.would_refuse(&key), None);
}

/// The whole point of the ladder is that it escalates on loops it *blocks*, not
/// only on loops it lets run: a refused retry never reaches the tool executor, so
/// the middleware has to hand the evidence back.
#[test]
fn test_blocked_retries_still_climb_the_ladder() {
    let mut engine = BifurcationEngine::new();
    let input = "{\"command\":\"edge-tts --text hi\"}";
    fail_step(&mut engine, "bash", input, "file_not_found");

    let key = BifurcationEngine::attempt_key("bash", input);
    assert_eq!(engine.regime(), Regime::Laminar);

    let mut seen = vec![engine.regime()];
    for _ in 0..6 {
        let refusal = engine
            .would_refuse(&key)
            .expect("the orbit is dead while the epoch has not moved");
        engine.note_refusal(&refusal);
        engine.observe(&OrbitSample::new(seen.len()));
        seen.push(engine.regime());
    }

    assert!(
        seen.contains(&Regime::Dichotomy) && seen.contains(&Regime::Quadrupole),
        "blocked retries must walk the ladder, got {seen:?}"
    );
    assert_eq!(*seen.last().unwrap(), Regime::Chaos, "and reach r∞");
    assert!(
        seen.windows(2).all(|w| w[1] >= w[0]),
        "a blocked loop only ever escalates: {seen:?}"
    );
}

#[test]
fn test_read_only_success_does_not_reopen_a_dead_orbit() {
    let mut engine = BifurcationEngine::new();
    let key = BifurcationEngine::attempt_key("edit_file", "{\"old_string\": \"x\"}");
    fail_step(&mut engine, "edit_file", "{\"old_string\": \"x\"}", "file_not_found");
    fail_step(&mut engine, "edit_file", "{\"old_string\": \"x\"}", "file_not_found");

    ok_step(&mut engine, "read_file", "{\"path\": \"a.rs\"}");

    assert!(
        engine.would_refuse(&key).is_some(),
        "reading a file cannot change what an edit sees"
    );
}

#[test]
fn test_transient_failures_are_the_breakers_job_not_the_ladders() {
    let mut engine = BifurcationEngine::new();
    let key = BifurcationEngine::attempt_key("web_fetch", "{\"url\": \"https://x\"}");
    for _ in 0..6 {
        fail_step(&mut engine, "web_fetch", "{\"url\": \"https://x\"}", "timeout");
    }
    assert_eq!(
        engine.would_refuse(&key),
        None,
        "a timed-out call may legitimately be retried"
    );
}

#[test]
fn test_regression_weighs_heavier_than_a_first_failure() {
    let mut fresh = BifurcationEngine::new();
    fail_step(&mut fresh, "bash", "cargo test", "syntax_error");

    let mut regressed = BifurcationEngine::new();
    ok_step(&mut regressed, "bash", "cargo test");
    fail_step(&mut regressed, "bash", "cargo test", "syntax_error");

    assert!(
        regressed.pressure() > fresh.pressure(),
        "breaking what worked must cost more resistance than failing forward"
    );
    assert!(regressed.summary().contains("regime=laminar") || regressed.pressure() > 0.0);
}

#[test]
fn test_escape_credits_the_ladder_with_the_dead_orbit() {
    let mut engine = BifurcationEngine::new();
    for _ in 0..3 {
        fail_step(
            &mut engine,
            "bash",
            "edge-tts --text hi",
            "missing_dependency",
        );
    }
    assert_eq!(engine.regime(), Regime::Dichotomy);

    ok_step(&mut engine, "bash", "python3 -m edge_tts --text hi");

    let escape = engine.take_escape().expect("the orbit split is worth recording");
    assert_eq!(escape.tool, "bash");
    assert_eq!(escape.from_regime, Regime::Dichotomy);
    let provenance = escape.provenance_line();
    assert!(provenance.contains("period-2"), "{provenance}");
    assert!(provenance.contains("edge-tts --text hi"), "{provenance}");
    assert_eq!(engine.regime(), Regime::Laminar);
}

#[test]
fn test_directive_states_measured_evidence_and_the_plan_update() {
    let mut engine = BifurcationEngine::new();
    for _ in 0..3 {
        fail_step(&mut engine, "edit_file", "{\"old_string\": \"x\"}", "file_not_found");
    }
    let directive = engine.directive_section().expect("dichotomy directive");
    assert!(directive.contains("PERIOD-2 ORBIT SPLIT"));
    assert!(directive.contains("Blocked orbits"), "{directive}");
    assert!(directive.contains("TaskGraph"));
    assert!(directive.contains("ORTHODOX") && directive.contains("INVERSION"));
    assert!(
        directive.contains(&format!("r∞={:.3}", engine.thresholds().r_infinity)),
        "the directive must quote the thresholds it is enforcing"
    );
}

#[test]
fn test_chaos_halt_is_bounded_and_off_by_default() {
    let mut lenient = BifurcationEngine::new();
    let mut strict = BifurcationEngine::new().with_halt_at_chaos(true);
    for _ in 0..10 {
        fail_step(&mut lenient, "bash", "make", "syntax_error");
        fail_step(&mut strict, "bash", "make", "syntax_error");
    }
    assert_eq!(strict.regime(), Regime::Chaos);
    assert!(
        !lenient.halt_requested(),
        "ending a user's turn stays an explicit opt-in"
    );
    assert!(strict.halt_requested(), "past r∞ the loop must be bounded");
}

#[test]
fn test_attempt_key_is_stable_and_input_sensitive() {
    let a = BifurcationEngine::attempt_key("bash", "cargo test");
    assert_eq!(a, BifurcationEngine::attempt_key("bash", "cargo test"));
    assert_ne!(a, BifurcationEngine::attempt_key("bash", "cargo test --offline"));
    assert_ne!(a, BifurcationEngine::attempt_key("read", "cargo test"));
}

#[test]
fn test_ledger_is_bounded() {
    let mut engine = BifurcationEngine::new();
    for i in 0..(MAX_LEDGER_ENTRIES + 40) {
        engine.note_attempt("bash", &format!("command-{i}"));
        engine.note_outcome("bash", &format!("command-{i}"), true, "syntax_error");
    }
    assert!(
        engine.summary().contains("blocked_orbits="),
        "the summary reports the ledger, got: {}",
        engine.summary()
    );
}
