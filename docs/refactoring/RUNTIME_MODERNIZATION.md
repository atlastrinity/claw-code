# Runtime Modernization: Architecture & Features

This document provides a comprehensive overview of the **Runtime Modernization** updates implemented to improve performance, maintainability, and reliability in the Claw architecture.

The modernization is split into three core phases:
1. **Parallel Tool Execution**
2. **Middleware Pipeline**
3. **Resilient Provider Chain**

---

## 1. Parallel Tool Execution

### Overview
Historically, `claw` executed multiple tool calls sequentially, even if they were read-only operations (like `read_file`, `grep_search`). This resulted in a linear increase in latency based on the number of requested tool calls. 

We introduced `ToolDispatcher` which intercepts batched tool calls from the provider and runs eligible ones concurrently using `tokio::task::spawn_blocking`.

### How it Works
- `is_parallelizable(tool_name: &str) -> bool`: Identifies whether a tool is pure/read-only (e.g., `read_file`, `ToolSearch`). Write/mutation operations (e.g., `bash`, `write_file`) return `false`.
- **Batching Strategy:** Tool calls are dynamically grouped into `ToolBatch::Parallel` and `ToolBatch::Sequential`. A sequence of parallelizable tools will execute concurrently. The moment a mutation tool is hit, the batch is broken, and that tool runs isolated to prevent race conditions.
- **Backward Compatibility:** `ToolExecutor` exposes a `supports_parallel()` trait method. Existing implementations default to `false` unless explicitly overridden.

---

## 2. Middleware Pipeline

### Overview
The legacy `ConversationRuntime::run_turn()` was a monolithic method managing health probes, tool hooks, permission validations, usage tracking, and execution simultaneously.

We extracted these responsibilities into a clean, composable **Middleware Chain**.

### Components
All execution flows now traverse a typed pipeline conforming to the `TurnMiddleware` trait:

1. **`TracingMiddleware`**: Logs `tool_started` and `tool_finished` events automatically.
2. **`PermissionMiddleware`**: Evaluates `PermissionPolicy` and prompts the user securely before proceeding.
3. **`HookMiddleware`**: Executes arbitrary pre-hook scripts and merges returned outputs seamlessly into the tool's final response metadata to inform the model contextually.
4. **`ExecutionMiddleware`**: The terminal middleware. Directly calls the underlying `ToolExecutor` (e.g., `CliToolExecutor` or `StaticToolExecutor`) and captures the strict execution result.

### Usage
This design preserves `ConversationRuntime::new()` internally but opens up dynamic builder patterns via `ConversationRuntime::builder()`.

```rust
// Example Middleware Assembly
let mut chain = MiddlewareChain::new(executor)
    .with(TracingMiddleware::new(tracer))
    .with(PermissionMiddleware::new(policy))
    .with(HookMiddleware::new(hooks));

let outcomes = chain.process_batch(tool_calls);
```

---

## 3. Resilient Provider Chain

### Overview
To improve reliability, especially when primary models experience API degradation or rate limits, the provider fallback loops were overhauled to include Circuit Breaking and Cost Tracking capabilities.

### Features
- **`CircuitBreaker`**: 
  - Exposes three states: `Closed` (Healthy), `Open` (Tripped), `HalfOpen` (Recovery testing).
  - Automatically trips after a configured threshold of consecutive retryable failures from a provider.
  - While `Open`, it short-circuits the failing provider to instantly fall back to the secondary models, reducing time-to-first-token latency.
- **`ResilientProviderChain`**: 
  - A structured wrapper for fallback providers replacing the raw vector iteration loops.
  - Automatically evaluates circuit states and implements graceful failovers across Claude, Grok, etc.
- **`CostTracker`**:
  - Embedded seamlessly to accumulate `total_input_tokens`, `total_output_tokens`, and `estimated_cost_usd` across all retries and model handoffs over a turn.

---

## Summary
These features dramatically accelerate read-heavy reasoning turns, make the codebase highly modular (uncoupling logging, hooks, and execution), and harden the provider ingestion against upstream network disruptions.
