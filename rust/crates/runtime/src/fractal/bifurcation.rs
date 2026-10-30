//! Bifurcation ladder — the cognitive half of the δ/α control plane.
//!
//! [`super::rate_limiter`] already absorbs *transport* chaos: a provider that
//! will not answer, retried with a δ-scaled pause until the breaker sheds. This
//! module governs the other failure mode, which no HTTP retry policy can see —
//! the agent keeps acting, every call returns cleanly, and the workspace still
//! does not approach the goal.
//!
//! ## What is actually measured
//!
//! No hidden metaphor: every quantity below is a count of observable events in
//! the `run_turn` loop of [`crate::session::conversation::ConversationRuntime`].
//!
//! - `pressure` (r) — resistance without information. A step that fails with a
//!   fingerprint the runtime has already seen, while nothing in the workspace
//!   changed, adds 1.0. A step that fails on genuinely *new* ground adds 0.5
//!   (struggling, but learning). A step with clean progress divides r by α, so
//!   convergence drains resistance instead of hiding it.
//! - `mutation_epoch` — bumped whenever a successful call could have changed the
//!   world. This is what makes the refusal gate sound: an identical retry can
//!   only produce an identical result while the epoch never moved, so refusing
//!   it removes nothing but the loop.
//! - [`Regime`] — r is compared against a period-doubling ladder whose gaps
//!   shrink by δ (r₂ = r₁+Δ, r₃ = r₂+Δ/δ, …). Because those gaps accumulate at a
//!   *finite* chaos point, a struggling agent escalates faster and faster until
//!   it is forced out of the basin, then — after a bounded number of steps — is
//!   told to stop and hand the decision to a human. The ladder advances at most
//!   one doubling per iteration, so each thinking mode gets a real turn instead
//!   of being skipped by a coarse step size.
//!
//! ## What it does with the measurement
//!
//! Two levers, both already proven in this codebase:
//!
//! 1. inject a directive into the next iteration's system prompt (the same slot
//!    `ErrorTracker` uses for learned skills), and
//! 2. refuse the dead retry through the middleware chain (the same slot
//!    [`crate::loop_detection`] uses today, but with a semantic test instead of
//!    byte equality).
//!
//! Transient failures (`timeout`, `connection_error`, `rate_limit`) are never
//! refused: retrying those is legitimate and belongs to the circuit breaker.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::constants::{FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA};

/// Resistance that opens the first bifurcation (r₁), in units where one failing
/// step at an unchanged world is worth [`REPEAT_STEP_PRESSURE`].
pub const FIRST_BIFURCATION_PRESSURE: f64 = 1.0;

/// Width of the first cascade gap (r₂ = r₁ + Δ); later gaps shrink by δ.
pub const PRESSURE_GAP: f64 = 1.0;

/// Pressure added by a step that repeats a known failure without changing the
/// world — the limit-cycle signature.
pub const REPEAT_STEP_PRESSURE: f64 = 0.5;

/// Pressure added by a step that fails on ground the runtime has not seen yet:
/// resistance, but the agent is still learning, so it weighs half as much.
pub const NOVEL_STEP_PRESSURE: f64 = 0.25;

/// Extra pressure when a tool that previously *succeeded* starts failing: the
/// agent is moving away from the attractor, not merely around it.
pub const REGRESSION_PRESSURE: f64 = 0.75;

/// Steps allowed past the accumulation point before the ladder asks for a human.
/// Only honoured when [`BifurcationEngine::with_halt_at_chaos`] is enabled.
pub const CHAOS_HALT_STEPS: usize = 3;

/// Attempts tolerated before the gate closes on an unchanged world. One executed
/// failure is enough: the retry is the loop. This is not stricter than the
/// byte-equality detector it replaces — that one already denied the second
/// identical call because it counted the assistant message being executed. The
/// difference is what the agent is told: two orbits to split into, with the
/// measured evidence, instead of a bare prohibition.
pub const MIN_ATTEMPTS_BEFORE_REFUSAL: usize = 1;

/// Retention on the failed-attempt ledger. Bounded so a long session cannot grow
/// it without limit; the oldest entries are evicted first.
pub const MAX_LEDGER_ENTRIES: usize = 64;

