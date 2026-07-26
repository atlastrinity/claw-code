#!/bin/bash
export CLAW_NEW_SESSION=true
export CLAW_BYPASS_WORKSPACE_CHECK=true
export CLICOLOR_FORCE=1
export FORCE_COLOR=true
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/run_claw.sh" "$@"
