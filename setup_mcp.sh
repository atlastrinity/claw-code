#!/usr/bin/env bash
# Script to automate setup of iOS Simulator MCP dependencies
# Handles installation of ios-simulator-mcp, fb-idb, idb-companion,
# and creates a robust python wrapper for the idb client.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BREW_PREFIX="$(brew --prefix 2>/dev/null || echo '/opt/homebrew')"
IDB_DEST="${BREW_PREFIX}/bin/idb"

echo "==> [1/4] Installing ios-simulator-mcp globally..."
npm install -g ios-simulator-mcp

echo "==> [2/4] Installing fb-idb Python package..."
pip3 install --user fb-idb

echo "==> [3/5] Tapping facebook/fb and installing idb-companion..."
brew tap facebook/fb
brew trust facebook/fb || true
brew install idb-companion

echo "==> [4/5] Installing xcodegen (required for project generation)..."
brew install xcodegen || true

echo "==> [5/5] Creating idb wrapper at ${IDB_DEST}..."
SANDBOX_HOME="${SCRIPT_DIR}/.sandbox-home"

# Detect if we are running in a sandboxed home directory
if [ -d "${SANDBOX_HOME}" ]; then
    echo "    Detected sandbox-home directory at ${SANDBOX_HOME}"
    SITE_PACKAGES="${SANDBOX_HOME}/Library/Python/3.9/lib/python/site-packages"
    IDB_BIN="${SANDBOX_HOME}/Library/Python/3.9/bin/idb"
else
    echo "    Using default user home directory"
    SITE_PACKAGES="${HOME}/Library/Python/3.9/lib/python/site-packages"
    IDB_BIN="${HOME}/Library/Python/3.9/bin/idb"
fi

# Ensure the parent directory of IDB_DEST is writable and exists
mkdir -p "$(dirname "${IDB_DEST}")"
rm -f "${IDB_DEST}"

cat <<EOF > "${IDB_DEST}"
#!/bin/bash
PYTHONPATH="${SITE_PACKAGES}" exec /Applications/Xcode.app/Contents/Developer/usr/bin/python3 "${IDB_BIN}" "\$@"
EOF

chmod +x "${IDB_DEST}"
echo "    Successfully wrote wrapper to ${IDB_DEST}"

echo "==> Verification..."
echo "    ios-simulator-mcp: $(which ios-simulator-mcp)"
echo "    idb:               $(which idb)"
echo "    idb_companion:     $(which idb_companion)"

if idb list-targets >/dev/null 2>&1; then
    echo "✅ Setup successful! idb client and companion are communicating."
else
    echo "⚠️ Setup completed, but idb targets list check failed. Ensure Simulator is booted."
fi
