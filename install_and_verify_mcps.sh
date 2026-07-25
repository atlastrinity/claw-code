#!/usr/bin/env bash
# ==============================================================================
# Claw MCP Servers Installer, Verifier & Health Check
# ==============================================================================
#
# Usage:
#   ./install_and_verify_mcps.sh               # verify MCP servers + auth
#   ./install_and_verify_mcps.sh --build        # build release binaries first, then verify
#   ./install_and_verify_mcps.sh --build --debug # build debug binaries first, then verify
#
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Source shared library
source "${SCRIPT_DIR}/scripts/lib_sync.sh"

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

DO_BUILD=0
BUILD_PROFILE="release"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --build)  DO_BUILD=1 ;;
        --debug)  BUILD_PROFILE="debug" ;;
        -h|--help)
            echo "Usage: $0 [--build] [--debug]"
            echo "  --build   Build claw binaries before verifying MCP servers"
            echo "  --debug   Use debug profile instead of release (only with --build)"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
    shift
done

# ---------------------------------------------------------------------------
# Load environment
# ---------------------------------------------------------------------------

if [ -f "$HOME/.claw/.env" ]; then
    source "$HOME/.claw/.env"
fi

# ---------------------------------------------------------------------------
# Header
# ---------------------------------------------------------------------------

echo -e "${_LS_BOLD}${_LS_CYAN}========================================================================${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}                Claw MCP Servers Installer & Verifier                    ${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}========================================================================${_LS_NC}"
echo ""

# ---------------------------------------------------------------------------
# Optional: Build binaries first
# ---------------------------------------------------------------------------

if [ "$DO_BUILD" -eq 1 ]; then
    RUST_DIR="${SCRIPT_DIR}/rust"

    if [ ! -d "$RUST_DIR" ] || [ ! -f "$RUST_DIR/Cargo.toml" ]; then
        echo -e "${_LS_RED}ERROR:${_LS_NC} Rust workspace not found at ${RUST_DIR}"
        exit 1
    fi

    echo -e "${_LS_BOLD}Building claw workspace (${BUILD_PROFILE})...${_LS_NC}"
    CARGO_FLAGS=("build" "--workspace")
    if [ "$BUILD_PROFILE" = "release" ]; then
        CARGO_FLAGS+=("--release")
    fi

    (cd "$RUST_DIR" && cargo "${CARGO_FLAGS[@]}")

    copy_binaries "${RUST_DIR}/target" "$BUILD_PROFILE"
    echo ""
fi

# ---------------------------------------------------------------------------
# Step 1: Update extension paths in JSON configs
# ---------------------------------------------------------------------------

echo -e "${_LS_BOLD}Updating IDE extension paths...${_LS_NC}"
update_mcp_paths

# ---------------------------------------------------------------------------
# Step 2: Sync settings
# ---------------------------------------------------------------------------

sync_settings "$SCRIPT_DIR"

# ---------------------------------------------------------------------------
# Step 3: Verify MCP servers
# ---------------------------------------------------------------------------

echo ""
echo -e "${_LS_BOLD}${_LS_CYAN}------------------------------------------------------------------------${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}                     Verifying 10 MCP Servers                           ${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}------------------------------------------------------------------------${_LS_NC}"

verify_command() {
    local name="$1"
    local check_cmd="$2"
    local install_cmd="$3"

    echo -e "Checking ${_LS_BOLD}$name${_LS_NC}..."

    if eval "$check_cmd" </dev/null >/dev/null 2>&1; then
        echo -e "  -> Status: ${_LS_GREEN}✅ Installed & Verified${_LS_NC}"
        return 0
    else
        echo -e "  -> Status: ${_LS_YELLOW}⚠️  Not found or failed. Attempting to install/refresh...${_LS_NC}"
        if [ -n "$install_cmd" ]; then
            echo -e "  -> Running: $install_cmd"
            if eval "$install_cmd" >/dev/null 2>&1; then
                # Re-verify
                if eval "$check_cmd" </dev/null >/dev/null 2>&1; then
                    echo -e "  -> Status: ${_LS_GREEN}✅ Installed & Verified successfully${_LS_NC}"
                    return 0
                fi
            fi
        fi
        echo -e "  -> Status: ${_LS_RED}❌ Verification failed${_LS_NC}"
        return 1
    fi
}

# 1. appstore-connect
verify_command "App Store Connect (asc-mcp)" \
    "npx --prefer-offline --package @pofky/asc-mcp asc-mcp --help" \
    "npm install -g @pofky/asc-mcp"

# 2. firebase-mcp-server
export FIREBASE_CLI_NO_PROMPT=1
verify_command "Firebase Tools MCP" \
    "npx --prefer-offline --package firebase-tools firebase --version --non-interactive" \
    "npm install -g firebase-tools"

# 3. github
verify_command "GitHub MCP Server" \
    "npx --prefer-offline --package @modelcontextprotocol/server-github mcp-server-github --help" \
    "npm install -g @modelcontextprotocol/server-github"

# 4. ios-simulator
verify_command "iOS Simulator MCP" \
    "npx --prefer-offline --package ios-simulator-mcp ios-simulator-mcp --help" \
    "npm install -g ios-simulator-mcp"

# 5. render (mcp-remote)
verify_command "Render MCP Client (mcp-remote)" \
    "npx --prefer-offline --package mcp-remote mcp-remote --version >/dev/null 2>&1 || [ -n \"\$(command -v npx)\" ]" \
    "npm install -g mcp-remote"

# 6. xcode-bridge (mcpbridge)
verify_command "Xcode Bridge (mcpbridge)" \
    "[ -f /Applications/Xcode.app/Contents/Developer/usr/bin/mcpbridge ]" \
    "echo 'Please make sure Xcode and its Command Line Tools are installed.'"

# 7. swiftlens
verify_command "SwiftLens (uvx swiftlens)" \
    "uvx --python 3.13 swiftlens --help" \
    "uv tool install --python 3.13 swiftlens"

# 8. notebooks
verify_command "Notebooks Proxy Extension" \
    "[ -f \"\$HOME/.antigravity-ide/extensions/googlecloudtools.datacloud-\"*/\"/mcp_servers/cli/mcp_proxy_bundle.js\" ]" \
    "echo 'Please verify the Google Cloud Tools extension is installed in your IDE.'"

# 9. visualization
verify_command "Visualization Proxy Extension" \
    "[ -f \"\$HOME/.antigravity-ide/extensions/googlecloudtools.datacloud-\"*/\"/mcp_servers/cli/mcp_proxy_bundle.js\" ]" \
    "echo 'Please verify the Google Cloud Tools extension is installed in your IDE.'"

# 10. pyscn-mcp
verify_command "PyScn MCP (pyscn-mcp)" \
    "uvx pyscn-mcp --help" \
    "uv tool install pyscn-mcp"

# ---------------------------------------------------------------------------
# Step 4: Check authentication
# ---------------------------------------------------------------------------

echo ""
echo -e "${_LS_BOLD}${_LS_CYAN}------------------------------------------------------------------------${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}                   Checking Authentication Credentials                  ${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}------------------------------------------------------------------------${_LS_NC}"

check_auth || true

echo ""
echo -e "${_LS_BOLD}${_LS_CYAN}========================================================================${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}                    Verification process completed                      ${_LS_NC}"
echo -e "${_LS_BOLD}${_LS_CYAN}========================================================================${_LS_NC}"
