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
            cascade
                .iter()
                .filter(|other| other.alias == tier.alias)
                .count(),
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
        engine
            .would_refuse(&BifurcationEngine::attempt_key("bash", "cargo test"))
            .is_none(),
        "the world moved (edit_file succeeded), so re-running the test is a new experiment"
    );
}

#[test]
fn test_dead_orbit_is_refused_only_while_the_world_is_unchanged() {
    let mut engine = BifurcationEngine::new();
    let key = BifurcationEngine::attempt_key("edit_file", "{\"old_string\": \"x\"}");

    fail_step(
        &mut engine,
        "edit_file",
        "{\"old_string\": \"x\"}",
        "file_not_found",
    );
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
    fail_step(
        &mut engine,
        "edit_file",
        "{\"old_string\": \"x\"}",
        "file_not_found",
    );
    fail_step(
        &mut engine,
        "edit_file",
        "{\"old_string\": \"x\"}",
        "file_not_found",
    );

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
        fail_step(
            &mut engine,
            "web_fetch",
            "{\"url\": \"https://x\"}",
            "timeout",
        );
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

    let escape = engine
        .take_escape()
        .expect("the orbit split is worth recording");
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
        fail_step(
            &mut engine,
            "edit_file",
            "{\"old_string\": \"x\"}",
            "file_not_found",
        );
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
    assert_ne!(
        a,
        BifurcationEngine::attempt_key("bash", "cargo test --offline")
    );
    assert_ne!(a, BifurcationEngine::attempt_key("read", "cargo test"));
}

#[test]
fn test_attempt_key_ignores_the_caption_and_the_key_order() {
    let first = BifurcationEngine::attempt_key(
        "bash",
        "{\"command\":\"ls -la\",\"description\":\"List files\"}",
    );
    let reworded = BifurcationEngine::attempt_key(
        "bash",
        "{\"description\":\"Now show me everything\",\"command\":\"ls -la\"}",
    );
    assert_eq!(
        first, reworded,
        "a caption is not a new attempt at the world"
    );
    assert_ne!(
        first,
        BifurcationEngine::attempt_key(
            "bash",
            "{\"command\":\"ls -l\",\"description\":\"List files\"}"
        ),
        "the action itself still separates attempts"
    );
    // Nested payloads canonicalize: a reordered `edits` array is the same edit.
    assert_eq!(
        BifurcationEngine::operative_input(
            "{\"path\":\"a.rs\",\"edits\":[{\"a\":1,\"b\":2},{\"c\":3}]}"
        ),
        BifurcationEngine::operative_input(
            "{\"edits\":[{\"b\":2,\"a\":1},{\"c\":3}],\"path\":\"a.rs\"}"
        )
    );
    assert_eq!(BifurcationEngine::operative_input("not json"), "not json");
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

// ---------------------------------------------------------------------------
// Cognitive superconductor — Maxwell's demon
// ---------------------------------------------------------------------------

use crate::session::state::{ContentBlock, ConversationMessage, MessageRole};

fn error_result(id: &str, tool: &str, output: &str) -> ConversationMessage {
    ConversationMessage {
        role: MessageRole::Tool,
        blocks: vec![ContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            tool_name: tool.to_string(),
            output: output.to_string(),
            is_error: true,
        }],
        usage: None,
    }
}

fn ok_result(id: &str, tool: &str, output: &str) -> ConversationMessage {
    let mut message = error_result(id, tool, output);
    if let ContentBlock::ToolResult { is_error, .. } = &mut message.blocks[0] {
        *is_error = false;
    }
    message
}

fn output_of(message: &ConversationMessage) -> &str {
    match &message.blocks[0] {
        ContentBlock::ToolResult { output, .. } => output,
        _ => panic!("expected a tool result"),
    }
}

fn assistant_tool_use(id: &str, tool: &str, input: &str) -> ConversationMessage {
    ConversationMessage::assistant(vec![ContentBlock::ToolUse {
        id: id.to_string(),
        name: tool.to_string(),
        input: input.to_string(),
        signature: None,
    }])
}

