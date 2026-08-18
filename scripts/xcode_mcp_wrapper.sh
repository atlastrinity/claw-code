#!/usr/bin/env bash
# Wrapper for Xcode MCP Bridge (xcrun mcpbridge)
# Automatically locates running Xcode GUI PID to prevent 30s timeout

XCODE_PID=$(pgrep -f "/Applications/Xcode.app/Contents/MacOS/Xcode" 2>/dev/null | head -n 1 || true)
if [ -n "$XCODE_PID" ]; then
    export MCP_XCODE_PID="$XCODE_PID"
fi

exec /Applications/Xcode.app/Contents/Developer/usr/bin/mcpbridge "$@"
