#!/usr/bin/env bash

echo "🛑 Stopping all claw-related processes..."

# Kill the main claw binary
if pgrep -x "claw" > /dev/null; then
    echo "Killing 'claw' processes..."
    pkill -x "claw"
fi

# Kill the claw-analog wrapper if running
if pgrep -x "claw-analog" > /dev/null; then
    echo "Killing 'claw-analog' processes..."
    pkill -x "claw-analog"
fi

# Kill the claw-rag-service backend
if pgrep -x "claw-rag-service" > /dev/null; then
    echo "Killing 'claw-rag-service' processes..."
    pkill -x "claw-rag-service"
fi

# Kill node/MCP server processes spawned by claw
# Be careful to only kill those that look like our MCP servers
echo "Killing XcodeBuildMCP processes..."
pkill -f "XcodeBuildMCP/build/index.js" || true

echo "✅ All claw processes have been terminated."