#[test]
fn pruning_collapses_dead_orbits_and_preserves_tool_use_ids() {
    let mut engine = BifurcationEngine::new();
    let long_error = format!(
        "{{\n  \"error\": \"panicked at 'fatal: {}'\",\n  \"trace\": \"stack frame one\\nstack frame two\"\n}}",
        "x".repeat(400)
    );
    let mut messages = vec![
        ConversationMessage::user_text("do the thing"),
        assistant_tool_use("t1", "edit_file", "{\"path\":\"a.rs\"}"),
        error_result("t1", "edit_file", &long_error),
        assistant_tool_use("t2", "edit_file", "{\"path\":\"a.rs\"}"),
        error_result("t2", "edit_file", &long_error),
        ok_result("t3", "read_file", "file contents"),
        error_result(
            "t4",
            "edit_file",
            format!(
                "[SYSTEM DIRECTIVE — BIFURCATION: DEAD ORBIT REFUSED] {}",
                "y".repeat(400)
            )
            .as_str(),
        ),
        error_result("t5", "edit_file", &long_error),
    ];

    // Two failed attempts at an unchanged world → a dead orbit with real stats.
    engine.note_attempt("edit_file", "{\"path\":\"a.rs\"}");
    engine.note_outcome("edit_file", "{\"path\":\"a.rs\"}", true, "file_not_found");
    engine.note_attempt("edit_file", "{\"path\":\"a.rs\"}");
    engine.note_outcome("edit_file", "{\"path\":\"a.rs\"}", true, "file_not_found");
    // The read in between does not move the mutation epoch, so the orbit stays dead.

    let pruned = engine.prune_dead_orbit_outputs(&mut messages);
    assert_eq!(pruned, 2, "both stale long errors are dead-orbit exhibits");

    // The newest tool result (t5) stays whole: it is what the model is reacting
    // to now. Refusal directives are live instructions, never dead evidence.
    assert_eq!(
        output_of(&messages[7]),
        long_error,
        "the newest result is never collapsed"
    );
    assert!(output_of(&messages[4]).contains("Full stacktrace purged"));
    assert!(output_of(&messages[4]).contains("category=\"file_not_found\""));
    assert!(output_of(&messages[4]).contains("attempts=2"));
    assert!(output_of(&messages[4]).contains("regime="));
    assert!(output_of(&messages[4]).starts_with("[🗑️ Dead orbit collapsed by runtime"));
    assert!(
        !output_of(&messages[4]).contains("stack frame two"),
        "stack frames purged"
    );
    assert!(
        output_of(&messages[4]).contains("panicked at 'fatal"),
        "the excerpt survives"
    );
    assert!(
        output_of(&messages[4]).chars().count() < 400,
        "the tombstone is one line, not the exhibit"
    );
    assert!(
        output_of(&messages[6]).starts_with("[SYSTEM DIRECTIVE"),
        "directives are not exhibits"
    );

    // Pairing-critical fields survive the rewrite.
    for (index, id) in [(2usize, "t1"), (4, "t2"), (6, "t4")] {
        let ContentBlock::ToolResult {
            tool_use_id,
            tool_name,
            is_error,
            ..
        } = &messages[index].blocks[0]
        else {
            panic!("tool result expected");
        };
        assert_eq!(tool_use_id, id);
        assert_eq!(tool_name, "edit_file");
        assert!(
            *is_error,
            "is_error must survive on a pruned error (index {index})"
        );
    }
    assert_eq!(engine.pruned_outputs(), 2, "the counter is observable");
}

#[test]
fn pruning_is_quiet_when_the_turn_has_no_dead_orbits() {
    let mut engine = BifurcationEngine::new();
    let long = "e".to_string() + &"x".repeat(500);
    let mut messages = vec![
        ConversationMessage::user_text("go"),
        error_result("t1", "write_file", &long),
        error_result("t2", "write_file", &long),
    ];
    // Nothing on the ledger → nothing is provably dead → nothing is collapsed.
    assert_eq!(engine.prune_dead_orbit_outputs(&mut messages), 0);
    assert_eq!(output_of(&messages[1]), long);
}

