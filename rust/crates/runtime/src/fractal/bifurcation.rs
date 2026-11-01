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
//!
//! ## Cognitive superconductor (Maxwell's demon)
//!
//! Three further levers make *any user-selected model* behave at its ceiling
//! without ever switching it: no call here substitutes a model — the intelligence
//! comes from context hygiene and execution contracts.
//!
//! - [`BifurcationEngine::prune_dead_orbit_outputs`] — every bifurcation
//!   crossing collapses the stale error `tool_result` blocks of the current turn
//!   into one-line tombstones that keep the `tool_use_id` pairing intact. The
//!   model stops re-reading its own dead orbits, and the attention that used to
//!   drift into those stacktraces is spent on the live problem.
//! - [`BifurcationEngine::gate_mutations_for_step`] — the hard probe gate: once
//!   the ladder leaves laminar flow, a mutating call is only allowed if a
//!   read-only probe was emitted *before* it in the same step. Blind writes are
//!   refused pre-execution; seeing first is a contract, not advice. The contract
//!   is discharged by a probe that returned something
//!   ([`OrbitSample::successful_probe`]), never by one that errored, and it
//!   carries into the next user turn instead of resetting: a turn that begins
//!   mid-cascade gets no free blind write.
//! - [`crate::syntax_guard`] backpressure — a syntactically broken write is
//!   flagged in its own tool output, and [`OrbitSample::syntax_backpressure`]
//!   stops the ladder from discounting resistance for it.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::constants::{FEIGENBAUM_ALPHA, FEIGENBAUM_DELTA};
use crate::bash_validation::{classify_command, CommandIntent};
use crate::session::state::{ContentBlock, ConversationMessage, MessageRole};

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

/// Error outputs at or below this length are already cheap to attend to — only
/// verbose ones (stacktraces, large diffs) are worth collapsing into a tombstone.
pub const TOMBSTONE_MIN_OUTPUT_CHARS: usize = 300;

/// Character budget for the error excerpt kept inside a tombstone.
const TOMBSTONE_EXCERPT_CHARS: usize = 160;

/// Consecutive steps that only redraw the plan before the ladder treats that as
/// resistance. Re-tasking feels like progress — every call succeeds — while the
/// world is untouched, which is how a run narrates ten plan updates and delivers
/// nothing. Past this streak the orbit is forced to split.
pub const MAX_STALL_STEPS: usize = 3;

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
            format!("  L1 dichotomy  opens at r ≥ {:.3}", self.r1),
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
             1. ORTHODOX — state the precondition as the thing that has to become true, named \
             by its artifact: the command that must exit 0, the file that must exist, the tool \
             that must be on PATH. Make that the next action (a read-only probe counts). \
             \"Determine the precondition\" is not a step: it restates the task it was written \
             under, and a child that repeats its parent is refused by the graph.\n\
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
    /// True when a mutation succeeded but the syntax guard proved the file is
    /// broken. The world did change — but the step is not clean progress, so
    /// [`BifurcationEngine::observe`] refuses to contract pressure by α for it.
    pub syntax_backpressure: bool,
    /// True when this step ran a read-only probe that actually returned
    /// something. Emitting `read_file` on a path that does not exist is not
    /// looking — it is the orbit the gate exists to break — so a failed probe
    /// cannot discharge [`BifurcationEngine::probe_required`].
    pub successful_probe: bool,
    /// True when every call in the step was plan bookkeeping: the plan moved,
    /// the world did not. [`BifurcationEngine::observe`] counts those steps as
    /// stalled rather than treating them as forward progress.
    pub bookkeeping: bool,
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
    pub fn with_successful_probe(mut self) -> Self {
        self.successful_probe = true;
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

    #[must_use]
    pub fn with_syntax_backpressure(mut self) -> Self {
        self.syntax_backpressure = true;
        self
    }

    #[must_use]
    pub fn with_bookkeeping(mut self) -> Self {
        self.bookkeeping = true;
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

/// One call the session has already answered, in emission order.
///
/// Two kinds of failure, because the two demands read them differently. A tool
/// that refused to run (`errored`) never produced an answer; a command that ran
/// and exited non-zero (`command_failed`) produced an answer that says "no".
/// "Check whether the folder exists" is honestly closed by an `ls` that answered
/// *it does not* — demanding a zero exit there would lock the leaf shut.
#[derive(Debug, Clone, Default)]
pub struct StepRecord {
    pub tool: String,
    pub input: String,
    pub errored: bool,
    pub command_failed: bool,
    /// A passing check whose suite was empty — exit 0 over zero assertions is a
    /// green that proved nothing.
    pub vacuous: bool,
}

impl StepRecord {
    /// The call ran and something came back, whatever it said.
    #[must_use]
    pub fn answered(&self) -> bool {
        !self.errored
    }

    /// The call ran and succeeded.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        !self.errored && !self.command_failed
    }

    /// The call ran, succeeded, and asserted something.
    #[must_use]
    pub fn proved(&self) -> bool {
        self.succeeded() && !self.vacuous
    }
}

/// What a leaf's title demands as proof before it may be claimed `completed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Evidence {
    /// A command the runtime recognizes as the project's own check — build,
    /// test, compile — has to have exited 0 after the last change.
    ExecutedCheck,
    /// A read-only call has to have returned successfully after the last
    /// change: the agent looked at what it says it checked.
    SuccessfulLook,
}