/// Transport failures. These are the circuit breaker's job (pause and retry with
/// a δ-scaled backoff); the ladder only under-weights them, because an identical
/// retry of a timed-out call can legitimately succeed.
pub const TRANSIENT_CATEGORIES: &[&str] = &[
    "timeout",
    "connection_error",
    "rate_limit",
    "network_unreachable",
];

/// Thinking regime the runtime is currently forcing on the model. Ordered by
/// severity so `max()` composes the pressure-derived level with a cycle floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Regime {
    /// Period 2⁰ — laminar flow. One hypothesis, one action, no directive text
    /// injected: the ladder costs nothing while the agent is converging.
    Laminar,
    /// Period 2¹ — the orbit must split: an orthodox fix *and* an inversion
    /// that routes around the blocked component.
    Dichotomy,
    /// Period 2² — four orthogonal perspectives are demanded before acting.
    Quadrupole,
    /// Past r∞. Local logic is exhausted; leave the basin or stop and report.
    Chaos,
}

impl Regime {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Laminar => "laminar",
            Self::Dichotomy => "dichotomy",
            Self::Quadrupole => "quadrupole",
            Self::Chaos => "chaos",
        }
    }

    /// Cascade position: 0 = laminar … 3 = past the accumulation point.
    #[must_use]
    pub fn level(self) -> usize {
        match self {
            Self::Laminar => 0,
            Self::Dichotomy => 1,
            Self::Quadrupole => 2,
            Self::Chaos => 3,
        }
    }

    /// Number of distinct orbits the regime demands. Chaos is aperiodic, so it
    /// is reported as unbounded rather than pretending to be "period 8".
    #[must_use]
    pub fn orbits(self) -> Option<usize> {
        match self {
            Self::Laminar => Some(1),
            Self::Dichotomy => Some(2),
            Self::Quadrupole => Some(4),
            Self::Chaos => None,
        }
    }

    /// Cascade position clamped to the top of the ladder, so `level + 1` never
    /// overshoots into a regime that does not exist.
    #[must_use]
    pub fn from_level(level: usize) -> Self {
        match level {
            0 => Self::Laminar,
            1 => Self::Dichotomy,
            2 => Self::Quadrupole,
            _ => Self::Chaos,
        }
    }
}

impl std::fmt::Display for Regime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// The period-doubling ladder that the pressure scale is calibrated against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CascadeThresholds {
    pub r1: f64,
    pub r2: f64,
    pub r3: f64,
    /// Finite accumulation point of the cascade: `r1 + gap/(δ-1)`.
    pub r_infinity: f64,
}

impl CascadeThresholds {
    /// Build the ladder from the first threshold and the first gap. Each later
    /// gap shrinks by δ, so the cascade accumulates at a finite chaos point —
    /// the same property the logistic map has. Concretely: r₂ = r₁ + Δ,
    /// r₃ = r₂ + Δ/δ, and r∞ = r₁ + Δ/(1 − 1/δ).
    #[must_use]
    pub fn scaled(first: f64, gap: f64) -> Self {
        let r2 = first + gap;
        Self {
            r1: first,
            r2,
            r3: r2 + gap / FEIGENBAUM_DELTA,
            r_infinity: first + gap / (1.0 - 1.0 / FEIGENBAUM_DELTA),
        }
    }

    #[must_use]
    pub fn feigenbaum() -> Self {
        Self::scaled(FIRST_BIFURCATION_PRESSURE, PRESSURE_GAP)
    }

    /// Which regime a given pressure sits in.
    #[must_use]
    pub fn regime_at(&self, pressure: f64) -> Regime {
        if pressure >= self.r3 {
            Regime::Chaos
        } else if pressure >= self.r2 {
            Regime::Quadrupole
        } else if pressure >= self.r1 {
            Regime::Dichotomy
        } else {
            Regime::Laminar
        }
    }

    /// Threshold for a regime, or `None` for laminar flow (no threshold to
    /// cross — it is where the agent starts).
    #[must_use]
    pub fn threshold_for(&self, regime: Regime) -> Option<f64> {
        match regime {
            Regime::Laminar => None,
            Regime::Dichotomy => Some(self.r1),
            Regime::Quadrupole => Some(self.r2),
            Regime::Chaos => Some(self.r3),
        }
    }

