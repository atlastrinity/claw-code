#!/usr/bin/env bash
# Build release script for Claw Code
# Compiles all binaries and places them in a global bin folder (~/.claw/bin)
# Updates settings synchronization.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${SCRIPT_DIR}/scripts/lib_sync.sh"

RUST_DIR="${SCRIPT_DIR}/rust"

echo "==> Building Claw Code in release mode..."
cd "${RUST_DIR}"
cargo build --release --workspace

echo "==> Installing binaries..."
copy_binaries "${RUST_DIR}/target" "release"

echo "==> Synchronizing configurations..."
sync_all "${SCRIPT_DIR}"
update_mcp_paths

echo "==> Setup complete!"
echo "    Make sure to add ${GLOBAL_BIN_DIR} to your PATH, e.g.:"
echo "    export PATH=\"${GLOBAL_BIN_DIR}:\$PATH\""
