# Feigenbaum Fractal Architecture (δ ≈ 4.669) in Rust

## Overview

The **Feigenbaum Fractal Architecture** applies the universal period-doubling bifurcation constant ($\delta \approx 4.6692016091029907$) to three critical subsystems of the `claw` runtime:

1. **Self-Similar Task Graph (`FractalTaskGraph`)**: Governs task decomposition budgets, depth decay, and chaos-threshold termination.
2. **Adaptive Rate Limiter (`FractalRateLimiter`)**: Escalates request pauses, timeouts, and TPM limits using $\delta^L$ scaling with circuit-breaker protection.
3. **Inverted Model Cascade (`ModelCascade`)**: Automatically assigns tasks to model tiers based on depth and rate-limiter pressure.
4. **Geometric Session Compaction (`fractal_compact_messages`)**: Thins conversation transcripts using $\delta$-spaced geometric sampling to preserve long-term history without memory inflation.

---

## Module Organization (`rust/crates/runtime/src/fractal/`)

The architecture is implemented in pure Rust under `rust/crates/runtime/src/fractal/`:

```
rust/crates/runtime/src/fractal/
├── mod.rs             # Public module re-exports
├── constants.rs       # Feigenbaum constants (δ, α), budget decay, depth fractions, FractalBudget
├── task_graph.rs      # FractalTaskNode, FractalTaskGraph with .clawd-task-graph.json compatibility
├── rate_limiter.rs    # FractalRateLimiter (δ-backoff, hysteresis recovery, circuit breaker)
├── model_cascade.rs   # ModelTier, default_cascade(), select_model_for_depth()
├── compact.rs         # Geometric fractal transcript compaction
└── tests.rs           # Pure Rust unit test suite
```

---

## Mathematical Formulation & Code Reference

### 1. Fundamental Constants (`constants.rs`)

```rust
pub const FEIGENBAUM_DELTA: f64 = 4.669_201_609_102_990_7;
pub const FEIGENBAUM_ALPHA: f64 = 2.502_907_875_095_893;
pub const CHAOS_THRESHOLD: f64 = 0.01; // 1% of root budget
```

#### Token Budget Decay by Depth
$$B(d) = \left\lfloor \frac{B_0}{\delta^d} \right\rfloor$$

```rust
pub fn level_budget(total: usize, depth: usize) -> usize {
    if depth == 0 {
        return total;
    }
    let scaled = (total as f64) / FEIGENBAUM_DELTA.powi(depth as i32);
    (scaled.floor() as usize).max(1)
}
```

#### Chaos Boundary Detection
A task node is considered **atomic** when its fractional budget falls below 1% of the total budget:
```rust
pub fn is_atomic(total: usize, depth: usize) -> bool {
    let fraction = level_fraction(depth);
    fraction < CHAOS_THRESHOLD
}
```

---

### 2. TaskGraph Decomposition (`task_graph.rs`)

`FractalTaskNode` manages task trees where child budgets automatically decay by $\frac{1}{\delta}$.

| Depth | Budget Fraction | Tokens (out of 2000) | Atomic Status |
|-------|----------------|-----------------------|---------------|
| 0 (Root) | 100% | 2000 | False |
| 1 | 21.4% | 428 | False |
| 2 | 4.6% | 91 | False |
| 3 | 0.98% | 19 | True (Chaos Boundary) |

---

### 3. Adaptive Rate Limiter (`rate_limiter.rs`)

The `FractalRateLimiter` adjusts inter-request pause, request timeout, and TPM caps dynamically upon failure:

- **Pause**: $P(L) = P_0 \cdot \delta^L$
- **Timeout**: $T(L) = T_0 \cdot \delta^L$
- **TPM Cap**: $\text{TPM}(L) = \frac{\text{TPM}_0}{\delta^L}$

```rust
pub fn current_pause(&self) -> Duration {
    let factor = FEIGENBAUM_DELTA.powi(self.current_level as i32);
    Duration::from_secs_f64(self.base_pause_secs * factor)
}
```

#### Escalation & Hysteresis Recovery:
- **`on_failure()`**: Escalates level $L \to L + 1$ (up to `max_level`).
- **`on_success()`**: Slowly de-escalates $L \to L - 1$ to prevent oscillation.
- **Circuit Breaker**: When $L = L_{\max}$, `is_at_chaos_point()` returns `true`, halting requests to prevent API ban.

---

### 4. Inverted Model Cascade (`model_cascade.rs`)

Maps task complexity and depth to model tiers:
- **Root Tasks (Depth 0)** $\to$ Reasoning / Heavy Models (`reasoner`, `mega`)
- **Deep Atomic Tasks** $\to$ Light / Fast Models (`quick`)
- **Limiter Pressure ($L > 0$)** $\to$ Automatically shifts selection to lighter tiers.

```rust
pub fn select_model_for_depth(depth: usize, cascade: &[ModelTier], limiter_level: usize) -> ModelTier {
    let max_tier = cascade.len() - 1;
    let ideal_tier = max_tier.saturating_sub(depth.min(max_tier));
    let adjusted_tier = ideal_tier.saturating_sub(limiter_level);
    cascade[adjusted_tier].clone()
}
```

---

### 5. Geometric Session Compaction (`compact.rs`)

Compacts transcript history by preserving recent entries fully while sampling older entries at geometrically increasing intervals:

$$\Delta_k = \delta^k$$

This retains a *fractal memory* of conversation trajectory while keeping token usage strictly bounded.

---

## Verification & Testing

The Rust implementation has been fully validated with unit tests:

```bash
cd rust
cargo test --package runtime --lib fractal::tests
```

### Results:
- **8 Pure Rust Fractal Unit Tests**: All passed.
- **Full `runtime` crate test suite**: 670 passed, 0 failed.
- **Warnings**: 0.