    #[must_use]
    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!(
                "  L1 dichotomy  opens at r ≥ {:.3}",
                self.r1
            ),
            format!("  L2 quadrupole opens at r ≥ {:.3}", self.r2),
            format!("  L3 chaos      opens at r ≥ {:.3}", self.r3),
            format!(
                "  cascade accumulation point r∞ = {:.3} (gap₁={:.3}, each later gap ÷ δ = {:.4})",
                self.r_infinity,
                self.r2 - self.r1,
                FEIGENBAUM_DELTA
            ),
        ]
    }
}

impl Default for CascadeThresholds {
    fn default() -> Self {
        Self::feigenbaum()
    }
}

/// Identity of one attempted call: the tool plus a hash of its input.
pub type AttemptKey = (String, u64);

/// Why the runtime wants to refuse a retry, phrased in observed facts so the
/// model can argue with reality rather than with a bare prohibition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub tool: String,
    pub category: String,
    pub attempts: usize,
    pub epoch: u64,
}

impl Refusal {
    #[must_use]
    pub fn to_directive(&self) -> String {
        format!(
            "[SYSTEM DIRECTIVE — BIFURCATION: DEAD ORBIT REFUSED]\n\
             `{tool}` with this exact input already failed with `{category}` \
             ({attempts} prior attempt{s}) at mutation epoch {epoch}. Nothing that could \
             change the outcome has succeeded since, so the same call cannot produce a \
             different result.\n\
             Split the orbit instead:\n\
             1. ORTHODOX — name the concrete precondition that must become true first, and \
             make that the next action (a read-only probe counts).\n\
             2. INVERSION — reach the same goal without this call at all (different tool, \
             different layer, different entry point).\n\
             Then record the split: use TaskGraph operation 'add' to create the two orbit \
             sub-tasks under the active leaf, and mark the blocked approach 'failed'. Do not \
             delete the failing task.",
            tool = self.tool,
            category = self.category,
            attempts = self.attempts,
            s = if self.attempts == 1 { "" } else { "s" },
            epoch = self.epoch,
        )
    }
}

/// A bifurcation that resolved: the agent was stuck at some regime and then got
/// a previously failing tool to work. This is the moment worth remembering.
#[derive(Debug, Clone, PartialEq)]
pub struct Escape {
    pub tool: String,
    /// Regime the agent escaped from.
    pub from_regime: Regime,
    /// Steps spent stuck before the escape.
    pub stuck_steps: usize,
    /// Input the failing orbit kept using, already truncated.
    pub failed_input_summary: String,
}

impl Escape {
    /// One-line provenance for a learned skill, so the axiom records *which*
    /// orbit died rather than only which one worked.
    #[must_use]
    pub fn provenance_line(&self) -> String {
        let orbits = self
            .from_regime
            .orbits()
            .map_or_else(|| "chaotic".to_string(), |n| format!("period-{n}"));
        let dead = if self.failed_input_summary.is_empty() {
            String::new()
        } else {
            format!(
                " Dead orbit: `{}`.",
                self.failed_input_summary.replace('\n', " ")
            )
        };
        format!(
            "Found by a {orbits} orbit split after {} stuck step{s} on `{tool}`.{dead}",
            self.stuck_steps,
            s = if self.stuck_steps == 1 { "" } else { "s" },
            tool = self.tool,
        )
    }
}

/// One step of the trajectory, assembled by the conversation loop from the tool
/// results it just observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrbitSample {
    /// Iteration number within the turn.
    pub iteration: usize,
    /// `tool:category` fingerprints of the calls that failed in this step.
    pub failures: Vec<String>,
    /// Transient fingerprints among `failures` (exempt from refusal, lighter on
    /// pressure — retrying them is reasonable).
    pub transient_failures: Vec<String>,
    /// Effective tool names whose calls succeeded in this step.
    pub successes: Vec<String>,
    /// True when at least one success could have changed the workspace.
    pub mutated_state: bool,
}

