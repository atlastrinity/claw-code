#!/usr/bin/env bash
# Wrapper for Xcode MCP Bridge (xcrun mcpbridge)
# Automatically locates running Xcode GUI PID to prevent 30s timeout

XCODE_PID=$(pgrep -x Xcode 2>/dev/null | head -n 1 || true)
if [ -z "$XCODE_PID" ]; then
    XCODE_PID=$(pgrep -f "Xcode.app/Contents/MacOS/Xcode" 2>/dev/null | head -n 1 || true)
fi
if [ -n "$XCODE_PID" ]; then
    export MCP_XCODE_PID="$XCODE_PID"
fi

exec /Applications/Xcode.app/Contents/Developer/usr/bin/mcpbridge "$@"
