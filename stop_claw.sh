#!/usr/bin/env bash

echo "🛑 Stopping all claw-related processes..."

# Helper function to kill processes gracefully then forcefully
kill_process() {
    local pattern="$1"
    local exact="$2"
    
    if [ "$exact" = true ]; then
        if pgrep -x "$pattern" > /dev/null; then
            echo "Killing processes matching exact name: '$pattern'..."
            pkill -x "$pattern"
            sleep 1
            if pgrep -x "$pattern" > /dev/null; then
                echo "Force killing '$pattern'..."
                pkill -9 -x "$pattern"
            fi
        fi
    else
        if pgrep -f "$pattern" > /dev/null; then
            echo "Killing processes matching: '$pattern'..."
            pkill -f "$pattern"
            sleep 1
            if pgrep -f "$pattern" > /dev/null; then
                echo "Force killing '$pattern'..."
                pkill -9 -f "$pattern"
            fi
        fi
    fi
}

kill_process "claw" true
kill_process "claw-analog" true
kill_process "claw-rag-service" true

# Kill cargo commands (both run and test) and the compiled test binaries
kill_process "cargo run.*claw" false
kill_process "cargo test.*" false
kill_process "deps/claw-" false

# Kill node/MCP server processes spawned by claw
kill_process "ios-simulator-mcp" false
kill_process "mcpbridge" false

# Kill iOS simulator auxiliary daemon processes
kill_process "idb_companion" false
kill_process "bin/idb " false

echo "✅ All claw processes have been terminated."