impl OrbitSample {
    #[must_use]
    pub fn new(iteration: usize) -> Self {
        Self {
            iteration,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_failure(mut self, fingerprint: impl Into<String>) -> Self {
        self.failures.push(fingerprint.into());
        self
    }

    #[must_use]
    pub fn with_transient_failure(mut self, fingerprint: impl Into<String>) -> Self {
        let fingerprint = fingerprint.into();
        self.failures.push(fingerprint.clone());
        self.transient_failures.push(fingerprint);
        self
    }

    #[must_use]
    pub fn with_success(mut self, tool: impl Into<String>) -> Self {
        self.successes.push(tool.into());
        self
    }

    #[must_use]
    pub fn with_mutation(mut self) -> Self {
        self.mutated_state = true;
        self
    }
}

/// Failed attempt retained in the ledger.
#[derive(Debug, Clone)]
struct FailedAttempt {
    tool: String,
    input_summary: String,
    category: String,
    /// Mutation epoch the failure was recorded at.
    epoch: u64,
    attempts: usize,
    step: usize,
    transient: bool,
}

/// Tracks the trajectory of one conversation loop and forces the thinking mode
/// to split when resistance accumulates.
#[derive(Debug, Clone)]
pub struct BifurcationEngine {
    thresholds: CascadeThresholds,
    pressure: f64,
    regime: Regime,
    /// Floor imposed by a detected limit cycle, independent of pressure.
    cycle_floor: Regime,
    mutation_epoch: u64,
    steps: usize,
    chaos_steps: usize,
    ledger: HashMap<AttemptKey, FailedAttempt>,
    ledger_order: VecDeque<AttemptKey>,
    /// Failure fingerprints already produced in earlier steps. Repeating one is
    /// the limit-cycle signature, so this must not include the current step.
    seen_fingerprints: HashSet<String>,
    /// Tools that succeeded at some point, for regression detection.
    proven_tools: HashMap<String, usize>,
    /// Dead orbits the agent asked for during the current step and the gate
    /// blocked. A refused call never executes, so without recording it here the
    /// ladder would be blind to its cleanest limit-cycle evidence. Drained by
    /// [`Self::observe`].
    refused_this_step: Vec<String>,
    /// Set when the ladder is past r∞ for [`CHAOS_HALT_STEPS`] steps and halting
    /// was enabled by the caller.
    halt_requested: bool,
    halt_at_chaos: bool,
    /// Escape detected in the last `observe`/`note_outcome`, consumed by the
    /// skill forge.
    pending_escape: Option<Escape>,
    crossings: Vec<(usize, Regime)>,
}

impl Default for BifurcationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BifurcationEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            thresholds: CascadeThresholds::feigenbaum(),
            pressure: 0.0,
            regime: Regime::Laminar,
            cycle_floor: Regime::Laminar,
            mutation_epoch: 0,
            steps: 0,
            chaos_steps: 0,
            ledger: HashMap::new(),
            ledger_order: VecDeque::new(),
            seen_fingerprints: HashSet::new(),
            proven_tools: HashMap::new(),
            refused_this_step: Vec::new(),
            halt_requested: false,
            halt_at_chaos: false,
            pending_escape: None,
            crossings: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_thresholds(mut self, thresholds: CascadeThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }

    /// Opt in to ending the turn when the system sits past the accumulation point
    /// for too long. Off by default: burning a user's turn is worse than burning
    /// a token, so halting stays an explicit policy choice.
    #[must_use]
    pub fn with_halt_at_chaos(mut self, halt_at_chaos: bool) -> Self {
        self.halt_at_chaos = halt_at_chaos;
        self
    }

    #[must_use]
    pub fn regime(&self) -> Regime {
        self.regime
    }

    #[must_use]
    pub fn pressure(&self) -> f64 {
        self.pressure
    }

    #[must_use]
    pub fn thresholds(&self) -> CascadeThresholds {
        self.thresholds
    }

    #[must_use]
    pub fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    #[must_use]
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Bifurcations crossed this turn, as `(step, regime)` — the audit trail.
    #[must_use]
    pub fn crossings(&self) -> &[(usize, Regime)] {
        &self.crossings
    }

    #[must_use]
    pub fn halt_requested(&self) -> bool {
        self.halt_requested
    }

    /// Transient categories belong to the circuit breaker, not to the ladder: an
    /// identical retry of a timed-out call can legitimately succeed.
    #[must_use]
    pub fn is_transient_category(category: &str) -> bool {
        TRANSIENT_CATEGORIES.contains(&category)
    }

    /// The categories the ladder deliberately under-weights, for status output.
    #[must_use]
    pub fn transient_categories() -> &'static [&'static str] {
        TRANSIENT_CATEGORIES
    }