#[test]
fn probe_gate_blocks_blind_mutations_and_lets_probed_mutations_through() {
    let mut engine = BifurcationEngine::new();
    let mutation = ("edit_file".to_string(), "{\"path\":\"a.rs\"}".to_string());
    let probe = ("read_file".to_string(), "{\"path\":\"a.rs\"}".to_string());

    // Laminar: no contract yet.
    assert_eq!(
        engine.gate_mutations_for_step(std::slice::from_ref(&mutation)),
        Vec::<usize>::new()
    );
    assert!(!engine.probe_required());

    // A crossing arms the gate.
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    assert_eq!(engine.regime(), Regime::Dichotomy);
    assert!(
        engine.probe_required(),
        "the first bifurcation opens the gate"
    );

    // Blind mutation: blocked, charged to the ladder, trip counted.
    let blocked = engine.gate_mutations_for_step(std::slice::from_ref(&mutation));
    assert_eq!(blocked, vec![0]);
    assert_eq!(engine.probe_gate_trips(), 1);
    assert!(
        engine.probe_required(),
        "a blocked write does not satisfy the contract"
    );

    // A mutation preceded by a probe in the same step is legal — but emitting
    // the probe is not yet proof of looking; see
    // `only_a_probe_that_returned_something_discharges_the_gate`.
    assert_eq!(
        engine.gate_mutations_for_step(&[probe.clone(), mutation.clone()]),
        Vec::<usize>::new()
    );
    assert!(
        engine.probe_required(),
        "the gate waits for the probe's outcome, not its emission"
    );
    assert!(BifurcationEngine::probe_gate_refusal("edit_file").contains("PROBE GATE"));
}

#[test]
fn probe_gate_classifies_bash_by_what_it_actually_does() {
    for (command, probe, mutating) in [
        ("cargo check", true, false),
        ("cargo test --workspace", true, false),
        ("git status", true, false),
        ("git diff HEAD~1", true, false),
        ("ls -la", true, false),
        ("cat src/main.rs", true, false),
        ("bash -n script.sh", true, false),
        ("python3 -m py_compile mod.py", true, false),
        ("tsc --noEmit", true, false),
        ("edge-tts --text hi", false, false),
        ("wc -l file", true, false),
        ("echo hi > out.txt", false, true),
        ("echo hi >> out.txt", false, true),
        ("rm -f out", false, true),
        ("sed -i s/a/b/ f.txt", false, true),
        ("cargo add serde", false, true),
        ("touch new.rs", false, true),
    ] {
        let input = format!("{{\"command\":{}}}", serde_json::json!(command));
        assert_eq!(
            BifurcationEngine::is_probe_call("bash", &input),
            probe,
            "probe classification for {command:?}"
        );
        assert_eq!(
            BifurcationEngine::is_mutating_call("bash", &input),
            mutating,
            "mutation classification for {command:?}"
        );
    }
    // Descriptor duplication is not a write; the verifier override wins over it.
    let cmd = "{\"command\":\"cargo test 2>&1 | tail -5\"}";
    assert!(BifurcationEngine::is_probe_call("bash", cmd));
    assert!(!BifurcationEngine::is_mutating_call("bash", cmd));
    // Unknown tools are neither: the gate blocks blind writes, it does not
    // invent failure modes for tools it cannot classify.
    assert!(!BifurcationEngine::is_mutating_call(
        "my_mcp_tool",
        "{\"x\":1}"
    ));
    assert!(!BifurcationEngine::is_probe_call(
        "my_mcp_tool",
        "{\"x\":1}"
    ));
    // Write tools are mutations on their name alone — even a truncated or
    // malformed payload came from a write call.
    assert!(BifurcationEngine::is_mutating_call(
        "write_file",
        "{\"content\":\"x\"}"
    ));
    assert!(BifurcationEngine::is_mutating_call(
        "write_file",
        "not json"
    ));
    // Unparsable bash is inconclusive, so it is not blocked (fail-open).
    assert!(!BifurcationEngine::is_mutating_call("bash", "not json"));
}

#[test]
fn a_blocked_step_reaches_chaos_instead_of_stalling_at_dichotomy() {
    // The blocked mutation never executed, but gate_mutations_for_step pushed
    // probe_gate evidence into the ladder; each gated step must still climb.
    let mut engine = BifurcationEngine::new();
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    assert_eq!(engine.regime(), Regime::Dichotomy);
    let mutation = ("edit_file".to_string(), "{\"path\":\"a.rs\"}".to_string());
    let mut seen = vec![engine.regime()];
    for _ in 0..8 {
        let blocked = engine.gate_mutations_for_step(std::slice::from_ref(&mutation));
        assert!(
            !blocked.is_empty(),
            "a blind retry of a blind write stays blocked"
        );
        engine.observe(&OrbitSample::new(seen.len()));
        seen.push(engine.regime());
    }
    assert_eq!(
        *seen.last().unwrap(),
        Regime::Chaos,
        "blocked blind writes must escalate the ladder: {seen:?}"
    );
}

