# Feigenbaum Fractal Architecture (δ ≈ 4.669, α ≈ 2.503) in Rust

## Overview

The **Feigenbaum Fractal Architecture** applies the universal period-doubling
bifurcation constant ($\delta \approx 4.669201609102991$) and orbit scaling
constant ($\alpha \approx 2.502907875095893$) to three subsystems of the `claw`
runtime. Each one is a *self-chosen* use of a self-similar scale factor — see
*Scope and honest limits* at the end before reading anything physical into it.

1. **Adaptive Rate Limiter / Circuit Breaker (`FractalRateLimiter`)**: escalates
   inter-request pause, timeout and TPM caps by $\delta^L$ across failed retry
   chains, adds $\pm\,\text{jitter}/\alpha$ thundering-herd protection, sheds
   load at the chaos point, and cools back down after a quiet period.
2. **Geometric Session Compaction (`fractal_sample_indices`)**: thins removed
   conversation history with $\delta$-spaced geometric sampling so long sessions
   keep a bounded "fractal memory" of the older trajectory.
3. **Model Tier Cascade (`default_cascade`)**: the `quick` / `stable` / `mega` /
   `reasoner` aliases the CLI resolves for `--model`. Tier *selection* stays
   explicit and user-driven; δ does not pick models.

---

## Module Organization (`rust/crates/runtime/src/fractal/`)

```text
rust/crates/runtime/src/fractal/
├── mod.rs             # Public module re-exports
├── constants.rs       # FEIGENBAUM_DELTA, FEIGENBAUM_ALPHA, MAX_FRACTAL_DEPTH
├── rate_limiter.rs    # FractalRateLimiter (δ^L escalation, α-jitter, cooling decay, breaker)
├── compact.rs         # fractal_sample_indices — δ-geometric thinning
├── model_cascade.rs   # ModelTier, default_cascade()
└── tests.rs           # Pure Rust unit test suite
```

Earlier revisions of this document described a `task_graph.rs` / `telemetry.rs`
pair and a family of budget helpers (`level_budget`,
`asymmetric_sibling_weight/budget`, `optimal_children`, `dynamic_max_depth`,
`FractalBudget`, `CHAOS_THRESHOLD`), plus `fractal_compact_messages`,
`select_model_for_depth/budget` and `tier_for_pressure`. None of them had a
call site outside this module, so they have been deleted rather than kept as
decorative API.

---

## Mathematical Formulation & Code Reference

### 1. Fundamental Constants (`constants.rs`)

```rust
pub const FEIGENBAUM_DELTA: f64 = 4.669_201_609_102_991;
pub const FEIGENBAUM_ALPHA: f64 = 2.502_907_875_095_893;
pub const MAX_FRACTAL_DEPTH: usize = 4;
```

`MAX_FRACTAL_DEPTH` is the default breaker escalation ceiling
(`FractalRateLimiter::default()`); API clients override it from their retry
policy (`max_retries + 2`).

---

### 2. Adaptive Rate Limiter with Stochastic Alpha-Jitter (`rate_limiter.rs`)

The `FractalRateLimiter` adjusts inter-request pause, request timeout and TPM
caps as the level $L$ escalates:

- **Pause**: $P(L) = P_0 \cdot \delta^L$
- **Jittered Pause**: $P(L)_{\text{jitter}} = P(L) \pm \frac{\text{jitter\_factor}}{\alpha}$
- **Timeout**: $T(L) = T_0 \cdot \delta^L$
- **TPM Cap**: $\text{TPM}(L) = \frac{\text{TPM}_0}{\delta^L}$

```rust
pub fn current_pause_with_jitter(&self, jitter_factor: f64) -> Duration {
    let base_secs = self.current_pause().as_secs_f64();
    let jitter = (jitter_factor.clamp(-1.0, 1.0)) / FEIGENBAUM_ALPHA;
    let final_secs = (base_secs + jitter).max(0.1);
    Duration::from_secs_f64(final_secs)
}
```

#### Escalation, hysteresis and cooling

- **`on_failure()`**: $L \to L + 1$ (clamped at `max_level`), increments
  `consecutive_failures` and stamps `last_failure_time`.
- **`on_success()`**: $L \to L - 1$ — slower than escalation, so the breaker
  shows hysteresis instead of oscillating at the boundary.
- **`apply_decay(now)`**: after `BREAKER_DECAY_PERIOD` (60 s) with no failure,
  the breaker returns to $L = 0$ — a failing provider cannot lock a session out
  permanently.
- **Open state**: at `is_at_chaos_point()` ($L \ge L_{\max}$) requests are shed
  before any HTTP call and counted in `breaker_trips`.
- **Async safety**: `duration_to_wait(now)` is the non-blocking mirror of the
  blocking `wait_if_needed()`; async callers must use the former
  (`thread::sleep` on a tokio worker blocks the runtime).

**Runtime integration**: both API clients embed the limiter as a shared
circuit breaker (`send_with_retry` shed → `ApiError::CircuitOpen`,
`apply_decay()`/`BREAKER_DECAY_PERIOD` cooling, `breaker_trips` telemetry). See
*δ-fractal circuit breaker in the API clients* below.

---

### 3. Geometric Session Compaction (`compact.rs`)

