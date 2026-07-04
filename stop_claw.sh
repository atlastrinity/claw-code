#!/usr/bin/env bash

echo "🛑 Stopping all claw-related processes..."

# Helper function to kill processes gracefully then forcefully by tracking PIDs
kill_process() {
    local pattern="$1"
    local exact="$2"
    local pids
    
    if [ "$exact" = true ]; then
        pids=$(pgrep -x "$pattern" 2>/dev/null || true)
    else
        pids=$(pgrep -f "$pattern" 2>/dev/null || true)
    fi
    
    # Remove any empty lines or whitespace to normalize
    pids=$(echo "$pids" | tr '\n' ' ' | xargs)
    
    if [ -n "$pids" ]; then
        echo "Killing processes matching '$pattern' (PIDs: $pids)..."
        kill -15 $pids 2>/dev/null || true
        
        sleep 1.5
        
        local remaining=""
        for pid in $pids; do
            if kill -0 "$pid" 2>/dev/null; then
                remaining="$remaining $pid"
            fi
        done
        
        remaining=$(echo "$remaining" | xargs)
        if [ -n "$remaining" ]; then
            echo "Force killing remaining processes: $remaining..."
            kill -9 $remaining 2>/dev/null || true
        fi
    fi
}

# Kill launcher wrappers first so they do not auto-restart claw
kill_process "run_claw.sh" false
kill_process "run_claw_new_session.sh" false

# Kill voice narrator script
kill_process "claw_voice_narrator.py" false

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
