#!/usr/bin/env bash

# Color codes for pretty terminal output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

echo -e "${BOLD}${CYAN}========================================================================${NC}"
echo -e "${BOLD}${CYAN}                Claw MCP Servers Installer & Verifier                    ${NC}"
echo -e "${BOLD}${CYAN}========================================================================${NC}"
echo ""

# Ensure we are in the script directory
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Verify environment variables are loaded
if [ -f "$HOME/.claw/.env" ]; then
    source "$HOME/.claw/.env"
fi

# 1. Update stale extension paths in ~/.claw/settings.json & .claw.json
echo -e "${BOLD}Updating IDE extension paths in settings.json...${NC}"
python3 -c '
import json, os, glob

def update_mcp_paths(json_path):
    if not os.path.exists(json_path):
        return
    with open(json_path, "r") as f:
        data = json.load(f)
    
    avail = data.get("availableMcpServers", {})
    updated = False
    
    bundles = glob.glob(os.path.expanduser("~/.antigravity-ide/extensions/googlecloudtools.datacloud-*/mcp_servers/cli/mcp_proxy_bundle.js"))
    if bundles:
        valid_bundle = bundles[-1]
        for key in ["notebooks", "visualization"]:
            if key in avail:
                args = avail[key].get("args", [])
                if args and args[0] != valid_bundle:
                    args[0] = valid_bundle
                    updated = True
                    print(f"  -> Updated {key} path to: {valid_bundle}")
    
    if updated:
        with open(json_path, "w") as f:
            json.dump(data, f, indent=2)

update_mcp_paths(os.path.expanduser("~/.claw/settings.json"))
update_mcp_paths(os.path.expanduser("'"${SCRIPT_DIR}"'/.claw.json"))
'

echo ""
echo -e "${BOLD}${CYAN}------------------------------------------------------------------------${NC}"
echo -e "${BOLD}${CYAN}                     Verifying 10 MCP Servers                           ${NC}"
echo -e "${BOLD}${CYAN}------------------------------------------------------------------------${NC}"

verify_command() {
    local name="$1"
    local check_cmd="$2"
    local install_cmd="$3"
    
    echo -e "Checking ${BOLD}$name${NC}..."
    
    if eval "$check_cmd" </dev/null >/dev/null 2>&1; then
        echo -e "  -> Status: ${GREEN}✅ Installed & Verified${NC}"
        return 0
    else
        echo -e "  -> Status: ${YELLOW}⚠️  Not found or failed. Attempting to install/refresh...${NC}"
        if [ -n "$install_cmd" ]; then
            echo -e "  -> Running: $install_cmd"
            if eval "$install_cmd" >/dev/null 2>&1; then
                # Re-verify
                if eval "$check_cmd" </dev/null >/dev/null 2>&1; then
                    echo -e "  -> Status: ${GREEN}✅ Installed & Verified successfully${NC}"
                    return 0
                fi
            fi
        fi
        echo -e "  -> Status: ${RED}❌ Verification failed${NC}"
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
    "npx --prefer-offline --package mcp-remote mcp-remote --version >/dev/null 2>&1 || [ -n \"$(command -v npx)\" ]" \
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
    "[ -f \"$HOME/.antigravity-ide/extensions/googlecloudtools.datacloud-\"*\"/mcp_servers/cli/mcp_proxy_bundle.js\" ]" \
    "echo 'Please verify the Google Cloud Tools extension is installed in your IDE.'"

# 9. visualization
verify_command "Visualization Proxy Extension" \
    "[ -f \"$HOME/.antigravity-ide/extensions/googlecloudtools.datacloud-\"*\"/mcp_servers/cli/mcp_proxy_bundle.js\" ]" \
    "echo 'Please verify the Google Cloud Tools extension is installed in your IDE.'"

# 10. pyscn-mcp
verify_command "PyScn MCP (pyscn-mcp)" \
    "uvx pyscn-mcp --help" \
    "uv tool install pyscn-mcp"

echo ""
echo -e "${BOLD}${CYAN}------------------------------------------------------------------------${NC}"
echo -e "${BOLD}${CYAN}                   Checking Authentication Credentials                  ${NC}"
echo -e "${BOLD}${CYAN}------------------------------------------------------------------------${NC}"

if npx --prefer-offline firebase-tools projects:list >/dev/null 2>&1; then
    echo -e "Firebase CLI: ${GREEN}✅ Authenticated${NC}"
else
    echo -e "Firebase CLI: ${YELLOW}⚠️  Not authenticated (run 'npx firebase login')${NC}"
fi

if gcloud auth application-default print-access-token >/dev/null 2>&1; then
    echo -e "GCP ADC: ${GREEN}✅ Authenticated${NC}"
else
    echo -e "GCP ADC: ${YELLOW}⚠️  Not authenticated (run 'gcloud auth application-default login')${NC}"
fi

echo ""
echo -e "${BOLD}${CYAN}========================================================================${NC}"
echo -e "${BOLD}${CYAN}                    Verification process completed                      ${NC}"
echo -e "${BOLD}${CYAN}========================================================================${NC}"
