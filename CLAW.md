# Core Agent Directive

## Autonomous Execution

Execute every task fully to completion. Do not stop to ask for clarification, confirmation, or present choices mid-task. Make the best judgment call and proceed. Only pause if genuinely blocked by missing critical information that cannot be inferred.

## Vision & Native Computer Control

You have full macOS screen vision and GUI control via `macos-use_*` tools. Always start with `macos-use_vision` to get exact OCR coordinates before clicking or typing — never guess coordinates. Use `ToolSearch` to discover any available tools by keyword.

## RAG Context Store

- **`retrieve_context`**: Query before starting complex tasks to find prior knowledge.
- **`ingest_context`**: Store findings, patterns, and decisions after significant work for future retrieval. Use descriptive, keyword-rich text.

## Project Structure

- **`rusty-claude-cli` (`claw`)**: Full CLI agent with REPL, tools, plugins, MCP.
- **`claw-analog`**: Lean non-interactive agent for CI/scripts.
- **`claw-rag-service`**: Local RAG service (SQLite + embeddings) for semantic search.
- Source of truth: `rust/` directory. `src/` and `tests/` are Python reference implementations.

## Verification Commands

- Format: `scripts/fmt.sh` (check: `scripts/fmt.sh --check`)
- Lint: `cd rust && cargo clippy --workspace --all-targets -- -D warnings`
- Test: `cd rust && cargo test --workspace`
- Build: `cd rust && cargo build --workspace`
- Health check: `./rust/target/debug/claw doctor`

## Configuration

Config load order: user global → `.claw.json` → `.claw/settings.json` → `.claw/settings.local.json`. The `tools` array in `.claw.json` whitelists available tools. Permission modes: `read-only`, `workspace-write`, `danger-full-access`.
