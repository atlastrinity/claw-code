#!/usr/bin/env bash
# Build release script for Claw Code
# Compiles all binaries and places them in a global bin folder (~/.claw/bin)
# Updates settings synchronization.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RUST_DIR="${SCRIPT_DIR}/rust"
GLOBAL_DIR="$HOME/.claw"
GLOBAL_BIN_DIR="${GLOBAL_DIR}/bin"

echo "==> Building Claw Code in release mode..."
cd "${RUST_DIR}"
cargo build --release --workspace

echo "==> Ensuring global bin directory exists: ${GLOBAL_BIN_DIR}"
mkdir -p "${GLOBAL_BIN_DIR}"

echo "==> Copying binaries to global bin directory..."
cp target/release/claw "${GLOBAL_BIN_DIR}/"
cp target/release/claw-analog "${GLOBAL_BIN_DIR}/"
cp target/release/claw-rag-service "${GLOBAL_BIN_DIR}/"

echo "==> Synchronizing MCP Server Settings..."
LOCAL_SETTINGS="${SCRIPT_DIR}/.claw.json"
GLOBAL_SETTINGS="${GLOBAL_DIR}/settings.json"

if [ -f "${LOCAL_SETTINGS}" ] && [ ! -f "${GLOBAL_SETTINGS}" ]; then
    echo "    Creating global settings from local..."
    cp "${LOCAL_SETTINGS}" "${GLOBAL_SETTINGS}"
elif [ ! -f "${LOCAL_SETTINGS}" ] && [ -f "${GLOBAL_SETTINGS}" ]; then
    echo "    Creating local settings from global..."
    cp "${GLOBAL_SETTINGS}" "${LOCAL_SETTINGS}"
elif [ -f "${LOCAL_SETTINGS}" ] && [ -f "${GLOBAL_SETTINGS}" ]; then
    echo "    Syncing configurations between local and global..."
    if [ "${LOCAL_SETTINGS}" -nt "${GLOBAL_SETTINGS}" ]; then
        echo "    Local .claw.json is newer. Overwriting global settings.json..."
        cp "${LOCAL_SETTINGS}" "${GLOBAL_SETTINGS}"
    elif [ "${GLOBAL_SETTINGS}" -nt "${LOCAL_SETTINGS}" ]; then
        echo "    Global settings.json is newer. Overwriting local .claw.json..."
        cp "${GLOBAL_SETTINGS}" "${LOCAL_SETTINGS}"
    else
        echo "    Settings are identical in timestamp."
    fi
else
    echo "    Warning: Missing both local and global settings files."
fi

echo "==> Setup complete!"
echo "    Make sure to add ${GLOBAL_BIN_DIR} to your PATH, e.g.:"
echo "    export PATH=\"${GLOBAL_BIN_DIR}:\$PATH\""