    /// Tools whose success provably cannot change the workspace. Everything not
    /// listed counts as a mutation, which keeps the refusal gate conservative: an
    /// identical retry is only refused when nothing that could matter happened
    /// in between. Unknown MCP tools are treated as mutating for the same reason.
    #[must_use]
    pub fn is_read_only_tool(name: &str) -> bool {
        let lower = name.to_lowercase();
        matches!(
            lower.as_str(),
            "read_file" | "list_dir" | "glob_search" | "grep_search" | "read" | "ls" | "grep"
        ) || lower.starts_with("read_")
            || lower.starts_with("list_")
            || lower.starts_with("get_")
            || lower.starts_with("search_")
            || lower.starts_with("view_")
    }

    /// Fingerprint used for pressure: `tool:category`.
    #[must_use]
    pub fn fingerprint(tool: &str, category: &str) -> String {
        format!("{tool}:{category}")
    }

    #[must_use]
    pub fn attempt_key(tool: &str, input: &str) -> AttemptKey {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        (tool.to_string(), u64::from_be_bytes(bytes))
    }

    fn truncate(input: &str) -> String {
        const LIMIT: usize = 240;
        let mut out = String::new();
        for ch in input.chars() {
            if out.len() >= LIMIT {
                out.push('…');
                break;
            }
            out.push(ch);
        }
        out
    }

    // -----------------------------------------------------------------------
    // Refusal gate (called from the middleware chain, before execution)
    // -----------------------------------------------------------------------

    /// An orbit is dead when the ledger holds a real (non-transient) failure for
    /// it and the mutation epoch has not moved since — nothing that could change
    /// the outcome has succeeded in between.
    fn is_dead_orbit(&self, entry: &FailedAttempt) -> bool {
        !entry.transient
            && !entry.category.is_empty()
            && entry.attempts >= MIN_ATTEMPTS_BEFORE_REFUSAL
            && entry.epoch == self.mutation_epoch
    }

    /// Would this exact retry be refused? Only a *non-transient* failure whose
    /// recorded epoch still matches the current one qualifies, and only once
    /// [`MIN_ATTEMPTS_BEFORE_REFUSAL`] attempts are on the record: if the epoch
    /// moved, something in the world changed and the retry is a new experiment.
    #[must_use]
    pub fn would_refuse(&self, key: &AttemptKey) -> Option<Refusal> {
        let entry = self.ledger.get(key)?;
        if !self.is_dead_orbit(entry) {
            return None;
        }
        Some(Refusal {
            tool: entry.tool.clone(),
            category: entry.category.clone(),
            attempts: entry.attempts,
            epoch: entry.epoch,
        })
    }

    /// Record that the gate blocked a dead orbit this step. Called by the
    /// middleware right after it denies, so the refusal counts as trajectory
    /// evidence even though no tool ever ran.
    pub fn note_refusal(&mut self, refusal: &Refusal) {
        self.refused_this_step
            .push(Self::fingerprint(&refusal.tool, &refusal.category));
    }

    /// Note that an attempt is starting. The outcome arrives in
    /// [`Self::note_outcome`]; a call that never completes simply stays pending.
    pub fn note_attempt(&mut self, tool: &str, input: &str) {
        let key = Self::attempt_key(tool, input);
        if let Some(entry) = self.ledger.get_mut(&key) {
            if entry.epoch == self.mutation_epoch {
                entry.attempts += 1;
            } else {
                // A fresh epoch makes the previous failure history stale for the
                // refusal gate, but keeps the tool-level pressure accounting.
                entry.epoch = self.mutation_epoch;
                entry.attempts = 1;
                entry.step = self.steps;
            }
            return;
        }
        let entry = FailedAttempt {
            tool: tool.to_string(),
            input_summary: Self::truncate(input),
            category: String::new(),
            epoch: self.mutation_epoch,
            attempts: 1,
            step: self.steps,
            transient: false,
        };
        self.ledger.insert(key.clone(), entry);
        self.ledger_order.push_back(key);
        while self.ledger_order.len() > MAX_LEDGER_ENTRIES {
            if let Some(oldest) = self.ledger_order.pop_front() {
                self.ledger.remove(&oldest);
            }
        }
    }

