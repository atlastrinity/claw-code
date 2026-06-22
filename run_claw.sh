#!/bin/bash
cargo run --manifest-path rust/Cargo.toml --bin claw -- \
  --model gemini-lite \
  --skip-permissions \
  --accept-danger-non-interactive \
  --attach-skill .claw/skills/project_specific/ios_remote_client.md "$@"