Removed transcript entries are sampled at geometrically increasing intervals
backwards from the newest candidate:

$$\Delta_k = \delta^k \;\Rightarrow\; \text{sample size} \approx \log_\delta n$$

**Runtime integration**: `runtime::compact::compact_session` applies
`fractal_sample_indices` to the removed message range. A small verbatim sample
(only for removals of ≥ 5 tool-free messages) is kept between the summary and
the preserved tail, so long sessions retain a bounded fractal memory of the
older trajectory. Tool-use/tool-result messages are never sampled, to avoid
orphaned tool pairs.

---

### 4. Model Tier Cascade (`model_cascade.rs`)

```rust
pub fn default_cascade() -> Vec<ModelTier>
// quick → stable → mega → reasoner, tier index 0..3
```

**Runtime integration**: the CLI alias resolver
(`rusty-claude-cli/src/config.rs::resolve_model_alias_with_config`) falls back
to `default_cascade()` so `--model quick|stable|mega|reasoner` resolves to the
cascade model IDs when no user-defined config alias matches. This is the only
consumer, and it is deliberately user-initiated: nothing in the runtime
silently swaps a user's model.

---

## Integration coverage (audit, 2026-10-30)

Call sites outside `crates/runtime/src/fractal/`:

| Component | Wired into production? | Where |
|---|---|---|
| `compact.rs` (`fractal_sample_indices`) | Yes | `runtime::compact::compact_session` — δ-geometric thinning of removed history |
| `rate_limiter.rs` (`FractalRateLimiter`) | Yes — as a **circuit breaker** | `api::providers::{anthropic, openai_compat}::send_with_retry` |
| `rate_limiter.rs` diagnostics | Yes | `claw fractal` / `/fractal` (`rusty-claude-cli/src/utils.rs::print_fractal_status_snapshot`) |
| `model_cascade.rs` (`default_cascade`) | Yes | `rusty-claude-cli/src/config.rs` — `quick/stable/mega/reasoner` alias fallback |
| `api/src/breaker.rs` (`new_breaker`, `lock`, `snapshot`) | Yes | one registry-backed breaker per API client; `claw fractal` prints live state |

## δ-fractal circuit breaker in the API clients

Each `AnthropicClient` / `OpenAiCompatClient` owns a
`SharedBreaker = Arc<Mutex<FractalRateLimiter>>` built by
`api::breaker::new_breaker(provider, max_retries)` (base 1 s pause / 30 s
timeout / 80k TPM, `max_level = max_retries + 2`). The registry keeps a
reference under `{provider}#{seq}` so `claw fractal` can report live state.

- **Escalation**: every *exhausted retry chain* (not every 429) calls
  `on_failure()` → L↑, `consecutive_failures↑`.
- **De-escalation**: any successful response calls `on_success()` → L↓.
- **Open state**: at `is_at_chaos_point()` the next `send_with_retry()` shed
  immediately with `ApiError::CircuitOpen` (class `provider_circuit_open`,
  `is_retryable() == false`) *before* any HTTP request — stopping request
  storms against a persistently failing provider.
- **Cooling**: `apply_decay()` runs at the top of each `send_with_retry()`, so
  the breaker closes itself after `BREAKER_DECAY_PERIOD` (60 s) without a
  failure. No `thread::sleep` on the breaker path.
- The **per-attempt schedule stays exponential 2ⁿ + full-range splitmix64
  jitter + `Retry-After` honouring** — δ governs *across-chain* escalation and
  load shedding, not the intra-chain delay. It complements the per-key cooldown
  tracker (`api/src/cooldown.rs`), which is keyed by credential rather than by
  provider client.

## `claw fractal`

`claw fractal [--output-format json]` (and `/fractal` in the REPL) prints the δ
limiter level schedule with the `← ACTIVE` / `💥 CHAOS` markers, live breaker
state for the current process, the model cascade, the constants and the list of
integration touchpoints. `ApiError::CircuitOpen`'s message points at this
command.

## Scope and honest limits

δ and α are universal constants *of the logistic-map period-doubling route to
chaos*. Using them as a scale factor for backoff or sampling is a design
choice, not a physical law: they are irrational, well-known, and conveniently
"not 2", which is exactly the property wanted for decorrelating escalation
steps. Nothing in this architecture predicts or measures chaos in the provider;
$\delta^L$ is simply a fast geometric ramp, and the safety properties come from
the hysteresis, the 60 s cooling decay and the load-shedding ceiling — not from
Feigenbaum.

## Verification & Testing

```bash
cd rust
cargo test -p runtime --lib fractal
cargo test -p api --test client_integration fractal
```

### Results (2026-10-30)
- **9 fractal unit tests** (`fractal::tests` plus
  `compact::tests::retains_fractal_sample_of_removed_history`): δ-geometric
  sampling, breaker escalation and clamping, α-jitter, cooling decay,
  non-blocking `duration_to_wait`, cascade alias table.
- **2 API circuit-breaker integration tests**: shed at the chaos point with zero
  extra HTTP requests (exactly the canned 12 responses consumed); de-escalation
  on success.
- **Full workspace suite**: `cargo test --workspace` — 39 suites, all passing;
  `cargo check --workspace --all-targets` and `cargo clippy` report no new
  warnings in the fractal/API/CLI files touched by this architecture.
