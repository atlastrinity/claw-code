#!/usr/bin/env bash
# ==============================================================================
# lib_sync.sh — Shared utility library for Claw Code scripts
# ==============================================================================
#
# Provides common functions for:
#   - Bidirectional file synchronization (local ↔ global)
#   - MCP extension path updates in JSON configs
#   - Authentication status checks (Firebase CLI, GCP ADC)
#
# Usage:
#   SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
#   source "${SCRIPT_DIR}/scripts/lib_sync.sh"
#   # or from within scripts/:
#   source "$(cd "$(dirname "$0")" && pwd)/lib_sync.sh"
#
# Depends on: python3, bash 3.2+
# ==============================================================================

# Prevent double-sourcing
if [ -n "${_LIB_SYNC_LOADED:-}" ]; then
    return 0 2>/dev/null || true
fi
_LIB_SYNC_LOADED=1

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

GLOBAL_CLAW_DIR="${GLOBAL_CLAW_DIR:-$HOME/.claw}"
GLOBAL_SETTINGS="${GLOBAL_SETTINGS:-${GLOBAL_CLAW_DIR}/settings.json}"
GLOBAL_BIN_DIR="${GLOBAL_BIN_DIR:-${GLOBAL_CLAW_DIR}/bin}"

# Colors (only if stdout is a terminal)
if [ -t 1 ] 2>/dev/null; then
    _LS_GREEN='\033[0;32m'
    _LS_YELLOW='\033[0;33m'
    _LS_RED='\033[0;31m'
    _LS_CYAN='\033[0;36m'
    _LS_BOLD='\033[1m'
    _LS_NC='\033[0m'
else
    _LS_GREEN='' _LS_YELLOW='' _LS_RED='' _LS_CYAN='' _LS_BOLD='' _LS_NC=''
fi

# ---------------------------------------------------------------------------
# sync_file — Bidirectional file synchronization by modification timestamp
# ---------------------------------------------------------------------------
# Usage: sync_file <local_path> <global_path> [label]
#
# Rules:
#   - If only one file exists, copy to the missing side
#   - If both exist, the newer file overwrites the older one
#   - If timestamps are equal, no action taken
#
sync_file() {
    local local_path="$1"
    local global_path="$2"
    local label="${3:-$(basename "$local_path")}"

    if [ -f "$local_path" ] && [ ! -f "$global_path" ]; then
        mkdir -p "$(dirname "$global_path")"
        cp "$local_path" "$global_path"
        echo -e "  ${_LS_CYAN}→${_LS_NC} ${label}: local → global (created)"
    elif [ ! -f "$local_path" ] && [ -f "$global_path" ]; then
        mkdir -p "$(dirname "$local_path")"
        cp "$global_path" "$local_path"
        echo -e "  ${_LS_CYAN}←${_LS_NC} ${label}: global → local (created)"
    elif [ -f "$local_path" ] && [ -f "$global_path" ]; then
        if [ "$local_path" -nt "$global_path" ]; then
            cp "$local_path" "$global_path"
            echo -e "  ${_LS_CYAN}→${_LS_NC} ${label}: local → global (newer)"
        elif [ "$global_path" -nt "$local_path" ]; then
            cp "$global_path" "$local_path"
            echo -e "  ${_LS_CYAN}←${_LS_NC} ${label}: global → local (newer)"
        else
            echo -e "  ${_LS_GREEN}=${_LS_NC} ${label}: in sync"
        fi
    else
        echo -e "  ${_LS_YELLOW}!${_LS_NC} ${label}: missing on both sides"
    fi
}

# ---------------------------------------------------------------------------
# sync_settings — Sync .claw.json ↔ ~/.claw/settings.json
# ---------------------------------------------------------------------------
# Usage: sync_settings <project_root>
#
sync_settings() {
    local project_root="$1"
    local local_settings="${project_root}/.claw.json"

    echo -e "${_LS_BOLD}Synchronizing settings...${_LS_NC}"
    sync_file "$local_settings" "$GLOBAL_SETTINGS" "settings.json"
}

