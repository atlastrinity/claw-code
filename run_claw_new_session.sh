#!/bin/bash
export CLAW_NEW_SESSION=true
export CLAW_BYPASS_WORKSPACE_CHECK=true
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/run_claw.sh" --permission-mode danger-full-access "$@"