    /// Record the result of an attempt the engine was told about. A success on a
    /// tool that could have changed the workspace advances the mutation epoch
    /// immediately, so the very next retry is judged against the new world.
    pub fn note_outcome(&mut self, tool: &str, input: &str, is_error: bool, category: &str) {
        let key = Self::attempt_key(tool, input);
        if is_error {
            if let Some(entry) = self.ledger.get_mut(&key) {
                entry.category = category.to_string();
                entry.transient = Self::is_transient_category(category);
            }
            return;
        }
        if !Self::is_read_only_tool(tool) {
            self.mutation_epoch += 1;
        }
        self.proven_tools.insert(tool.to_string(), self.steps);
        // A tool that was failing now works: that is the escape, whether the
        // identical call finally passed (the world moved under it) or a different
        // input did (the orbit split). Only the identical entry is retired — the
        // dead orbit stays in the ledger so the gate keeps refusing it.
        let identical = self.ledger.remove(&key);
        if identical.is_some() {
            self.ledger_order.retain(|k| k != &key);
        }
        let dead = match identical {
            Some(entry) if !entry.category.is_empty() => Some(entry),
            _ => self.failed_entry_for_tool(tool),
        };
        if let Some(entry) = dead {
            if self.regime > Regime::Laminar && self.pending_escape.is_none() {
                self.pending_escape = Some(Escape {
                    tool: tool.to_string(),
                    from_regime: self.regime,
                    stuck_steps: self.steps.saturating_sub(entry.step).max(1),
                    failed_input_summary: entry.input_summary,
                });
            }
            // The attractor was reached: two doublings of contraction, so the
            // ladder goes quiet without letting one win erase every other block.
            self.pressure /= FEIGENBAUM_ALPHA * FEIGENBAUM_ALPHA;
            self.cycle_floor = Regime::Laminar;
            self.regime = self
                .thresholds
                .regime_at(self.pressure)
                .max(self.cycle_floor);
            self.chaos_steps = 0;
            self.halt_requested = false;
        }
    }

    /// Copy of the first ledger entry that recorded a real failure for `tool`.
    /// The entry itself stays, so a refused orbit stays refused.
    fn failed_entry_for_tool(&self, tool: &str) -> Option<FailedAttempt> {
        self.ledger
            .iter()
            .find(|((name, _), entry)| name == tool && !entry.category.is_empty())
            .map(|(_, entry)| entry.clone())
    }

    /// Take the escape detected since the last call, for the skill forge.
    pub fn take_escape(&mut self) -> Option<Escape> {
        self.pending_escape.take()
    }

    // -----------------------------------------------------------------------
    // Observation (called once per iteration with the step's tool results)
    // -----------------------------------------------------------------------