# ---------------------------------------------------------------------------
# sync_all — Full sync of settings, CLAW.md, .env, skills
# ---------------------------------------------------------------------------
# Usage: sync_all <project_root>
#
sync_all() {
    local project_root="$1"

    echo -e "${_LS_BOLD}Synchronizing all configurations...${_LS_NC}"
    sync_file "${project_root}/.claw.json"   "$GLOBAL_SETTINGS"          "settings.json"
    sync_file "${project_root}/CLAW.md"      "${GLOBAL_CLAW_DIR}/CLAW.md" "CLAW.md"
    sync_file "${project_root}/.env"         "${GLOBAL_CLAW_DIR}/.env"    ".env"

    # Bidirectional skills sync via rsync
    if [ -d "${project_root}/.claw/skills" ] || [ -d "${GLOBAL_CLAW_DIR}/skills" ]; then
        mkdir -p "${project_root}/.claw/skills" "${GLOBAL_CLAW_DIR}/skills"
        rsync -au --exclude=".build" --exclude=".git" \
            "${project_root}/.claw/skills/" "${GLOBAL_CLAW_DIR}/skills/"
        rsync -au --exclude=".build" --exclude=".git" \
            "${GLOBAL_CLAW_DIR}/skills/" "${project_root}/.claw/skills/"
        echo -e "  ${_LS_GREEN}↔${_LS_NC} skills: bidirectional sync complete"
    fi

    # Dynamically pull and sync MCP servers across global and local settings
    update_mcp_paths "${project_root}/.claw.json" "$GLOBAL_SETTINGS" "${CLAW_CALLER_CWD:-.}/.claw.json"
}