#[test]
fn syntax_backpressure_does_not_discount_resistance() {
    let mut engine = BifurcationEngine::new();
    fail_step(
        &mut engine,
        "write_file",
        "{\"path\":\"a.json\"}",
        "syntax_error",
    );
    fail_step(
        &mut engine,
        "write_file",
        "{\"path\":\"a.json\"}",
        "syntax_error",
    );
    assert_eq!(engine.regime(), Regime::Dichotomy);
    let stuck = engine.pressure();

    // A write "succeeds" but the file on disk is broken: no α contraction.
    engine.observe(
        &OrbitSample::new(engine.steps() + 1)
            .with_success("write_file")
            .with_mutation()
            .with_syntax_backpressure(),
    );
    assert!(
        engine.pressure() >= stuck - 1e-12,
        "a corrupted artifact is not convergence: {stuck} → {}",
        engine.pressure()
    );
    assert_eq!(engine.syntax_backpressure_hits(), 1);

    // The same step without the marker *is* clean progress.
    engine.observe(
        &OrbitSample::new(engine.steps() + 1)
            .with_success("write_file")
            .with_mutation(),
    );
    assert!(
        engine.pressure() < stuck,
        "a good write still contracts: {}",
        engine.pressure()
    );
}

#[test]
fn escalated_directives_carry_the_turn_working_set() {
    let mut engine = BifurcationEngine::new();
    engine.set_turn_file_summary(Some("/tmp/a.rs (2→4 lines, 2 edits)".to_string()));
    for _ in 0..3 {
        fail_step(&mut engine, "bash", "make", "syntax_error");
    }
    for _ in 0..4 {
        fail_step(&mut engine, "bash", "make", "syntax_error");
    }
    assert!(
        engine.regime() >= Regime::Quadrupole,
        "need L2 for the working set, got {}",
        engine.regime()
    );
    let directive = engine.directive_section().expect("escalated directive");
    assert!(
        directive.contains(
            "Modified files in this turn since last clean state: /tmp/a.rs (2→4 lines, 2 edits)"
        ),
        "{directive}"
    );
}

#[test]
fn summary_exposes_the_demon_counters() {
    let mut engine = BifurcationEngine::new();
    engine.arm_probe_gate();
    engine.prune_dead_orbit_outputs(&mut []);
    let summary = engine.summary();
    assert!(
        summary.contains(
            "Maxwell demon: pruned_outputs=0 probe_gate=armed trips=0 syntax_backpressure_hits=0"
        ),
        "{summary}"
    );
}

#[test]
fn a_turn_that_begins_past_the_first_bifurcation_starts_gated() {
    let mut engine = BifurcationEngine::new();
    // Laminar: a fresh turn has no contract to carry.
    engine.rearm_probe_gate_for_turn();
    assert!(!engine.probe_required());

    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    assert!(engine.probe_required());
    let regime = engine.regime();
    let pressure = engine.pressure();
    engine.rearm_probe_gate_for_turn();
    assert!(
        engine.probe_required(),
        "the next turn must not get a free blind write while the orbit is still splitting"
    );
    assert_eq!(
        engine.regime(),
        regime,
        "escalation state is session-scoped"
    );
    assert_eq!(engine.pressure(), pressure);
}

#[test]
fn only_a_probe_that_returned_something_discharges_the_gate() {
    let mut engine = BifurcationEngine::new();
    let mutation = ("edit_file".to_string(), "{\"path\":\"a.rs\"}".to_string());
    let probe = (
        "read_file".to_string(),
        "{\"path\":\"missing.rs\"}".to_string(),
    );
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    fail_step(
        &mut engine,
        "edit_file",
        "{\"path\":\"a.rs\"}",
        "file_not_found",
    );
    assert!(engine.probe_required());

    // The probe emitted, ran, and failed: that is one more point of the same
    // orbit, not a look. The gate stays armed and the next blind write is blocked.
    engine.gate_mutations_for_step(&[probe.clone(), mutation.clone()]);
    engine.observe(&OrbitSample::new(engine.steps() + 1).with_failure(
        BifurcationEngine::fingerprint("read_file", "file_not_found"),
    ));
    assert!(
        engine.probe_required(),
        "a failed probe cannot buy a blind write"
    );
    assert_eq!(
        engine.gate_mutations_for_step(std::slice::from_ref(&mutation)),
        vec![0]
    );

    // A probe that returned something discharges the contract for the turn.
    engine.gate_mutations_for_step(&[probe.clone(), mutation.clone()]);
    engine.observe(
        &OrbitSample::new(engine.steps() + 1)
            .with_success("read_file")
            .with_successful_probe(),
    );
    assert!(!engine.probe_required());
    assert_eq!(
        engine.gate_mutations_for_step(std::slice::from_ref(&mutation)),
        Vec::<usize>::new()
    );
}

