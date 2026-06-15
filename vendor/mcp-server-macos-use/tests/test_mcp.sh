#!/bin/bash
BINARY="./vendor/mcp-server-macos-use/.build/release/mcp-server-macos-use"

# Send initialize
echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}' | $BINARY
echo "\n---"

# Send tools/list
echo '{"jsonrpc":"2.0","method":"tools/list","id":2}' | $BINARY
echo "\n---"

# Send tools/call
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"execute_command","arguments":{"command":"echo Hello from MCP"}},"id":3}' | $BINARY
echo "\n---"