# ---------------------------------------------------------------------------
# update_mcp_paths — Fix stale extension paths in JSON configs
# ---------------------------------------------------------------------------
# Usage: update_mcp_paths [json_path ...]
#
# If no arguments provided, updates both global and local (if SCRIPT_DIR is set).
#
update_mcp_paths() {
    local json_paths=("$@")

    # Default: update both global and local settings
    if [ ${#json_paths[@]} -eq 0 ]; then
        json_paths=("$GLOBAL_SETTINGS")
        # Try to find a local .claw.json relative to SCRIPT_DIR
        if [ -n "${SCRIPT_DIR:-}" ] && [ -f "${SCRIPT_DIR}/.claw.json" ]; then
            json_paths+=("${SCRIPT_DIR}/.claw.json")
        fi
        if [ -n "${CLAW_CALLER_CWD:-}" ] && [ -f "${CLAW_CALLER_CWD}/.claw.json" ]; then
            json_paths+=("${CLAW_CALLER_CWD}/.claw.json")
        fi
    fi

    python3 -c '
import json, os, glob, sys

paths = [p for p in sys.argv[1:] if p]
all_avail = {}
valid_paths = [p for p in paths if os.path.exists(p)]

# 1. Accumulate availableMcpServers across all configs
for path in valid_paths:
    try:
        with open(path, "r") as f:
            d = json.load(f)
        avail = d.get("availableMcpServers", {})
        if isinstance(avail, dict):
            for k, v in avail.items():
                if k not in all_avail:
                    all_avail[k] = v
    except Exception:
        pass

# Find proxy bundle path if available
bundles = glob.glob(os.path.expanduser(
    "~/.antigravity-ide/extensions/googlecloudtools.datacloud-*/mcp_servers/cli/mcp_proxy_bundle.js"
))
valid_bundle = sorted(bundles)[-1] if bundles else None

# 2. Update each file with merged availableMcpServers and sync mcpServers
for path in valid_paths:
    try:
        with open(path, "r") as f:
            data = json.load(f)
        updated = False
        
        # Merge availableMcpServers
        avail = data.get("availableMcpServers", {})
        if not isinstance(avail, dict):
            avail = {}
        for k, v in all_avail.items():
            if k not in avail:
                avail[k] = v
                updated = True
        data["availableMcpServers"] = avail

        # Dynamically pull/sync missing servers from availableMcpServers into mcpServers
        mcp = data.get("mcpServers")
        if not isinstance(mcp, dict):
            mcp = {}
            updated = True
        
        for k, v in avail.items():
            if k not in mcp:
                mcp[k] = v
                updated = True
            elif valid_bundle and k in ["notebooks", "visualization"]:
                args = mcp[k].get("args", [])
                if args and args[0] != valid_bundle:
                    args[0] = valid_bundle
                    updated = True

        # Update proxy bundle paths in availableMcpServers as well
        if valid_bundle:
            for key in ["notebooks", "visualization"]:
                if key in avail and isinstance(avail[key], dict):
                    args = avail[key].get("args", [])
                    if args and args[0] != valid_bundle:
                        args[0] = valid_bundle
                        updated = True
        
        data["mcpServers"] = mcp

        if updated:
            with open(path, "w") as f:
                json.dump(data, f, indent=2)
            print(f"  ✅ Dynamically synced MCP servers in {os.path.basename(path)}")
    except Exception as e:
        print(f"  ⚠️ Error syncing MCP servers in {path}: {e}")
' "${json_paths[@]}"
}

# ---------------------------------------------------------------------------
# check_auth — Verify authentication status for Firebase CLI and GCP ADC
# ---------------------------------------------------------------------------
# Usage: check_auth
#
# Returns 0 if all checks pass, 1 if any fail (but does not exit).
#
check_auth() {
    local all_ok=0

    echo -e "${_LS_BOLD}Checking authentication credentials...${_LS_NC}"

    # Firebase CLI
    if npx --prefer-offline firebase-tools projects:list >/dev/null 2>&1; then
        echo -e "  ${_LS_GREEN}✅${_LS_NC} Firebase CLI: Authenticated"
    else
        echo -e "  ${_LS_YELLOW}⚠️${_LS_NC}  Firebase CLI: Not authenticated"
        echo -e "     ${_LS_CYAN}→${_LS_NC} Run: npx firebase login"
        all_ok=1
    fi

    # GCP ADC
    if gcloud auth application-default print-access-token >/dev/null 2>&1; then
        echo -e "  ${_LS_GREEN}✅${_LS_NC} GCP ADC: Authenticated"
    else
        echo -e "  ${_LS_YELLOW}⚠️${_LS_NC}  GCP ADC: Not authenticated"
        echo -e "     ${_LS_CYAN}→${_LS_NC} Run: gcloud auth application-default login"
        all_ok=1
    fi

    return $all_ok
}

# ---------------------------------------------------------------------------
# copy_binaries — Copy compiled binaries to global bin directory
# ---------------------------------------------------------------------------
# Usage: copy_binaries <rust_target_dir> [profile]
#
# profile defaults to "release"
#
copy_binaries() {
    local rust_target_dir="$1"
    local profile="${2:-release}"
    local src_dir="${rust_target_dir}/${profile}"

    echo -e "${_LS_BOLD}Copying binaries to ${GLOBAL_BIN_DIR}...${_LS_NC}"
    mkdir -p "$GLOBAL_BIN_DIR"

    local binaries=("claw" "claw-analog" "claw-rag-service")
    for bin in "${binaries[@]}"; do
        if [ -x "${src_dir}/${bin}" ]; then
            rm -f "${GLOBAL_BIN_DIR}/${bin}"
            cp "${src_dir}/${bin}" "${GLOBAL_BIN_DIR}/"
            echo -e "  ${_LS_GREEN}✅${_LS_NC} ${bin}"
        else
            echo -e "  ${_LS_YELLOW}⚠️${_LS_NC}  ${bin}: not found in ${src_dir}"
        fi
    done

    # Re-sign on macOS
    if [ "$(uname)" = "Darwin" ]; then
        echo -e "  ${_LS_CYAN}→${_LS_NC} Re-signing binaries for macOS..."
        for bin in "${binaries[@]}"; do
            if [ -x "${GLOBAL_BIN_DIR}/${bin}" ]; then
                codesign -s - -f "${GLOBAL_BIN_DIR}/${bin}" 2>/dev/null || true
            fi
        done
    fi
}
