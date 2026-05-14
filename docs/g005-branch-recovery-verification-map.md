# G005 Branch/Test Awareness and Recovery Verification Map

Source plan: `.omx/plans/claw-code-2-0-adaptive-plan.md` Stream 3.
Durable audit owner: leader checkpoint to `.omx/ultragoal/ledger.jsonl` after final verification. This file intentionally does not mutate leader-owned `.omx/ultragoal` state.

## Covered ROADMAP / PRD pinpoints

- `ROADMAP.md:912-921` — Phase 3 §7 stale-branch detection before broad verification: broad workspace test commands are preflighted before execution, stale/diverged branches emit `branch.stale_against_main`, and targeted tests bypass the broad-test gate.
- `ROADMAP.md:922-933` — Phase 3 §8 recovery recipes: stale-branch recovery remains represented by the `stale_branch` recipe, with one automatic attempt before escalation.
- `ROADMAP.md:935-949` — Phase 3 §8.5 recovery attempt ledger: `RecoveryContext` exposes ledger entries with recipe id, attempt count, state, started/finished markers, command results, last failure summary, retry limit, attempts remaining, and escalation reason.
- `ROADMAP.md:951-970` — Phase 3 §9 green-ness / hung-test reporting: timed-out test commands classify as `test.hung` with structured provenance instead of generic timeout.
- `ROADMAP.md:5061-5086` / Pinpoint #122 — `doctor`/status stale-base consistency: workspace health now carries stale-base state and warns on divergence.
- `prd.json:37-44` — US-003 stale-branch detection before broad verification: verified through the `workspace_test_branch_preflight` broad-test block and targeted-test bypass tests.
- `prd.json:50-57` — US-004 recovery recipes with ledger: verified through recovery ledger unit coverage and serialization-compatible recovery structs.

## Scope-to-artifact map

| Requirement | Evidence |
| --- | --- |
| Stale branch detection before broad tests | `rust/crates/tools/src/lib.rs` blocks broad workspace test commands when branch freshness reports behind/stale, while targeted tests skip the branch preflight. Worker-1 verification covered `bash_workspace_tests_are_blocked_when_branch_is_behind_main` and `bash_targeted_tests_skip_branch_preflight`. |
| Stale base/doctor consistency | `rust/crates/rusty-claude-cli/src/main.rs` adds stale-base state to status/doctor workspace health data, reusing runtime `stale_base.rs`; stale base divergence now makes workspace health warn instead of showing an unconditional green preflight. |
| Recovery recipes and attempt ledger | `rust/crates/runtime/src/recovery_recipes.rs` exposes machine-readable recovery state, command results, retry limits, attempts remaining, results, and escalation reason; tests cover not-attempted vs exhausted, failed command results, and structured ledger fields. |
| Green-ness contract | `rust/crates/runtime/src/green_contract.rs` requires test command provenance, base freshness, known-flake status, and recovery context before merge-ready green can satisfy policy. |
| Merge/reconcile policy requires green contract | `rust/crates/runtime/src/policy_engine.rs` gates `GreenAt` on `LaneContext.green_contract_satisfied`; `rust/crates/tools/src/lane_completion.rs` populates this field for automatic completion contexts. |
| Hung-test classification | `rust/crates/runtime/src/bash.rs` and `rust/crates/tools/src/lib.rs` classify timed-out test commands as `test.hung` with `failureClass: test_hang` and structured provenance. |

## Implementation anchors

- `rust/crates/runtime/src/stale_branch.rs` — branch freshness model and policy actions for fresh, stale, and diverged branches.
- `rust/crates/tools/src/lib.rs` — `workspace_test_branch_preflight`, `branch_divergence_output`, Bash/PowerShell broad-test gating, and `test.hung` structured timeout provenance on tool-shell timeouts.
- `rust/crates/runtime/src/recovery_recipes.rs` — recovery recipes plus `RecoveryLedgerEntry` / `RecoveryAttemptState` ledger surface.
- `rust/crates/runtime/src/bash.rs` — runtime Bash timeout classification and structured provenance for hung test commands.
- `rust/crates/runtime/src/green_contract.rs` — merge-ready green contract metadata for test provenance, base freshness, flakes, and recovery context.
- `rust/crates/runtime/src/policy_engine.rs` and `rust/crates/tools/src/lane_completion.rs` — policy/completion integration for `green_contract_satisfied`.
- `rust/crates/rusty-claude-cli/src/main.rs` — stale-base state in doctor/status workspace health.

## Leader verification commands

Run from repo root before checkpointing G005:

```sh
git diff --check
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo check --manifest-path rust/Cargo.toml -p runtime
cargo check --manifest-path rust/Cargo.toml -p tools
cargo check --manifest-path rust/Cargo.toml -p rusty-claude-cli
cargo test --manifest-path rust/Cargo.toml -p runtime recovery_ -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p runtime green_contract -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p runtime stale_branch -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p runtime stale_base -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p runtime timed_out_test_command_is_classified_as_hung_test_with_provenance -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p tools bash_tool_reports_success_exit_failure_timeout_and_background -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p tools lane_completion -- --nocapture
cargo test --manifest-path rust/Cargo.toml -p rusty-claude-cli workspace_health_warns_when_stale_base_diverged -- --nocapture
```

## Known unresolved / out-of-scope items

- Full `cargo test -p tools` has known permission-enforcer expectation failures reported by workers as pre-existing/out-of-scope for G005 branch freshness, recovery ledger, and hung-test classification.
- Open roadmap PR/issue reconciliation is gated to G011/G012 per `docs/pr-issue-resolution-gate.md`.

## Delegation evidence

- Worker-1 task 1 spawned two probes (`019e25c8-1b13-75f0-baee-182deee69724`, `019e25c8-1db7-73c0-a0d5-4425fdc9061a`); both errored with 429, direct repo evidence integrated.
- Worker-1 task 2 spawned repository map probe `019e25d5-9be9-7193-8a33-f21450beb62c`; it errored with 429, direct ROADMAP/PRD/doc findings integrated.
- Worker-2 task 3 spawned two child tasks (`019e25cb-b340-7041-9e49-143a95ccd263`, `019e25cb-b936-7310-9f39-6c77f40ae805`); one hit 429 and one timed out/shutdown, local tests/inspection integrated.
- Worker-3 task 4 spawned change-slice probe `019e25cc-da54-7860-abe6-80c8222ad4db`; it errored with 429, serial evidence integrated.
