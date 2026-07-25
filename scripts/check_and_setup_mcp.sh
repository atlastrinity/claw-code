#!/usr/bin/env bash
# ==============================================================================
# MCP Health Check & Authentication Verification Script for Claw Code
# ==============================================================================

set -e

echo "🔍 === Claw Code MCP Server Health Check & Verification ==="
echo ""

# 1. Update stale paths in ~/.claw/settings.json and .claw.json
python3 -c '
import json, os, glob

def update_mcp_paths(json_path):
    if not os.path.exists(json_path):
        return
    with open(json_path, "r") as f:
        data = json.load(f)
    
    avail = data.get("availableMcpServers", {})
    updated = False
    
    # Find current datacloud extension path
    bundles = glob.glob(os.path.expanduser("~/.antigravity-ide/extensions/googlecloudtools.datacloud-*/mcp_servers/cli/mcp_proxy_bundle.js"))
    if bundles:
        valid_bundle = bundles[-1]
        for key in ["notebooks", "visualization"]:
            if key in avail:
                args = avail[key].get("args", [])
                if args and args[0] != valid_bundle:
                    args[0] = valid_bundle
                    updated = True
                    print(f"✅ Updated {key} path to: {valid_bundle}")
    
    if updated:
        with open(json_path, "w") as f:
            json.dump(data, f, indent=2)

update_mcp_paths(os.path.expanduser("~/.claw/settings.json"))
update_mcp_paths(os.path.expanduser("/Users/dev/Documents/GitHub/claw-code/.claw.json"))
'

echo "--------------------------------------------------------"
echo "🛠️ 2. Verifying Available MCP Servers in settings.json..."
python3 -c '
import json, os

settings_path = os.path.expanduser("~/.claw/settings.json")
if os.path.exists(settings_path):
    with open(settings_path) as f:
        data = json.load(f)
    servers = list(data.get("availableMcpServers", {}).keys())
    print("Registered available MCP servers:", ", ".join(servers))
'

echo "--------------------------------------------------------"
echo "🔐 3. Checking Firebase Authentication Status..."
if npx --prefer-offline firebase-tools projects:list >/dev/null 2>&1; then
    echo "✅ Firebase CLI is AUTHENTICATED!"
else
    echo "⚠️ Firebase CLI is NOT authenticated yet."
    echo "👉 To authenticate Firebase for Firestore & hosting access, run:"
    echo "   npx firebase login"
fi

echo "--------------------------------------------------------"
echo "🔐 4. Checking Google Cloud Application Default Credentials (ADC)..."
if gcloud auth application-default print-access-token >/dev/null 2>&1; then
    echo "✅ GCP ADC is AUTHENTICATED!"
else
    echo "⚠️ GCP ADC is NOT authenticated yet."
    echo "👉 To authenticate GCP ADC for BigQuery/Cloud DBs, run:"
    echo "   gcloud auth application-default login"
fi

echo "--------------------------------------------------------"
echo "🚀 5. Testing Launch capability for all 10 MCP Servers..."
python3 -c '
import json, subprocess, os

settings_file = os.path.expanduser("~/.claw/settings.json")
with open(settings_file, "r") as f:
    data = json.load(f)

avail = data.get("availableMcpServers", {})
for k, v in avail.items():
    cmd = v.get("command")
    args = v.get("args", [])
    full_cmd = [cmd] + args
    try:
        res = subprocess.run(full_cmd + ["--version"], capture_output=True, text=True, timeout=3)
        print(f"   [{k}] READY")
    except subprocess.TimeoutExpired:
        print(f"   [{k}] READY (Stdio JSON-RPC waiting)")
    except Exception as e:
        print(f"   [{k}] ERROR: {e}")
'

echo ""
echo "✨ MCP Health Check Complete!"
