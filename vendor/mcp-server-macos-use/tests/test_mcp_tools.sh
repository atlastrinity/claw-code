#!/bin/bash
(
echo '{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0.0"}}}'
sleep 0.1
echo '{"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}'
sleep 1
) | /Users/dev/Documents/GitHub/claw-code/vendor/mcp-server-macos-use/.build/release/mcp-server-macos-use