#[test]
fn re_drawing_the_plan_is_neither_a_probe_nor_a_mutation() {
    assert!(BifurcationEngine::is_probe_call(
        "TaskGraph",
        "{\"operation\":\"view\"}"
    ));
    for operation in ["update_status", "add", "remove"] {
        let input = format!("{{\"operation\":\"{operation}\",\"id\":\"1.1\"}}");
        assert!(
            !BifurcationEngine::is_probe_call("TaskGraph", &input),
            "`{operation}` rewrites the plan, it does not look at the world"
        );
        assert!(BifurcationEngine::is_plan_bookkeeping("taskgraph", &input));
    }
    assert!(!BifurcationEngine::is_plan_bookkeeping(
        "TaskGraph",
        "{\"operation\":\"view\"}"
    ));
    assert!(!BifurcationEngine::is_plan_bookkeeping(
        "write_file",
        "{\"path\":\"a.rs\"}"
    ));
    // Unparseable input fails closed: a graph call the runtime cannot prove is a
    // read must not buy a blind write.
    assert!(BifurcationEngine::is_plan_bookkeeping(
        "TaskGraph",
        "not json"
    ));
}

#[test]
fn a_plan_only_streak_forces_the_orbit_out_of_bookkeeping() {
    let mut engine = BifurcationEngine::new();
    for _ in 0..MAX_STALL_STEPS {
        engine.observe(&OrbitSample::new(engine.steps() + 1).with_bookkeeping());
    }
    assert_eq!(engine.stalled_steps(), MAX_STALL_STEPS);
    assert!(
        engine.regime() >= Regime::Dichotomy,
        "a run that only redraws the plan is not converging: {:?}",
        engine.regime()
    );
    assert!(
        engine.probe_required(),
        "the escalation must arm the gate, or the next step is a blind write again"
    );
    let directive = engine
        .directive_section()
        .expect("stalled orbit gets a directive");
    assert!(directive.contains("redrew the task graph"));
    assert!(directive.contains("evidence first, status second"));
    assert!(engine
        .summary()
        .contains(&format!("plan_only_steps={}", MAX_STALL_STEPS)));
}

#[test]
fn the_first_plan_only_step_already_says_so() {
    let mut engine = BifurcationEngine::new();
    engine.observe(&OrbitSample::new(1).with_bookkeeping());
    let directive = engine
        .directive_section()
        .expect("a plan-only streak is worth naming before it escalates");
    assert!(directive.contains("PLAN-ONLY STREAK"));
    assert!(directive.contains("1 consecutive step"));
    assert_eq!(engine.regime(), Regime::Laminar);
    assert_eq!(engine.pressure(), 0.0, "one step is a notice, not pressure");
}

#[test]
fn a_world_action_ends_the_plan_only_streak() {
    let mut engine = BifurcationEngine::new();
    engine.observe(&OrbitSample::new(1).with_bookkeeping());
    engine.observe(&OrbitSample::new(2).with_bookkeeping());
    assert_eq!(engine.stalled_steps(), 2);
    engine.observe(
        &OrbitSample::new(3)
            .with_bookkeeping()
            .with_success("write_file")
            .with_mutation(),
    );
    assert_eq!(
        engine.stalled_steps(),
        0,
        "a graph update next to a real write is progress"
    );
    assert_eq!(engine.regime(), Regime::Laminar);
    assert!(engine.directive_section().is_none());
}

#[test]
fn plan_writes_do_not_advance_the_mutation_epoch() {
    let mut engine = BifurcationEngine::new();
    let bookkeeping = "{\"operation\":\"update_status\",\"id\":\"1.1\",\"status\":\"completed\"}";
    engine.note_attempt("TaskGraph", bookkeeping);
    engine.note_outcome("TaskGraph", bookkeeping, false, "");
    assert_eq!(
        engine.mutation_epoch(),
        0,
        "the plan moved; the world did not, so an identical retry is still the same orbit"
    );
    engine.note_outcome("edit_file", "{\"path\":\"a.rs\"}", false, "");
    assert_eq!(engine.mutation_epoch(), 1);
}