    /// Feed one step of the trajectory and return the regime the engine settled
    /// into — `Some` only when a bifurcation was crossed or recrossed upward.
    pub fn observe(&mut self, sample: &OrbitSample) -> Option<Regime> {
        self.steps += 1;

        // A blocked retry is a step of its own: the agent tried to close the
        // orbit and the gate stopped it. Fold those attempts in before measuring,
        // otherwise the ladder is blind to its clearest limit-cycle signature.
        let refused = std::mem::take(&mut self.refused_this_step);
        let mut delta = 0.0f64;
        let mut saw_failure = false;

        for fingerprint in sample.failures.iter().chain(refused.iter()) {
            saw_failure = true;
            let tool = fingerprint.split(':').next().unwrap_or(fingerprint);
            let transient = fingerprint
                .split_once(':')
                .map_or(false, |(_, category)| Self::is_transient_category(category));

            if self.proven_tools.contains_key(tool) {
                // Something that used to work now fails: divergence, not a cycle.
                delta += REGRESSION_PRESSURE;
            } else if transient {
                delta += NOVEL_STEP_PRESSURE * 0.5;
            } else if self.known_fingerprint(fingerprint) {
                // The orbit closed: this exact failure has already been seen and
                // the step produced no change of state. No agent may stay here.
                delta += REPEAT_STEP_PRESSURE;
                if !sample.mutated_state {
                    self.cycle_floor = self.cycle_floor.max(Regime::Dichotomy);
                }
            } else {
                delta += NOVEL_STEP_PRESSURE;
            }
        }

        if saw_failure {
            self.pressure += delta;
        } else if sample.mutated_state || !sample.successes.is_empty() {
            // Clean forward step: contract the orbit by α.
            self.pressure /= FEIGENBAUM_ALPHA;
            self.cycle_floor = Regime::Laminar;
        }

        self.seen_fingerprints
            .extend(sample.failures.iter().cloned().chain(refused));

        let before = self.regime;
        let derived = self.thresholds.regime_at(self.pressure).max(self.cycle_floor);
        // The cascade advances at most one doubling per iteration. Without this
        // clamp the later gaps (Δ/δⁿ) are smaller than a single failing step and
        // the ladder would jump straight to chaos, never showing the intermediate
        // thinking modes it exists to force.
        self.regime = if derived > before {
            Regime::from_level(before.level() + 1)
        } else {
            derived
        };

        if self.regime == Regime::Chaos {
            self.chaos_steps += 1;
            if self.halt_at_chaos && self.chaos_steps >= CHAOS_HALT_STEPS {
                self.halt_requested = true;
            }
        } else {
            self.chaos_steps = 0;
            self.halt_requested = false;
        }

        if self.regime != before && self.regime > before {
            self.crossings.push((sample.iteration, self.regime));
            tracing::warn!(
                iteration = sample.iteration,
                regime = self.regime.as_str(),
                pressure = (self.pressure * 1000.0).round() / 1000.0,
                chaos_steps = self.chaos_steps,
                "bifurcation crossed: forcing a wider thinking mode"
            );
            return Some(self.regime);
        }
        None
    }

    fn known_fingerprint(&self, fingerprint: &str) -> bool {
        self.seen_fingerprints.contains(fingerprint)
    }
    // -----------------------------------------------------------------------
    // Directives
    // -----------------------------------------------------------------------

    /// System-prompt section for the current regime, or `None` while the agent is
    /// converging. Kept short and factual: it states what was measured, then what
    /// to do about it.
    #[must_use]
    pub fn directive_section(&self) -> Option<String> {
        let measured = format!(
            "Measured by the runtime: pressure r={:.3}, thresholds r₁={:.3} r₂={:.3} r∞={:.3}, \
             step #{}, mutation epoch {}.",
            self.pressure, self.thresholds.r1, self.thresholds.r2, self.thresholds.r_infinity,
            self.steps, self.mutation_epoch,
        );
        let blocked = self.dead_orbits_line();
        let body = match self.regime {
            Regime::Laminar => return None,
            Regime::Dichotomy => format!(
                "## 🔀 BIFURCATION L1 — PERIOD-2 ORBIT SPLIT (runtime-enforced)\n\
                 {measured}{blocked}\n\
                 Resistance passed the first bifurcation: the current approach is not \
                 converging and repeating it is now forbidden behaviour.\n\
                 Before your next action, produce exactly two mutually exclusive orbits:\n\
                 1. ORTHODOX — the smallest fix that changes the failing precondition, inside \
                 the current approach.\n\
                 2. INVERSION — reach the goal without touching the blocked component at all \
                 (different tool, layer, entry point, or format).\n\
                 Then: pick one, name it in a single line, and state the cheapest read-only \
                 probe that would falsify it before running any mutating action. Record the \
                 split in TaskGraph with operation 'add' (two sibling sub-tasks under the \
                 active leaf) and mark the dead approach 'failed' — never delete it."
            ),
            Regime::Quadrupole => format!(
                "## 🔀🔀 BIFURCATION L2 — PERIOD-4 QUADRUPLE (runtime-enforced)\n\
                 {measured}{blocked}\n\
                 Both simple orbits have now met resistance, so the second bifurcation is open. \
                 One more attempt inside the same frame of mind is not allowed. Generate four \
                 orthogonal candidates before acting, one line each:\n\
                 1. ARCHITECT — change the structure so the failing path does not exist.\n\
                 2. MINIMALIST — delete the requirement: what is the smallest behaviour that \
                 still satisfies the goal?\n\
                 3. CONSTRAINT BREAKER — which assumption (path, binary, format, permission, \
                 protocol) is treated as fixed but has never actually been verified?\n\
                 4. ROOT-CAUSE — what is the upstream condition that makes every downstream \
                 symptom appear?\n\
                 Choose the candidate that is cheapest to test, run its read-only probe first, \
                 and add it to TaskGraph as the new active sub-task.{regression}"
            ,
                regression = self.regression_advisory(),
            ),
            Regime::Chaos => format!(
                "## 💥 BIFURCATION L3 — CHAOS POINT (r ≥ r∞, runtime-enforced)\n\
                 {measured}{blocked}\n\
                 Local logic inside this basin is exhausted — the cascade has run out of \
                 doublings. Do exactly one of the following, nothing else:\n\
                 A. LEVY STEP — change *scale*, not tactic. Leave the assumed files and inspect \
                 the layer above: environment, PATH and binary resolution, versions, \
                 permissions, sandbox, generated artifacts, the actual protocol on the wire. \
                 Probe read-only first.\n\
                 B. HAND OFF — if what is missing is a decision, a credential, or an authority \
                 you do not have, STOP acting and report: goal, orbits tried, the measured \
                 blocker, two options with their cost.\n\
                 Re-issuing a blocked call is refused by the runtime. If you produce neither a \
                 widened probe nor a hand-off report, the turn is only burning tokens.",
            ),
        };
        Some(body)
    }

