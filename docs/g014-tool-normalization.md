# Tool Normalization and Coercion

## Overview

In `claw-code`, the tool execution registry is responsible for receiving tool call requests from the AI model (LLM) and executing them securely. However, LLMs frequently make mistakes in formatting their tool calls. Common issues include:
1. **Tool Name Aliasing/Casing errors**: E.g., calling `WebFetch`, `web-fetch`, or `webfetch` instead of the canonical `web_fetch`.
2. **Type Coercion errors**: E.g., providing a string `"true"` instead of a boolean `true`, or `"120000"` instead of a number `120000`.

To address these, the normalization logic has been extracted into a dedicated module: `rust/crates/tools/src/normalization.rs`.

## Modern Approaches to Tool Error Handling

Modern agentic systems handle tool input errors using two primary strategies:

1. **LLM Self-Correction (Reactive)**: The system rejects the invalid tool call and returns an error message to the LLM (e.g., "Expected boolean for 'dry_run', got string"). The LLM then self-corrects and issues a new, fixed tool call. This is robust but costs extra tokens, latency, and API calls.
2. **Registry-Level Coercion (Proactive)**: The system intercepts the tool call before validation and silently coerces the inputs to the expected format. This is the approach we have taken here to optimize latency and reliability.

## How It Is Organized Here

The normalization system is split into two phases during `GlobalToolRegistry::execute` in `rust/crates/tools/src/lib.rs`:

### 1. Name Normalization (`canonical_allowed_tool_name`)
When a tool name is received, the registry first tries to match it exactly. If it fails, it resolves the name using `allowed_tool_aliases()`, which relies on `normalization::canonical_allowed_tool_name`.
This function converts all names into a canonical `snake_case` representation. For example, `WebFetch`, `Web-Fetch`, and `webFetch` all map correctly to `web_fetch`.

### 2. Input Coercion (`coerce_tool_input`)
Before passing the JSON payload to the specific tool handler, the entire payload is recursively traversed by `normalization::coerce_tool_input`. 
- Strings like `"true"` or `"FALSE"` are safely converted to JSON Booleans.
- Strings containing numeric characters (e.g., `"123"`) are safely parsed and converted to JSON Numbers.
- Nested objects and arrays are recursively processed.

By pulling this out of the >11k LOC `lib.rs` file into `normalization.rs`, the code is cleaner, more testable, and establishes a clear architectural boundary for input sanitation.
