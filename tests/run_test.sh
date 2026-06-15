#!/bin/bash
cargo test --manifest-path rust/Cargo.toml -p runtime --lib mcp_stdio::tests::manager_discovers_tools_from_stdio_config -- --nocapture