    fn regression_advisory(&self) -> String {
        if self.proven_tools.is_empty() {
            return String::new();
        }
        let regressed: Vec<&str> = self
            .ledger
            .values()
            .filter(|entry| self.proven_tools.contains_key(&entry.tool))
            .map(|entry| entry.tool.as_str())
            .collect();
        if regressed.is_empty() {
            return String::new();
        }
        format!(
            "\n\n⚠️ Divergence: {} ({}) previously succeeded and now fails — you are moving away \
             from the attractor, not around it. Restore the working state before trying \
             anything new, and if the damage is unclear, say so and ask rather than editing \
             further.",
            if regressed.len() == 1 { "tool" } else { "tools" },
            regressed.join(", ")
        )
    }

    /// Factual list of the orbits currently blocked, so directives cite evidence.
    fn dead_orbits_line(&self) -> String {
        let mut blocked: Vec<&FailedAttempt> = self
            .ledger
            .values()
            .filter(|entry| self.is_dead_orbit(entry))
            .collect();
        if blocked.is_empty() {
            return String::new();
        }
        blocked.sort_by_key(|entry| std::cmp::Reverse((entry.attempts, entry.step)));
        let items: Vec<String> = blocked
            .iter()
            .take(4)
            .map(|entry| {
                format!(
                    "{}× `{}` → `{}`",
                    entry.attempts, entry.tool, entry.category
                )
            })
            .collect();
        format!("\nBlocked orbits: {}.", items.join("; "))
    }

    /// Human-readable state of the cognitive ladder, mirroring
    /// `FractalRateLimiter::bifurcation_summary` for the transport ladder.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut lines = vec![
            format!(
                "Bifurcation ladder (δ = {:.4}, α = {:.4})",
                FEIGENBAUM_DELTA, FEIGENBAUM_ALPHA
            ),
            format!(
                "State: regime={} pressure={:.3} steps={} epoch={} blocked_orbits={} crossings={}",
                self.regime.as_str(),
                self.pressure,
                self.steps,
                self.mutation_epoch,
                self.ledger
                    .values()
                    .filter(|entry| self.is_dead_orbit(entry))
                    .count(),
                self.crossings.len(),
            ),
        ];
        lines.extend(self.thresholds.summary_lines());
        for (step, regime) in &self.crossings {
            lines.push(format!("  crossed → {} at iteration {step}", regime.as_str()));
        }
        if self.halt_at_chaos {
            lines.push(format!(
                "  chaos halt enabled after {CHAOS_HALT_STEPS} steps past r∞ (requested: {})",
                self.halt_requested
            ));
        }
        lines.join("\n")
    }
}
