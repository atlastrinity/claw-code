# Claw Code Project Context and Agent Rules

This file provides context and operational rules for any AI agent working within the `claw-code` repository.

## 1. Project Overview
- **Identity**: This is the public Rust implementation of the `claw` CLI agent harness, a system meant to run agents securely over codebases. 
- **Core Components**:
  - `rusty-claude-cli` (`claw`): The full CLI agent with REPL, tools, plugins, and MCP support.
  - `claw-analog`: A lean, non-interactive agent for CI/scripts with strictly bounded file system permissions and no bash/MCP.
  - `claw-rag-service`: A local retrieval-augmented generation service (indexing codebase into SQLite via embeddings) for semantic search.
- **Canonical Code**: The source of truth is in the `rust/` directory. The `src/` and `tests/` directories contain reference Python implementations and test surfaces.

## 2. Agent Operational Rules
- **Testing & Verification**: 
  - To check formatting: `scripts/fmt.sh --check`
  - To run formatting: `scripts/fmt.sh`
  - To run lints: `cd rust && cargo clippy --workspace --all-targets -- -D warnings`
  - To run tests: `cd rust && cargo test --workspace`
- **Security & Sandboxing**:
  - Pay attention to the agent permission mode. Operations should default to `workspace-write`. 
  - DO NOT use shell/bash commands to bypass file restrictions unless specifically requested by the user under `danger-full-access`.
  - When creating or executing new programs, scripts, or other software, ensure they are isolated within proper environments (e.g., virtual environments, local sandboxed directories, or containers) to adhere to sandbox constraints and prevent system-wide side effects. Specifically, use the root `.sandbox-tmp/` directory for temporary general-purpose scratchpad files/scripts, and note that `rust/.sandbox-home/` is reserved to isolate Rust toolchain operations from the user's global system.
- **Building the Project**:
  - Always build via `cargo build --workspace` inside the `rust/` directory.
  - The binary will be available at `target/debug/claw` (or `claw.exe` on Windows).
  - Health check should always be performed after a build: `./target/debug/claw doctor`.
- **Environment Variables & Auth**:
  - `ANTHROPIC_API_KEY` for Anthropic keys (`sk-ant-...`).
  - `ANTHROPIC_AUTH_TOKEN` for OAuth Bearer tokens.
  - `OPENAI_API_KEY` and `OPENAI_BASE_URL` for OpenAI-compatible proxies (like OpenRouter, vLLM).
  - Do not confuse the two shapes; Anthropic direct keys will cause a `401` if placed in the `AUTH_TOKEN` slot.

## 3. Configuration & Sandboxing (`.claw.json`)
The behavior, security limits, and capabilities of the agent are controlled by configuration files (loaded in order of precedence: user global -> project `.claw.json` -> `.claw/settings.json` -> `.claw/settings.local.json`).

Key parameters agents should be aware of:
- **`allowedTools` (Sandboxing)**: Explicitly whitelists the tools the agent can use. If tools like `bash`, `WebFetch`, or `WebSearch` are omitted, the agent operates in a restricted sandbox and cannot execute terminal commands or access the internet.
- **Permission Modes (CLI overrides)**:
  - `--permission-mode read-only`: Prevents any mutation or execution.
  - `--permission-mode workspace-write`: Allows code editing but blocks dangerous global commands.
  - `--permission-mode danger-full-access`: Unrestricted access.
- **`hooks`**: Can enforce execution of scripts before/after tools (e.g., running `audit.sh` before `bash` tool use).
- **`aliases`**: Defines quick model aliases (e.g., `"quick": "local/google/gemma-4-31b-it:free"`).
- **`mcpServers`**: Integrates external tools. Note that the `macos-use` server for native macOS UI automation is now natively integrated ("пришитий нативно") into the project configuration.
- **`rulesImport`**: Defines whether to automatically import instructions from `.cursorrules`, Copilot, etc.

## 4. Key Documentation References
When in doubt, consult the following core files:
- `README.md`: Overview and entry point.
- `USAGE.md`: Command structures, advanced REPL commands (`/ultraplan`, `/teleport`), provider matrices, and proxy setups.
- `docs/concept.md`: Architectural boundaries and the separation of concerns between `claw`, `claw-analog`, and `claw-rag-service`.
- `PARITY.md`: Tracking parity with upstream logic.

## 5. Work Style
- Keep changes localized, modular, and well-tested. 
- Do not modify existing `CLAUDE.md` content automatically. Extend rules locally in `.claw/rules/` or via `.claw.json` configurations.
