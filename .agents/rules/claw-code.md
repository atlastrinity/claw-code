---
trigger: manual
description: Core repository context, architecture, and log locations for claw-code
---

# Claw Code Repository Guide

## 1. Project Overview

`claw-code` is a public Rust ecosystem surrounding the `claw` CLI agent. It is designed to be a highly modular, fast, and strict agentic coding assistant. The entire Rust workspace is contained within the `rust/` directory.

## 2. Core Components (Crates)

- **`rusty-claude-cli`** (`bin: claw`): The main, heavy CLI application with REPL, user interaction, and terminal UI.
- **`claw-analog`** (`bin: claw-analog`): A lightweight, automated wrapper for the agent that uses explicit file system bounds, JSON streaming (NDJSON), and explicit permissions. Designed for AI and scripts.
- **`claw-rag-service`** (`bin: claw-rag-service`): A standalone HTTP service handling code indexing, embeddings, and semantic search (Retrieval-Augmented Generation). Separated from the CLI to keep the agent fast and stateless.
- **`logger`** (`lib: claw_logger`): The centralized `tracing`-based logging module.

## 3. Logs & Diagnostics

If you need to debug the application, inspect background processes, or check for runtime errors, **DO NOT rely solely on terminal output**.

- **Log Location**: `~/.claw/logs/`
- **Log Format**: Daily rotating files using the `tracing-appender` crate.
- **Filenames**:
  - `~/.claw/logs/claw.log.YYYY-MM-DD` (for the main CLI)
  - `~/.claw/logs/claw-analog.log.YYYY-MM-DD` (for the automation wrapper)
  - `~/.claw/logs/claw-rag-service.log.YYYY-MM-DD` (for the RAG backend)

_When the user reports an issue, you should proactively `cat` or `tail` the relevant files in `~/.claw/logs/` to find error stack traces._

## 4. Useful Commands

To run the main applications from the workspace root:

- Main CLI: `cargo run --manifest-path rust/Cargo.toml --bin claw -- <args>`
- RAG Service: `cargo run --manifest-path rust/Cargo.toml -p claw-rag-service -- serve`
- Analog: `cargo run --manifest-path rust/Cargo.toml -p claw-analog -- <args>`

## 5. Documentation

- `docs/concept.md` - The source of truth for architectural principles and boundaries.
- `docs/how_to_run.md` - Deep dive into how `claw-analog` works and how to execute it.
- `rust/TUI-ENHANCEMENT-PLAN.md` - Current plans and refactoring guidelines for the terminal interface.

## 6. Sessions & Conversation History

Conversation context and message history for the CLI are stored locally in JSON Lines format:

- **Location**: `.claw/sessions/<workspace-id>/`
- **Format**: `session-<timestamp>-<index>.jsonl`

If you need to analyze the exact messages sent to/from the LLM, the raw tool calls, or the exact tool results in a given session, parse these `.jsonl` files. They represent the "ground truth" of the AI's conversation state.