/// Which end of the leaf a plan write is claiming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    /// "This is done" — has to be earned.
    Completed,
    /// "This will not work here" — allowed, but the first time it follows a
    /// failing check with nothing changed since, it is asked once.
    Abandoned,
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
    /// Hard probe gate: armed by a bifurcation crossing, cleared when a step
    /// proves the agent looked before it touched the world. While armed,
    /// mutating calls that were not preceded by a probe in their own step are
    /// refused pre-execution.
    probe_required: bool,
    /// Blind mutations the gate actually blocked (observability).
    probe_gate_trips: usize,
    /// Completion claims refused because no check has passed since the last
    /// change (observability).
    verification_gate_trips: usize,
    /// Leaves whose first `failed` was sent back once for a materially
    /// different try. A node is refused here at most once, ever, so an
    /// environment that genuinely cannot build still has an honest exit.
    give_ups_refused: HashSet<String>,
    /// Give-up claims sent back (observability).
    give_up_gate_trips: usize,
    /// Error tool results collapsed into tombstones by the demon (observability).
    pruned_outputs: usize,
    /// Broken-write backpressure hits (observability).
    syntax_backpressure_hits: usize,
    /// Consecutive steps that moved the plan and nothing else. Resets on any
    /// step that touched the world; escalating past [`MAX_STALL_STEPS`] forces
    /// the orbit out of bookkeeping.
    stalled_steps: usize,
    /// Steps so far that moved the plan and nothing else (observability).
    bookkeeping_steps: usize,
    /// Turn working-set line rendered into Quadrupole/Chaos directives.
    turn_file_summary: Option<String>,
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
            probe_required: false,
            probe_gate_trips: 0,
            verification_gate_trips: 0,
            give_ups_refused: HashSet::new(),
            give_up_gate_trips: 0,
            pruned_outputs: 0,
            syntax_backpressure_hits: 0,
            stalled_steps: 0,
            bookkeeping_steps: 0,
            turn_file_summary: None,
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

    /// Is the hard probe gate currently armed?
    #[must_use]
    pub fn probe_required(&self) -> bool {
        self.probe_required
    }

    /// Blind mutations the gate blocked this turn.
    #[must_use]
    pub fn probe_gate_trips(&self) -> usize {
        self.probe_gate_trips
    }

    /// Completion claims the verification gate refused this turn.
    #[must_use]
    pub fn verification_gate_trips(&self) -> usize {
        self.verification_gate_trips
    }

    /// Give-up claims the gate sent back for one more try.
    #[must_use]
    pub fn give_up_gate_trips(&self) -> usize {
        self.give_up_gate_trips
    }

    /// Error outputs the demon collapsed this turn.
    #[must_use]
    pub fn pruned_outputs(&self) -> usize {
        self.pruned_outputs
    }

    /// Broken-write backpressure hits this turn.
    #[must_use]
    pub fn syntax_backpressure_hits(&self) -> usize {
        self.syntax_backpressure_hits
    }

    /// Consecutive plan-bookkeeping steps at the end of this turn — the streak
    /// the no-progress breaker escalates on.
    #[must_use]
    pub fn stalled_steps(&self) -> usize {
        self.stalled_steps
    }

    /// Steps so far that only redrew the plan (observability, not the live
    /// streak).
    #[must_use]
    pub fn bookkeeping_steps(&self) -> usize {
        self.bookkeeping_steps
    }

    /// Attach the turn working-set line rendered into escalated directives.
    pub fn set_turn_file_summary(&mut self, summary: Option<String>) {
        self.turn_file_summary = summary;
    }

    // -----------------------------------------------------------------------
    // Call classification (probe gate)
    // -----------------------------------------------------------------------

    /// Tools that observe rather than touch: seeing, searching, planning, and
    /// read-only network lookups. The gate deliberately stays wide — blocking a
    /// probe would create the very loop it exists to break — so this is a
    /// conservative allow-list of provably read-only names.
    const PROBE_TOOLS: &'static [&'static str] = &[
        "read_file",
        "list_dir",
        "glob_search",
        "grep_search",
        "search",
        "read",
        "ls",
        "grep",
        "task_graph",
        "taskgraph",
        "skill",
        "toolsearch",
        "tool_search",
        "mcpsearch",
        "mcp_search",
        "retrieve_context",
        "websearch",
        "web_search",
        "webfetch",
        "web_fetch",
        "lsp",
        "status",
        "doctor",
        "help",
        "think",
        "plan",
    ];

    /// Tools that provably touch the workspace.
    const MUTATION_TOOLS: &'static [&'static str] = &[
        "write_file",
        "edit_file",
        "new_file",
        "create_file",
        "write",
        "edit",
    ];

    #[must_use]
    fn bash_command(input: &str) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|value| {
                value
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
    }

    /// True when `command` starts with `program sub` (first tokens; paths like
    /// `./target/debug/cargo test` count as the program).
    fn command_starts_with(command: &str, program: &str, sub: &str) -> bool {
        let mut tokens = command.split_whitespace();
        let Some(first) = tokens.next() else {
            return false;
        };
        let program_ok = first.split('/').next_back() == Some(program);
        program_ok && tokens.next() == Some(sub)
    }

    /// Verifiers: commands that *prove* the current state instead of changing
    /// it (`cargo check`, `git status`, …). These count as probes.
    #[must_use]
    fn is_verifier_command(command: &str) -> bool {
        const VERIFIERS: &[(&str, &str)] = &[
            ("cargo", "check"),
            ("cargo", "test"),
            ("cargo", "build"),
            ("cargo", "clippy"),
            ("cargo", "tree"),
            ("cargo", "fmt"),
            ("go", "vet"),
            ("go", "test"),
            ("go", "build"),
            ("swift", "build"),
            ("swift", "test"),
            ("swift", "run"),
            ("dotnet", "test"),
            ("mvn", "test"),
            ("gradle", "test"),
            ("gradle", "build"),
            ("python3", "test"),
            ("node", "--check"),
            ("npm", "test"),
            ("pnpm", "test"),
            ("yarn", "test"),
            ("tsc", "--noEmit"),
        ];
        if VERIFIERS
            .iter()
            .any(|(program, sub)| Self::command_starts_with(command, program, sub))
        {
            return true;
        }
        if Self::command_starts_with(command, "python3", "-m")
            || Self::command_starts_with(command, "python", "-m")
        {
            return command.contains("py_compile") || command.contains("pytest");
        }
        if matches!(
            command.split_whitespace().next().unwrap_or(""),
            "bash" | "sh" | "zsh"
        ) {
            return command.contains(" -n");
        }
        // Tools that are their own subcommand: with no second token to match,
        // the program name is the whole check.
        if matches!(
            command.split_whitespace().next().unwrap_or(""),
            "xcodebuild" | "make" | "pytest" | "rspec" | "ctest"
        ) {
            return true;
        }
        // An application's own entry point can be its check: `<app> --self-test`
        // exits non-zero when the app cannot even start. The gate's own refusal
        // names "the app itself" as a place to look for evidence, but a path to
        // a built binary matched none of the tool names above, so a run that
        // printed nothing and returned 0 was recorded as "no check has run".
        if command.split_whitespace().skip(1).any(|arg| {
            matches!(
                arg,
                "--self-test" | "--selftest" | "--selfcheck" | "--check" | "--verify"
            )
        }) {
            return true;
        }
        // A project's own build/test entry point is its definition of "the
        // check": `./build.sh` runs `swift build` and can fail, but the gate
        // cannot see inside the script, so it used to report that no check had
        // ever run after a successful build.
        command
            .split_whitespace()
            .next()
            .and_then(|program| program.rsplit('/').next())
            .is_some_and(|script| {
                matches!(
                    script,
                    "build.sh"
                        | "test.sh"
                        | "run-tests.sh"
                        | "run_tests.sh"
                        | "verify.sh"
                        | "check.sh"
                )
            })
    }

    /// Does this bash command provably write to the workspace?
    #[must_use]
    fn bash_is_mutating(command: &str) -> bool {
        if Self::is_verifier_command(command) {
            return false;
        }
        if command.split_whitespace().next() == Some("sed") && command.contains(" -i") {
            return true;
        }
        if command.split_whitespace().any(|token| token == "-delete") {
            return true;
        }
        // Unambiguous write intent, or an output redirection (but not `=>` in
        // awk or an `&>`-style descriptor dup).
        match classify_command(command) {
            CommandIntent::Write | CommandIntent::Destructive | CommandIntent::SystemAdmin => {
                return true;
            }
            CommandIntent::PackageManagement => {
                // cargo/npm/pip install and friends can rewrite the workspace.
                return command.contains(" install") || command.contains(" add");
            }
            _ => {}
        }
        Self::has_output_redirect(command)
    }

    /// Scan for `> file` / `>> file` redirections, skipping `=>`, `&>`-style
    /// descriptor duplications (`2>&1`) and `>=` comparisons.
    #[must_use]
    fn has_output_redirect(command: &str) -> bool {
        let bytes: Vec<char> = command.chars().collect();
        let mut index = 0usize;
        while index < bytes.len() {
            if bytes[index] == '>' {
                let prev = if index > 0 { bytes[index - 1] } else { ' ' };
                if prev == '=' || prev == '<' {
                    index += 1;
                    continue;
                }
                let next = bytes.get(index + 1).copied().unwrap_or(' ');
                match next {
                    '>' => {
                        // `>>` append is a write unless the source is another
                        // descriptor (`2>>` duplicates stderr).
                        let after = bytes.get(index + 2).copied().unwrap_or(' ');
                        if !after.is_ascii_digit() {
                            return true;
                        }
                        index += 2;
                        continue;
                    }
                    '&' | '|' => {
                        index += 1;
                        continue;
                    }
                    ' ' | '\t' => return true,
                    digit if digit.is_ascii_digit() => {
                        let after = bytes.get(index + 2).copied().unwrap_or(' ');
                        if after == '&' {
                            // `2>&1` style: descriptor duplication, not a write.
                            index += 2;
                            continue;
                        }
                        if after.is_ascii_whitespace() {
                            return true;
                        }
                        index += 2;
                        continue;
                    }
                    _ => return true,
                }
            }
            index += 1;
        }
        false
    }

    /// Read-only/observation call that satisfies the probe gate?
    #[must_use]
    pub fn is_probe_call(tool: &str, input: &str) -> bool {
        let lower = tool.to_lowercase();
        if lower == "bash" {
            return Self::bash_is_read_only_probe(input);
        }
        if Self::MUTATION_TOOLS.contains(&lower.as_str()) {
            return false;
        }
        // Re-drawing the plan is not looking at the world. Without this, every
        // `update_status`/`add` on the way to a blind write would discharge the
        // evidence contract, which is how a run marks "folder created" done
        // without ever having read the folder.
        if Self::is_plan_bookkeeping(&lower, input) {
            return false;
        }
        if Self::PROBE_TOOLS.contains(&lower.as_str()) {
            return true;
        }
        // Names with a read/search/get prefix (matches is_read_only_tool).
        Self::is_read_only_tool(&lower) || lower.starts_with("lsp_") || lower.ends_with("_read")
    }

    /// Task-graph bookkeeping: it changes the plan, never the workspace, so it
    /// must not count as forward progress and must not satisfy the probe gate.
    /// Only the read-only `view` operation is an observation.
    #[must_use]
    pub fn is_plan_bookkeeping(tool: &str, input: &str) -> bool {
        let lower = tool.to_lowercase();
        if !Self::is_plan_tool(&lower) {
            return false;
        }
        let operation = serde_json::from_str::<serde_json::Value>(input)
            .ok()
            .and_then(|value| {
                value
                    .get("operation")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        !operation.eq_ignore_ascii_case("view")
    }

    fn bash_is_read_only_probe(command_json: &str) -> bool {
        let Some(command) = Self::bash_command(command_json) else {
            return false;
        };
        if Self::is_verifier_command(&command) {
            return true;
        }
        // The allowlist `classify_command` serves is the one that decides what may
        // run unattended; it was never meant to decide what counts as looking at
        // the machine. A toolchain probe the list does not know — `swiftc
        // --version`, `system_profiler`, `sw_vers` — still answers a question
        // about the environment, and a live run was locked out for four turns
        // because a `swiftc --version` that had just succeeded could not prove
        // the task titled "Check Swift/Xcode availability": the gate said nothing
        // had been looked at, the graph refused to advance, and every action that
        // could fix either sat behind that refusal.
        Self::is_observation_command(&command)
    }

    /// A command that reads the machine and provably changes nothing in it.
    ///
    /// An opaque program is not an observation, whatever its name suggests: a
    /// script path or an interpreter handed inline code can write anything, and
    /// the command text does not say which — one opaque segment makes the whole
    /// chain opaque. Plan enforcement asks this question directly, because a
    /// build is a look at the code but is still an action the plan has to own.
    #[must_use]
    pub fn is_observation_command(command: &str) -> bool {
        const OPAQUE_INTERPRETERS: [&str; 8] = [
            "bash",
            "sh",
            "zsh",
            "python",
            "python3",
            "node",
            "ruby",
            "osascript",
        ];
        if Self::bash_is_mutating(command) {
            return false;
        }
        if command
            .split([';', '|', '&', '\n'])
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .any(|segment| {
                let first = segment.split_whitespace().next().unwrap_or_default();
                first.contains('/')
                    || first.contains('\\')
                    || first.starts_with('.')
                    || first.starts_with('~')
                    || first.ends_with(".sh")
                    || OPAQUE_INTERPRETERS.contains(&first)
            })
        {
            return false;
        }
        // A command the allowlist has never heard of is only a look while nothing
        // in it says otherwise: `docker compose down` and `defaults write …` are
        // as unknown to the list as `system_profiler` is, and they change state.
        // A command the allowlist has never heard of is only a look while nothing
        // in it says otherwise: `docker compose down` and `defaults write …` are
        // as unknown to the list as `system_profiler` is, and they change state.
        let intent = classify_command(command);
        if intent == CommandIntent::Unknown
            && command.split_whitespace().any(|token| {
                matches!(
                    token.to_lowercase().as_str(),
                    "add"
                        | "apply"
                        | "build"
                        | "clean"
                        | "commit"
                        | "compile"
                        | "create"
                        | "delete"
                        | "deploy"
                        | "disable"
                        | "down"
                        | "edit"
                        | "enable"
                        | "exec"
                        | "init"
                        | "install"
                        | "kill"
                        | "launch"
                        | "migrate"
                        | "mount"
                        | "prune"
                        | "publish"
                        | "remove"
                        | "reset"
                        | "restart"
                        | "run"
                        | "set"
                        | "start"
                        | "stop"
                        | "uninstall"
                        | "update"
                        | "write"
                )
            })
        {
            return false;
        }
        matches!(intent, CommandIntent::ReadOnly | CommandIntent::Unknown)
    }

    /// Call that provably touches the workspace and therefore needs a probe
    /// first while the gate is armed. Unknown tools are deliberately *not*
    /// mutations: the gate blocks blind writes, it does not invent new failure
    /// modes for tools it cannot classify.
    #[must_use]
    pub fn is_mutating_call(tool: &str, input: &str) -> bool {
        let lower = tool.to_lowercase();
        if lower == "bash" {
            let Some(command) = Self::bash_command(input) else {
                return false;
            };
            return Self::bash_is_mutating(&command);
        }
        Self::MUTATION_TOOLS.contains(&lower.as_str())
    }

    /// Call that runs the project's own check — a build, a test suite, a compile.
    /// It is the only evidence that a task named "Test the application" means
    /// more than an intention, so it is what the verification gate below asks
    /// for. Only `bash` can carry one: a tool that reads a file proves the file
    /// exists, never that the program works.
    ///
    /// Chained commands count segment by segment — `cd App && swift build` is
    /// the same build as bare `swift build` — but a segment with a pipe does
    /// not: `swift build | tail -5` exits with `tail`'s status, so it reports
    /// success for a build that failed. That is the exact lie this gate exists
    /// to catch, so a piped check proves nothing until it is run plainly.
    #[must_use]
    pub fn is_verifier_call(tool: &str, input: &str) -> bool {
        if tool.to_lowercase() != "bash" {
            return false;
        }
        let Some(command) = Self::bash_command(input) else {
            return false;
        };
        command
            .split([';', '\n'])
            .flat_map(|segment| segment.split("&&"))
            .map(str::trim)
            .filter(|segment| !segment.is_empty() && !segment.contains('|'))
            .any(Self::is_verifier_command)
    }

    /// Did a shell call run and *not* succeed?
    ///
    /// The tool's own error flag says the tool refused to run. A command that
    /// *did* run and exited non-zero is the failure the gate cares about most
    /// and that flag never sees: `bash` reports it inside an otherwise
    /// successful result as `returnCodeInterpretation`. Anything the gate
    /// cannot read as a clean exit — unparsable output, an interrupted command,
    /// a command pushed to the background — counts as not succeeding, which
    /// costs one re-run rather than one false green.
    #[must_use]
    pub fn command_failed(tool: &str, output: &str) -> bool {
        if tool.to_lowercase() != "bash" {
            return false;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
            return true;
        };
        if value
            .get("interrupted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return true;
        }
        if value
            .get("backgroundTaskId")
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            return true;
        }
        value
            .get("returnCodeInterpretation")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| !status.is_empty())
    }

    /// Did a passing check assert anything at all?
    ///
    /// Run 6 closed "Test the application." on `swift test` exiting 0 over the
    /// template's placeholder suite — "Executed 0 tests, with 0 failures". The
    /// exit code is honest and the claim is still empty: nothing was tested. A
    /// green that asserts nothing is the softest false green there is, so the
    /// evidence has to say a check ran *and* something ran inside it.
    #[must_use]
    pub fn vacuous_check(tool: &str, output: &str) -> bool {
        const EMPTY: &[&str] = &[
            "executed 0 tests",
            "0 tests executed",
            "no tests ran",
            "no tests were executed",
            "no test files",
            "ran 0 tests",
            "tests run: 0",
            "test result: ok. 0 passed",
        ];
        if tool.to_lowercase() != "bash" {
            return false;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(output) else {
            return false;
        };
        let text = format!(
            "{}\n{}",
            value
                .get("stdout")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            value
                .get("stderr")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        )
        .to_lowercase();
        EMPTY.iter().any(|marker| text.contains(marker))
    }

    /// Task-graph bookkeeping tools by their effective names.
    #[must_use]
    fn is_plan_tool(lower: &str) -> bool {
        matches!(lower, "task_graph" | "taskgraph" | "taskgraphwrite")
    }

    /// Nodes this call moves to a status the gates police, with the title when
    /// the call carries one.
    ///
    /// `update_status` is the honest form. `add` can carry the same claim inside
    /// the node, which is the same lie told one operation earlier, so both land
    /// in the same gate — the title is right there on an `add` and has to be
    /// read back from history on a `update_status`.
    #[must_use]
    fn status_claims(input: &str) -> Vec<(String, Option<String>, Claim)> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(input) else {
            return Vec::new();
        };
        let Some(operation) = value
            .get("operation")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            return Vec::new();
        };
        let claims_status = operation.eq_ignore_ascii_case("update_status");
        let creates_nodes = operation.eq_ignore_ascii_case("add");
        if !claims_status && !creates_nodes {
            return Vec::new();
        }
        value
            .get("nodes")
            .and_then(serde_json::Value::as_array)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|node| {
                        let status = node.get("status").and_then(serde_json::Value::as_str)?;
                        let claim = if status.eq_ignore_ascii_case("completed") {
                            Claim::Completed
                        } else if status.eq_ignore_ascii_case("failed") {
                            Claim::Abandoned
                        } else {
                            return None;
                        };
                        let id = node.get("id").and_then(serde_json::Value::as_str)?;
                        let title = if creates_nodes {
                            node.get("content")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                        } else {
                            None
                        };
                        Some((id.to_string(), title, claim))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Title a node was created with, read back from the `add` calls in the
    /// executed history. `None` when this process never watched the node being
    /// created — a resumed session, or a plan written straight into the store —
    /// and the gate treats an unknown title as not check-shaped rather than
    /// blocking on a guess.
    #[must_use]
    fn title_from_history(id: &str, history: &[StepRecord]) -> Option<String> {
        for record in history {
            if !Self::is_plan_tool(&record.tool.to_lowercase()) {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&record.input) else {
                continue;
            };
            if !value
                .get("operation")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|operation| operation.eq_ignore_ascii_case("add"))
            {
                continue;
            }
            let Some(nodes) = value.get("nodes").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for node in nodes {
                if node.get("id").and_then(serde_json::Value::as_str) == Some(id) {
                    if let Some(content) = node.get("content").and_then(serde_json::Value::as_str) {
                        return Some(content.to_string());
                    }
                }
            }
        }
        None
    }

    /// Does the title claim work that has to be proven, and by what?
    ///
    /// Titles are imperative and English by mandate, so the first word is the
    /// verb. Only that verb position is read: "Runbook for the release" is
    /// documentation, not a run.
    ///
    /// Two kinds of proof, because one demand fits every title badly. "Test /
    /// Build / Run …" is a claim about the *program* and only an executed check
    /// can back it. "Verify / Check the folder" is a claim about what the agent
    /// saw, and `ls` returning after the `mkdir` is the honest evidence —
    /// demanding a compiler for it would lock the leaf shut and turn the gate
    /// into the treadmill it exists to prevent.
    ///
    /// A program word anywhere in the title wins, so "Verify the app builds." is
    /// not laundered by an earlier `ls`: the look tier only applies when the
    /// title never promises a run.
    #[must_use]
    fn required_evidence(title: &str) -> Option<Evidence> {
        const EXECUTED_CHECK: &[&str] = &[
            "test",
            "tests",
            "testing",
            "build",
            "builds",
            "building",
            "rebuild",
            "rebuilding",
            "compile",
            "compiles",
            "compiling",
            "run",
            "runs",
            "launch",
            "launches",
            "install",
            "installs",
            "deploy",
            "deploys",
            "benchmark",
            "benchmarks",
            "smoke",
        ];
        const SUCCESSFUL_LOOK: &[&str] = &[
            "verify",
            "verifies",
            "validate",
            "validates",
            "check",
            "checks",
            "confirm",
            "confirms",
        ];
        let words: Vec<String> = title
            .split_whitespace()
            .map(|word| {
                word.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .collect();
        if words
            .iter()
            .any(|word| EXECUTED_CHECK.contains(&word.as_str()))
        {
            return Some(Evidence::ExecutedCheck);
        }
        if words
            .first()
            .is_some_and(|word| SUCCESSFUL_LOOK.contains(&word.as_str()))
        {
            return Some(Evidence::SuccessfulLook);
        }
        None
    }

    /// Claims gate: a leaf whose title names a check may not be claimed
    /// `completed` until that check has passed *after* the last change to the
    /// workspace, and the same leaf may not be abandoned on the first failing
    /// run without one try that changed something.
    ///
    /// `history` is every call this session has already answered, in emission
    /// order. A call the tool refused never counts as proof; a command that ran
    /// and exited non-zero counts as proof only for a title that asks what the
    /// agent *saw*, not for one that asks whether the program works.
    ///
    /// This is the last cheap place to refuse. A run writes a source file, marks
    /// "Test the application" completed, closes the graph green, and reports
    /// success over a build that does not compile — because nothing tied the
    /// claim to an executed check. The evidence has to be ordered too: a check
    /// that passed before the edit says nothing about the edit, and a check
    /// emitted in this same step has not returned yet, so it proves nothing
    /// either.
    ///
    /// The give-up half is the same failure seen from the other side: one
    /// non-zero exit, a `failed`, and a run that ends with the error it could
    /// have fixed still in its own transcript. Asking once costs a step; not
    /// asking costs the task.
    ///
    /// Narrow on purpose: only check-shaped titles, only nodes this process can
    /// name a title for, and `failed` is refused at most once per node — an
    /// environment that cannot build has an honest exit, so neither gate can
    /// become the treadmill it exists to prevent.
    pub fn gate_claims_for_step(
        &mut self,
        calls: &[(String, String)],
        history: &[StepRecord],
    ) -> Vec<(usize, String)> {
        let last_mutation = Self::last_change_position(history);
        let mut refusals = Vec::new();
        for (index, (tool, input)) in calls.iter().enumerate() {
            if !Self::is_plan_tool(&tool.to_lowercase()) {
                continue;
            }
            for (id, inline_title, claim) in Self::status_claims(input) {
                let title = match inline_title {
                    Some(title) => title,
                    None => match Self::title_from_history(&id, history) {
                        Some(title) => title,
                        None => continue,
                    },
                };
                let Some(demand) = Self::required_evidence(&title) else {
                    continue;
                };
                match claim {
                    Claim::Completed => {
                        // A change queued ahead of this claim in the same step
                        // lands after any check that has already returned, so
                        // the evidence is stale before the step is even executed.
                        let mutation_ahead = calls[..index]
                            .iter()
                            .any(|(tool, input)| Self::is_mutating_call(tool, input));
                        let proof = Self::last_evidence_position(demand, history);
                        let earned = !mutation_ahead
                            && proof.is_some_and(|seen| {
                                last_mutation.is_none_or(|change| seen > change)
                            });
                        if earned {
                            continue;
                        }
                        self.verification_gate_trips += 1;
                        refusals.push((
                            index,
                            Self::verification_gate_refusal(
                                &id,
                                &title,
                                demand,
                                mutation_ahead,
                                history,
                            ),
                        ));
                    }
                    Claim::Abandoned => {
                        if demand != Evidence::ExecutedCheck || self.give_ups_refused.contains(&id)
                        {
                            continue;
                        }
                        let Some(failure) = Self::first_check_failure_position(history) else {
                            continue;
                        };
                        if Self::tried_again_since(failure, history) {
                            continue;
                        }
                        self.give_ups_refused.insert(id.clone());
                        self.give_up_gate_trips += 1;
                        refusals
                            .push((index, Self::give_up_refusal(&id, &title, failure, history)));
                    }
                }
            }
        }
        refusals
    }

    /// The last call that can serve as evidence for this demand. One definition,
    /// used by both the decision and the refusal text, so the advice can never
    /// describe an evidence the gate did not actually look for.
    #[must_use]
    fn last_evidence_position(demand: Evidence, history: &[StepRecord]) -> Option<usize> {
        history.iter().rposition(|record| match demand {
            Evidence::ExecutedCheck => {
                record.proved() && Self::is_verifier_call(&record.tool, &record.input)
            }
            Evidence::SuccessfulLook => {
                record.answered() && Self::is_probe_call(&record.tool, &record.input)
            }
        })
    }

    /// A passing check that asserted nothing, most recently — the reason the
    /// refusal can say "you ran it and it tested nothing" instead of "you never
    /// ran it", which would be a lie the model can disprove in one scroll.
    #[must_use]
    fn last_vacuous_check_position(history: &[StepRecord]) -> Option<usize> {
        history.iter().rposition(|record| {
            record.succeeded()
                && record.vacuous
                && Self::is_verifier_call(&record.tool, &record.input)
        })
    }

    /// The last call that actually changed the workspace.
    #[must_use]
    fn last_change_position(history: &[StepRecord]) -> Option<usize> {
        history.iter().rposition(|record| {
            record.succeeded() && Self::is_mutating_call(&record.tool, &record.input)
        })
    }

    /// The first check that ran and reported a non-zero exit — the attempt the
    /// agent is about to call hopeless.
    #[must_use]
    fn first_check_failure_position(history: &[StepRecord]) -> Option<usize> {
        history.iter().position(|record| {
            record.answered()
                && record.command_failed
                && Self::is_verifier_call(&record.tool, &record.input)
        })
    }

    /// Did anything happen after that failing check that could change its
    /// answer — a touched file, or the check run again?
    #[must_use]
    fn tried_again_since(failure: usize, history: &[StepRecord]) -> bool {
        history.iter().skip(failure + 1).any(|record| {
            (record.succeeded() && Self::is_mutating_call(&record.tool, &record.input))
                || (record.answered() && Self::is_verifier_call(&record.tool, &record.input))
        })
    }

    /// Refusal text for a first give-up: what was tried, what has not changed
    /// since, and the one thing that would make a repeat mean something.
    #[must_use]
    fn give_up_refusal(id: &str, title: &str, failure: usize, history: &[StepRecord]) -> String {
        let record = &history[failure];
        let detail = Self::bash_command(&record.input).unwrap_or_else(|| record.input.clone());
        let detail = detail.chars().take(72).collect::<String>();
        format!(
            "[SYSTEM DIRECTIVE — GIVE-UP GATE]\n\
             Task '{id}' (\"{title}\") is being abandoned, but the check its own title names ran \
             once (`{}` {detail}) and nothing in the workspace has changed since that run.\n\
             The same command over the same files returns the same answer, so a second `failed` \
             without a change in between is the same attempt counted twice.\n\
             Read the output of that run and change the cause it names — a missing manifest, the \
             wrong directory, a path, a flag, a dependency — then let the check run again.\n\
             If no such fix exists in this environment, re-issue this exact call: a give-up is \
             asked once and granted the second time, and a `failed` with a reason is worth more \
             than a green graph over a build that does not compile.",
            record.tool
        )
    }

    /// Refusal text for an unearned completion claim, naming the evidence that
    /// is missing rather than the rule that was broken.
    #[must_use]
    fn verification_gate_refusal(
        id: &str,
        title: &str,
        demand: Evidence,
        mutation_ahead: bool,
        history: &[StepRecord],
    ) -> String {
        let noun = match demand {
            Evidence::ExecutedCheck => "check",
            Evidence::SuccessfulLook => "read-only look",
        };
        let how_to_prove = match demand {
            Evidence::ExecutedCheck => {
                "Run the check now — the command this project actually uses (`swift build`, \
                 `cargo test`, `npm test`, `pytest`, the app itself) — let it return, and only \
                 then re-issue this same `update_status`."
            }
            Evidence::SuccessfulLook => {
                "Run the read-only command that answers this title now (`ls`, `read_file`, `git \
                 status`, the file you just wrote) — let it return, and only then re-issue this \
                 same `update_status`."
            }
        };
        let describe = |index: usize| -> String {
            let record = &history[index];
            let detail = Self::bash_command(&record.input).unwrap_or_else(|| record.input.clone());
            let detail = detail.chars().take(72).collect::<String>();
            format!("`{}` {detail}", record.tool)
        };
        let stale = "A change is being applied in this same step, so any proof that has already \
             returned describes the old workspace.";
        let empty_suite = match (
            Self::last_evidence_position(demand, history),
            Self::last_vacuous_check_position(history),
            Self::last_change_position(history),
        ) {
            (None, Some(vacuous), change) if change.is_none_or(|seen| vacuous > seen) => {
                Some(format!(
                "The check did run ({}) and exited 0, but it asserted nothing — an empty suite \
                 passing is the greenest kind of nothing. Write the test the title implies, in the \
                 place this project's runner looks for it (a file outside the declared targets is \
                 never compiled and never run), let the check run over it, and then make the \
                 claim.",
                describe(vacuous)
            ))
            }
            _ => None,
        };
        let evidence = empty_suite.unwrap_or_else(|| {
            match (
                Self::last_evidence_position(demand, history),
                Self::last_change_position(history),
            ) {
                (None, None) => format!(
                "Nothing that could prove it has run — no {noun} in this session, and no change \
                 to the workspace either."
            ),
                (None, Some(change)) => format!(
                "The workspace changed ({}) and no {noun} has run since. No {noun} has succeeded \
                 at any point in this session.",
                describe(change)
            ),
                (Some(seen), Some(change)) if seen > change => stale.to_string(),
                (Some(seen), Some(change)) => format!(
                    "The last passing {noun} ({}) came BEFORE the last change ({}), so it says \
                 nothing about what is there now.",
                    describe(seen),
                    describe(change)
                ),
                (Some(_), None) if mutation_ahead => stale.to_string(),
                (Some(seen), None) => format!(
                "A passing {noun} ({}) is on record, but this session has no change to compare \
                 it against — run the {noun} the title names and let it return.",
                describe(seen)
            ),
            }
        });
        format!(
            "[SYSTEM DIRECTIVE — VERIFICATION GATE]\n\
             Task '{id}' (\"{title}\") claims completion, but the {noun} its own title names has \
             not passed since the workspace last changed.\n\
             {evidence}\n\
             {how_to_prove}\n\
             Do not answer by editing more files: an edit is not evidence. If this environment \
             cannot satisfy the title, mark '{id}' `failed` and say why — that closes the branch \
             honestly, and it is never refused twice."
        )
    }

    /// Maxwell's-demon gate for one step's calls, evaluated in emission order:
    /// a probe emitted before a mutation proves the agent looked, so the
    /// mutation runs; a mutation with no probe yet in the step is blind and gets
    /// its index returned so the caller can refuse it pre-execution.
    ///
    /// Arming is done by a bifurcation crossing in [`Self::observe`]. Disarming
    /// is *not* decided here: at this point the probe has not run yet, and
    /// `read_file` on a path that does not exist is the orbit itself, not the
    /// escape from it. [`Self::observe`] discharges the contract once the step's
    /// probe has actually returned something.
    pub fn gate_mutations_for_step(&mut self, calls: &[(String, String)]) -> Vec<usize> {
        if !self.probe_required {
            return Vec::new();
        }
        let mut blocked = Vec::new();
        let mut probe_seen = false;
        for (index, (tool, input)) in calls.iter().enumerate() {
            if Self::is_probe_call(tool, input) {
                // A probe ahead of the mutation satisfies this step; the gate
                // stays armed until the probe's outcome is known.
                probe_seen = true;
                continue;
            }
            if Self::is_mutating_call(tool, input) && !probe_seen {
                blocked.push(index);
                self.probe_gate_trips += 1;
                self.refused_this_step
                    .push(Self::fingerprint(tool, "probe_gate"));
            }
        }
        blocked
    }

    /// Refusal text handed back as an `is_error` tool result for a
    /// gate-blocked mutation.
    #[must_use]
    pub fn probe_gate_refusal(tool: &str) -> String {
        format!(
            "[SYSTEM DIRECTIVE — BIFURCATION: PROBE GATE]\n\
             `{tool}` was blocked before execution: the runtime is past the first \
             bifurcation and this step emitted no read-only probe ahead of the mutation. \
             Blind writes are exactly the orbit the ladder just closed on you.\n\
             Emit any one of these first (they are never blocked by the gate), then re-issue \
             the change with what you learned:\n\
             - read_file / grep_search / glob_search / list_dir on the target or its consumers\n\
             - TaskGraph operation 'view' (plan state only — `update_status`/`add` do not count \
             as looking) or Skill / ToolSearch / McpSearch / retrieve_context\n\
             - WebSearch / WebFetch for external contracts\n\
             - lsp_* diagnostics, or `bash` read-only/verifier commands: cargo check|test|build|clippy, \
             go vet|test, tsc --noEmit, python3 -m py_compile, bash -n, git status|diff|log|show, ls, cat, find\n\
             The probe has to return something: a read that errors is another point of the same \
             orbit and keeps the gate armed. Pick a path you expect to exist.\n\
             The probe is not a formality: name in one line what you expect to see and what \
             would falsify your current hypothesis."
        )
    }

    /// Re-arm the gate explicitly (used by callers that want the contract from
    /// the first non-laminar step even without a fresh crossing).
    pub fn arm_probe_gate(&mut self) {
        self.probe_required = true;
    }

    /// Re-establish the "look before you touch" contract at the start of a new
    /// user turn. The regime is session-scoped: a turn that begins while the
    /// orbit is still past the first bifurcation is exactly the one that must
    /// not be allowed a blind write, so the gate carries over instead of being
    /// cleared. Discharging it takes a probe that returned something (see
    /// [`Self::observe`]) or the orbit genuinely re-converging to laminar.
    pub fn rearm_probe_gate_for_turn(&mut self) {
        self.probe_required = self.regime > Regime::Laminar;
    }

    /// The executable meaning of a call, with the fields a model varies between
    /// two otherwise identical attempts removed and object keys canonicalized.
    ///
    /// `description` is a comment on intent, not part of the action: re-issuing a
    /// failed command with a new one is the same orbit, and a gate keyed on the
    /// raw JSON lets a run retry a dead call forever by editing its caption.
    /// Unparseable input is returned unchanged — the byte compare is then exactly
    /// what it was before.
    #[must_use]
    pub fn operative_input(input: &str) -> String {
        const INCIDENTAL_KEYS: &[&str] = &[
            "description",
            "task_description",
            "reason",
            "note",
            "notes",
            "label",
            "intent",
            "explanation",
        ];
        let Ok(value) = serde_json::from_str::<serde_json::Value>(input) else {
            return input.to_string();
        };
        let Some(map) = value.as_object() else {
            return input.to_string();
        };
        let mut parts: Vec<String> = map
            .iter()
            .filter(|(key, _)| !INCIDENTAL_KEYS.contains(&key.as_str()))
            .map(|(key, value)| {
                // serde_json sorts object keys, so two hand-written variants of
                // the same call collapse onto one signature.
                let canonical = serde_json::to_string(value).unwrap_or_default();
                format!("{key}={canonical}")
            })
            .collect();
        parts.sort();
        parts.join("&")
    }

    #[must_use]
    pub fn attempt_key(tool: &str, input: &str) -> AttemptKey {
        let mut hasher = Sha256::new();
        hasher.update(Self::operative_input(input).as_bytes());
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
        // A plan redraw is not a new world. Counting it here is what let a run
        // tick `update_status` between two identical failures and call the retry
        // fresh evidence.
        if !Self::is_read_only_tool(tool) && !Self::is_plan_bookkeeping(tool, input) {
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
                .is_some_and(|(_, category)| Self::is_transient_category(category));

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
            self.stalled_steps = 0;
        } else if sample.syntax_backpressure {
            // The file on disk is broken. The world did change, but a corrupted
            // artifact is not convergence: resistance is not discounted, and the
            // model is told which line to fix in the tool output itself.
            self.syntax_backpressure_hits += 1;
            self.stalled_steps = 0;
        } else if sample.bookkeeping && !sample.mutated_state && sample.successes.is_empty() {
            // Redrawing the plan is not doing the work. Every call succeeded, so
            // the naive reading is "smooth progress" — that is exactly the
            // limit cycle this branch exists to break: a run can spend its whole
            // turn on `update_status` and leave nothing behind.
            self.stalled_steps += 1;
            self.bookkeeping_steps += 1;
            if self.stalled_steps >= MAX_STALL_STEPS {
                self.pressure += NOVEL_STEP_PRESSURE;
                self.cycle_floor = self.cycle_floor.max(Regime::Dichotomy);
            }
        } else if sample.mutated_state || !sample.successes.is_empty() {
            // Clean forward step: contract the orbit by α.
            self.stalled_steps = 0;
            self.pressure /= FEIGENBAUM_ALPHA;
            self.cycle_floor = Regime::Laminar;
        } else {
            self.stalled_steps = 0;
        }

        self.seen_fingerprints
            .extend(sample.failures.iter().cloned().chain(refused));

        let before = self.regime;
        let derived = self
            .thresholds
            .regime_at(self.pressure)
            .max(self.cycle_floor);
        // The cascade advances at most one doubling per iteration. Without this
        // clamp the later gaps (Δ/δⁿ) are smaller than a single failing step and
        // the ladder would jump straight to chaos, never showing the intermediate
        // thinking modes it exists to force.
        self.regime = if derived > before {
            Regime::from_level(before.level() + 1)
        } else {
            derived
        };

        if self.probe_required && sample.successful_probe {
            // The demon rests on evidence, not on intent: a probe that returned
            // something is a look, one that errored is another point of the same
            // orbit. Discharging here (after execution) is what makes a failed
            // `read_file` unable to buy a blind write in the next step.
            self.probe_required = false;
        }

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
            // A crossing means the last orbits were blind: from here a mutation
            // may only follow a probe, until the agent proves it can look.
            self.probe_required = true;
            tracing::warn!(
                iteration = sample.iteration,
                regime = self.regime.as_str(),
                pressure = (self.pressure * 1000.0).round() / 1000.0,
                chaos_steps = self.chaos_steps,
                "bifurcation crossed: forcing a wider thinking mode"
            );
            return Some(self.regime);
        }
        if self.regime == Regime::Laminar {
            // The agent is converging again; the contract rests until the next
            // bifurcation.
            self.probe_required = false;
        }
        None
    }

    fn known_fingerprint(&self, fingerprint: &str) -> bool {
        self.seen_fingerprints.contains(fingerprint)
    }

    // -----------------------------------------------------------------------
    // Dead-orbit context pruning (Maxwell's demon)
    // -----------------------------------------------------------------------

    /// Ledger evidence for a prunable orbit: `attempts >= 2` or the failure
    /// fingerprint already closed in a previous step. The category comes from
    /// the entry itself, so a tombstone only ever cites a real observed failure.
    fn dead_orbit_stats(&self, tool: &str) -> Option<(usize, String)> {
        self.ledger
            .values()
            .filter(|entry| entry.tool == tool && !entry.category.is_empty())
            .max_by_key(|entry| entry.attempts)
            .map(|entry| {
                (
                    entry.attempts.max(MIN_ATTEMPTS_BEFORE_REFUSAL),
                    entry.category.clone(),
                )
            })
    }

    /// Collapse stale error `tool_result` blocks of the current turn into
    /// one-line tombstones. Called after a bifurcation crossing: the orbits
    /// these outputs describe are provably dead, and letting the model re-read
    /// the same stacktrace every iteration is how attention drifts.
    ///
    /// Contract: only the `output` text is rewritten. `tool_use_id`, `tool_name`
    /// and `is_error` are preserved so the API pairing of tool_use/tool_result
    /// never breaks. The window is the current turn (after the last user
    /// message); the newest tool result is kept whole; refusal directives are
    /// never touched (they are live instructions, not dead evidence).
    pub fn prune_dead_orbit_outputs(&mut self, messages: &mut [ConversationMessage]) -> usize {
        let window_start = messages
            .iter()
            .rposition(|message| message.role == MessageRole::User)
            .map_or(0, |index| index + 1);
        let keep_message = messages
            .iter()
            .skip(window_start)
            .rposition(|message| {
                message
                    .blocks
                    .iter()
                    .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
            })
            .map(|index| window_start + index);

        let mut pruned = 0usize;
        for (message_index, message) in messages.iter_mut().enumerate().skip(window_start) {
            if Some(message_index) == keep_message {
                // The newest tool result is the one the model is reacting to
                // right now — it stays whole.
                continue;
            }
            for block in &mut message.blocks {
                let ContentBlock::ToolResult {
                    tool_name,
                    output,
                    is_error,
                    ..
                } = block
                else {
                    continue;
                };
                if !*is_error || output.len() < TOMBSTONE_MIN_OUTPUT_CHARS {
                    continue;
                }
                if output.starts_with("[🗑️") || output.starts_with("[SYSTEM DIRECTIVE") {
                    continue;
                }
                let Some(stats) = self.dead_orbit_stats(tool_name) else {
                    continue;
                };
                *output = self.tombstone(output, stats);
                pruned += 1;
            }
        }
        self.pruned_outputs += pruned;
        if pruned > 0 {
            tracing::info!(
                pruned,
                regime = self.regime.as_str(),
                "dead-orbit outputs collapsed by the runtime demon"
            );
        }
        pruned
    }

    fn tombstone(&self, output: &str, stats: (usize, String)) -> String {
        let (attempts, category) = stats;
        format!(
            "[🗑️ Dead orbit collapsed by runtime (regime={}, attempts={attempts}, category=\
             \"{category}\"): \"{excerpt}\". Full stacktrace purged to prevent attention drift.]",
            self.regime.as_str(),
            excerpt = Self::first_meaningful_line(output),
        )
    }

    /// First line with real content — skips JSON/braces noise and blanks —
    /// truncated to [`TOMBSTONE_EXCERPT_CHARS`].
    fn first_meaningful_line(output: &str) -> String {
        let mut line = output
            .lines()
            .map(str::trim)
            .find(|candidate| {
                !candidate.is_empty()
                    && !candidate
                        .chars()
                        .all(|ch| matches!(ch, '{' | '}' | '[' | ']' | ',' | '"'))
            })
            .unwrap_or_else(|| output.trim())
            .replace('"', "'");
        line = line.replace('\n', " ");
        if line.chars().count() > TOMBSTONE_EXCERPT_CHARS {
            let cut = line
                .char_indices()
                .nth(TOMBSTONE_EXCERPT_CHARS)
                .map_or(line.len(), |(index, _)| index);
            line.truncate(cut);
            line.push('…');
        }
        line
    }

    // -----------------------------------------------------------------------
    // Directives
    // -----------------------------------------------------------------------

    /// System-prompt section for the current regime, or `None` while the agent is
    /// converging. Kept short and factual: it states what was measured, then what
    /// to do about it.
    #[must_use]
    pub fn directive_section(&self) -> Option<String> {
        let measured =
            format!(
            "Measured by the runtime: pressure r={:.3}, thresholds r₁={:.3} r₂={:.3} r∞={:.3}, \
             step #{}, mutation epoch {}.",
            self.pressure, self.thresholds.r1, self.thresholds.r2, self.thresholds.r_infinity,
            self.steps, self.mutation_epoch,
        );
        let blocked = self.dead_orbits_line();
        let working_set = self.working_set_line();
        let stall = self.stall_line();
        let body = match self.regime {
            Regime::Laminar => {
                if self.stalled_steps == 0 {
                    return None;
                }
                // A plan-only streak is the one orbit worth naming before there is
                // any pressure to escalate on: by the time the ladder has resistance
                // the model has already narrated four green checkmarks it cannot show.
                format!(
                    "## ⏳ PLAN-ONLY STREAK (runtime-measured)\n\
                     {measured}\n\
                     {stall}"
                )
            }
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
                 {measured}{blocked}{working_set}\n\
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
                 and add it to TaskGraph as the new active sub-task.{regression}",
                regression = self.regression_advisory(),
            ),
            Regime::Chaos => format!(
                "## 💥 BIFURCATION L3 — CHAOS POINT (r ≥ r∞, runtime-enforced)\n\
                 {measured}{blocked}{working_set}\n\
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
        if self.stalled_steps > 0 && self.regime > Regime::Laminar {
            return Some(format!("{body}\n{stall}"));
        }
        Some(body)
    }

    /// The plan moved; the world did not. Names the streak and its length, then
    /// demands one world action before any further graph write — a model that is
    /// busy ticking boxes has no other signal that it is stalling.
    fn stall_line(&self) -> String {
        if self.stalled_steps == 0 {
            return String::new();
        }
        format!(
            "Measured by the runtime: {} consecutive step{s} redrew the task graph \
             (`update_status`/`add`) and touched no file, command, or artifact. Planning is not \
             progress; the ladder counts a plan-only streak as resistance, and status flags \
             marked without an artifact behind them are false accounting.\n\
             Next action must be a world action, not a graph write: run the smallest command or \
             make the smallest edit the active leaf asks for, then read back what it actually \
             produced (`ls`, `cat`, a compile, a test). Only once that result is in front of you \
             may the leaf be marked completed — evidence first, status second.",
            self.stalled_steps,
            s = if self.stalled_steps == 1 { "" } else { "s" },
        )
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
            if regressed.len() == 1 {
                "tool"
            } else {
                "tools"
            },
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

    /// Transactional working memory (M4): what this turn has already touched,
    /// rendered only when the agent is escalated — below the second bifurcation
    /// the model still has its own tool results in context.
    fn working_set_line(&self) -> String {
        match self.regime {
            Regime::Quadrupole | Regime::Chaos => {
                self.turn_file_summary
                    .as_deref()
                    .map_or_else(String::new, |summary| {
                        format!("\nModified files in this turn since last clean state: {summary}")
                    })
            }
            _ => String::new(),
        }
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
            format!(
                "Maxwell demon: pruned_outputs={} probe_gate={} trips={} verification_gate={} give_up_gate={} syntax_backpressure_hits={}",
                self.pruned_outputs,
                if self.probe_required { "armed" } else { "open" },
                self.probe_gate_trips,
                self.verification_gate_trips,
                self.give_up_gate_trips,
                self.syntax_backpressure_hits,
            ),
            format!(
                "No-progress breaker: plan_only_steps={bookkeeping} current_streak={streak} (escalates at {limit})",
                bookkeeping = self.bookkeeping_steps,
                streak = self.stalled_steps,
                limit = MAX_STALL_STEPS,
            ),
        ];
        lines.extend(self.thresholds.summary_lines());
        for (step, regime) in &self.crossings {
            lines.push(format!(
                "  crossed → {} at iteration {step}",
                regime.as_str()
            ));
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
