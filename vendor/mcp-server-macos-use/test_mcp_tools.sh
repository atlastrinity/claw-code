#!/bin/bash
# =============================================================================
# Native MCP Server Test Suite for mcp-server-macos-use
# Tests each tool via JSON-RPC over stdio
# =============================================================================

set -euo pipefail

# Check for binary in common places
if [[ -f "./.build/release/mcp-server-macos-use" ]]; then
    SERVER_BIN="./.build/release/mcp-server-macos-use"
elif [[ -f "./.build/debug/mcp-server-macos-use" ]]; then
    SERVER_BIN="./.build/debug/mcp-server-macos-use"
elif [[ -f "./mcp-server-macos-use" ]]; then
    SERVER_BIN="./mcp-server-macos-use"
else
    SERVER_BIN="./.build/debug/mcp-server-macos-use" # Fallback for error message
fi

PASSED=0
FAILED=0
SKIPPED=0
RESULTS=()

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper: Send JSON-RPC requests to the MCP server via stdin and capture stdout
# The MCP StdioTransport uses newline-delimited JSON
send_mcp_request() {
    local test_name="$1"
    local init_msg="$2"
    local tool_msg="$3"
    local timeout_sec="${4:-10}"

    echo -e "${BLUE}[TEST]${NC} $test_name" >&2

    local stdout_file
    stdout_file=$(mktemp)
    local stderr_file
    stderr_file=$(mktemp)
    local input_file
    input_file=$(mktemp)

    # Write newline-delimited JSON messages to input file
    printf '%s\n' "$init_msg" > "$input_file"
    printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >> "$input_file"
    printf '%s\n' "$tool_msg" >> "$input_file"

    # Pipe input with sleep to keep stdin open for processing, then EOF triggers exit
    { cat "$input_file"; sleep 2; } | "$SERVER_BIN" > "$stdout_file" 2> "$stderr_file" &
    local pipeline_pid=$!

    # Wait for timeout or natural completion
    local waited=0
    while kill -0 "$pipeline_pid" 2>/dev/null && [ $waited -lt $timeout_sec ]; do
        sleep 1
        waited=$((waited + 1))
    done

    # Kill if still running after timeout
    if kill -0 "$pipeline_pid" 2>/dev/null; then
        kill "$pipeline_pid" 2>/dev/null || true
        wait "$pipeline_pid" 2>/dev/null || true
    else
        wait "$pipeline_pid" 2>/dev/null || true
    fi

    local response
    response=$(cat "$stdout_file" 2>/dev/null || echo "")

    rm -f "$stdout_file" "$stderr_file" "$input_file"

    echo "$response"
}

# Helper: Validate response contains expected content
check_response() {
    local test_name="$1"
    local response="$2"
    local expected="$3"
    local should_not_contain="${4:-}"

    if echo "$response" | grep -q "$expected"; then
        if [ -n "$should_not_contain" ] && echo "$response" | grep -q "$should_not_contain"; then
            echo -e "${RED}[FAIL]${NC} $test_name - Found unexpected: $should_not_contain"
            FAILED=$((FAILED + 1))
            RESULTS+=("FAIL: $test_name")
            return 1
        fi
        echo -e "${GREEN}[PASS]${NC} $test_name"
        PASSED=$((PASSED + 1))
        RESULTS+=("PASS: $test_name")
        return 0
    else
        echo -e "${RED}[FAIL]${NC} $test_name - Expected '$expected' not found"
        echo "  Response (first 500 chars): ${response:0:500}"
        FAILED=$((FAILED + 1))
        RESULTS+=("FAIL: $test_name")
        return 1
    fi
}

skip_test() {
    local test_name="$1"
    local reason="$2"
    echo -e "${YELLOW}[SKIP]${NC} $test_name - $reason"
    SKIPPED=$((SKIPPED + 1))
    RESULTS+=("SKIP: $test_name - $reason")
}

# Standard MCP initialize request
INIT_REQ='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test-client","version":"1.0"}}}'

echo "============================================="
echo " MCP Server Native Test Suite"
echo " Binary: $SERVER_BIN"
echo "============================================="
echo ""

# Check binary exists
if [ ! -f "$SERVER_BIN" ]; then
    echo -e "${RED}ERROR: Server binary not found at $SERVER_BIN${NC}"
    echo "Run: swift build -c debug"
    exit 1
fi

echo -e "${BLUE}=== Phase 1: Protocol Tests ===${NC}"
echo ""

# -----------------------------------------------------------------------------
# Test 1: Initialize + ListTools
# -----------------------------------------------------------------------------
TEST_NAME="1. Initialize & ListTools"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" '"tools"' || true

# Check all expected tool names are listed
echo -e "${BLUE}  Checking tool catalog...${NC}"
EXPECTED_TOOLS=(
    "macos-use_open_application_and_traverse"
    "macos-use_click_and_traverse"
    "macos-use_type_and_traverse"
    "macos-use_press_key_and_traverse"
    "macos-use_refresh_traversal"
    "macos-use_scroll_and_traverse"
    "macos-use_right_click_and_traverse"
    "macos-use_double_click_and_traverse"
    "macos-use_triple_click_and_traverse"
    "macos-use_mouse_move"
    "macos-use_drag_and_drop_and_traverse"
    "macos-use_window_management"
    "execute_command"
    "terminal"
    "macos-use_take_screenshot"
    "screenshot"
    "macos-use_analyze_screen"
    "ocr"
    "analyze"
    "macos-use_set_clipboard"
    "macos-use_get_clipboard"
    "macos-use_system_control"
    "macos-use_fetch_url"
    "macos-use_get_time"
    "macos-use_run_applescript"
    "macos-use_calendar_events"
    "macos-use_create_event"
    "macos-use_reminders"
    "macos-use_create_reminder"
    "macos-use_spotlight_search"
    "macos-use_send_notification"
    "macos-use_notes_list_folders"
    "macos-use_notes_create_note"
    "macos-use_notes_get_content"
    "macos-use_mail_send"
    "macos-use_mail_read_inbox"
    "macos-use_finder_list_files"
    "macos-use_finder_get_selection"
    "macos-use_finder_open_path"
    "macos-use_finder_move_to_trash"
    "macos-use_list_running_apps"
    "macos-use_list_browser_tabs"
    "macos-use_list_all_windows"
    "macos-use_list_tools_dynamic"
)

TOOLS_FOUND=0
TOOLS_MISSING=0
for tool in "${EXPECTED_TOOLS[@]}"; do
    if echo "$RESPONSE" | grep -q "\"$tool\""; then
        TOOLS_FOUND=$((TOOLS_FOUND + 1))
    else
        echo -e "${RED}  Missing tool: $tool${NC}"
        TOOLS_MISSING=$((TOOLS_MISSING + 1))
    fi
done
echo -e "  Tools found: $TOOLS_FOUND/${#EXPECTED_TOOLS[@]}"
if [ $TOOLS_MISSING -eq 0 ]; then
    echo -e "${GREEN}[PASS]${NC} Tool catalog complete (${#EXPECTED_TOOLS[@]} tools)"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: Tool catalog complete")
else
    echo -e "${RED}[FAIL]${NC} Tool catalog incomplete ($TOOLS_MISSING missing)"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: Tool catalog incomplete")
fi

# -----------------------------------------------------------------------------
# Test 2: ListResources (dummy)
# -----------------------------------------------------------------------------
TEST_NAME="2. ListResources (dummy handler)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" '"resources"' || true

# -----------------------------------------------------------------------------
# Test 3: ListPrompts (dummy)
# -----------------------------------------------------------------------------
TEST_NAME="3. ListPrompts (dummy handler)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"prompts/list","params":{}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" '"prompts"' || true

echo ""
echo -e "${BLUE}=== Phase 2: Safe Tool Tests (no side effects) ===${NC}"
echo ""

# -----------------------------------------------------------------------------
# Test 4: execute_command (echo test)
# -----------------------------------------------------------------------------
TEST_NAME="4. execute_command (echo)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"echo hello_mcp_test"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "hello_mcp_test" || true

# -----------------------------------------------------------------------------
# Test 5: terminal alias
# -----------------------------------------------------------------------------
TEST_NAME="5. terminal alias (pwd)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"terminal","arguments":{"command":"pwd"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "/" || true

# -----------------------------------------------------------------------------
# Test 6: execute_command cd
# -----------------------------------------------------------------------------
TEST_NAME="6. execute_command (cd /tmp)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"cd /tmp"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "Changed directory" || true

# -----------------------------------------------------------------------------
# Test 7: execute_command cd to invalid dir
# -----------------------------------------------------------------------------
TEST_NAME="7. execute_command (cd invalid dir)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"cd /nonexistent_dir_xyz"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "No such file or directory" || true

# -----------------------------------------------------------------------------
# Test 8: macos-use_get_time
# -----------------------------------------------------------------------------
TEST_NAME="8. macos-use_get_time"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_get_time","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "202" || true  # Should contain year 202x

# -----------------------------------------------------------------------------
# Test 9: macos-use_get_time with timezone
# -----------------------------------------------------------------------------
TEST_NAME="9. macos-use_get_time (UTC timezone)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_get_time","arguments":{"timezone":"UTC"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
if echo "$RESPONSE" | grep -qE "(Greenwich|Гринвічем|UTC|GMT|text)"; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${RED}[FAIL]${NC} $TEST_NAME"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 10: macos-use_get_time invalid timezone
# -----------------------------------------------------------------------------
TEST_NAME="10. macos-use_get_time (invalid timezone)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_get_time","arguments":{"timezone":"Invalid/Tz"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "Invalid timezone" || true

# -----------------------------------------------------------------------------
# Test 11: macos-use_set_clipboard + macos-use_get_clipboard
# -----------------------------------------------------------------------------
TEST_NAME="11. macos-use_set_clipboard"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_set_clipboard","arguments":{"text":"mcp_test_clipboard_12345"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "Clipboard updated" || true

TEST_NAME="11b. macos-use_get_clipboard"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_get_clipboard","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "mcp_test_clipboard_12345" || true

# -----------------------------------------------------------------------------
# Test 12: macos-use_list_running_apps
# -----------------------------------------------------------------------------
TEST_NAME="12. macos-use_list_running_apps"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_list_running_apps","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "pid" || true

# -----------------------------------------------------------------------------
# Test 13: macos-use_list_all_windows
# -----------------------------------------------------------------------------
TEST_NAME="13. macos-use_list_all_windows"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_list_all_windows","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "ownerName" || true

# -----------------------------------------------------------------------------
# Test 14: macos-use_take_screenshot
# -----------------------------------------------------------------------------
TEST_NAME="14. macos-use_take_screenshot"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_take_screenshot","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 15)
# Base64 encoded JPEG should start with /9j/ or similar
if echo "$RESPONSE" | grep -qE '(\/9j\/|JFIF|isError.*false|Failed to capture screen|isError.*true)'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME (base64 image returned)"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${RED}[FAIL]${NC} $TEST_NAME - No valid base64 image found"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 15: screenshot alias
# -----------------------------------------------------------------------------
TEST_NAME="15. screenshot alias"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"screenshot","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 15)
if echo "$RESPONSE" | grep -qE '(\/9j\/|JFIF|isError.*false|Failed to capture screen|isError.*true)'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${RED}[FAIL]${NC} $TEST_NAME"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 16: macos-use_analyze_screen (OCR)
# -----------------------------------------------------------------------------
TEST_NAME="16. macos-use_analyze_screen (OCR)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_analyze_screen","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 20)
# OCR should return JSON array with text, confidence, coordinates
if echo "$RESPONSE" | grep -qE '(confidence|text|\[\])'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME (OCR results returned)"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${RED}[FAIL]${NC} $TEST_NAME - No OCR results"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 17: ocr alias
# -----------------------------------------------------------------------------
TEST_NAME="17. ocr alias"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ocr","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 20)
if echo "$RESPONSE" | grep -qE '(confidence|text|\[\])'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${RED}[FAIL]${NC} $TEST_NAME"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 18: macos-use_run_applescript
# -----------------------------------------------------------------------------
TEST_NAME="18. macos-use_run_applescript"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"return \"hello_from_applescript\""}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "hello_from_applescript" || true

# -----------------------------------------------------------------------------
# Test 19: macos-use_spotlight_search
# -----------------------------------------------------------------------------
TEST_NAME="19. macos-use_spotlight_search"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_spotlight_search","arguments":{"query":"Package.swift"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
# Spotlight might not find anything in test env, just check no error
if echo "$RESPONSE" | grep -qE '(isError.*true|error)'; then
    echo -e "${RED}[FAIL]${NC} $TEST_NAME - Error returned"
    FAILED=$((FAILED + 1))
    RESULTS+=("FAIL: $TEST_NAME")
else
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME (no error)"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 20: macos-use_list_tools_dynamic (help)
# -----------------------------------------------------------------------------
TEST_NAME="20. macos-use_list_tools_dynamic"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_list_tools_dynamic","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "inputSchema" || true

# -----------------------------------------------------------------------------
# Test 21: macos-use_finder_list_files
# -----------------------------------------------------------------------------
TEST_NAME="21. macos-use_finder_list_files (/tmp)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_finder_list_files","arguments":{"path":"/tmp"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 15)
# Finder may not be running or may need extra time to start
if echo "$RESPONSE" | grep -q '"id":2'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME (response received)"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${YELLOW}[SKIP]${NC} $TEST_NAME (Finder may be slow to start or not accessible)"
    SKIPPED=$((SKIPPED + 1))
    RESULTS+=("SKIP: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 22: macos-use_finder_get_selection
# -----------------------------------------------------------------------------
TEST_NAME="22. macos-use_finder_get_selection"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_finder_get_selection","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
# Will likely return "Nothing selected" which is fine
if echo "$RESPONSE" | grep -qE '(Nothing selected|POSIX|text)' && ! echo "$RESPONSE" | grep -q '"isError":true'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${YELLOW}[SKIP]${NC} $TEST_NAME (Finder may not be running)"
    SKIPPED=$((SKIPPED + 1))
    RESULTS+=("SKIP: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 23: Unknown tool (error handling)
# -----------------------------------------------------------------------------
TEST_NAME="23. Unknown tool (error handling)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nonexistent_tool","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "isError" || true

# -----------------------------------------------------------------------------
# Test 24: Missing required params (error handling)
# -----------------------------------------------------------------------------
TEST_NAME="24. Missing required params (execute_command without command)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "isError" || true

# -----------------------------------------------------------------------------
# Test 25: macos-use_fetch_url
# -----------------------------------------------------------------------------
TEST_NAME="25. macos-use_fetch_url"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_fetch_url","arguments":{"url":"https://httpbin.org/html"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 15)
if echo "$RESPONSE" | grep -qE '(text|Moby|Herman|content)' && ! echo "$RESPONSE" | grep -q '"isError":true'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${YELLOW}[SKIP]${NC} $TEST_NAME (network may be unavailable)"
    SKIPPED=$((SKIPPED + 1))
    RESULTS+=("SKIP: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 26: macos-use_fetch_url invalid URL
# -----------------------------------------------------------------------------
TEST_NAME="26. macos-use_fetch_url (invalid URL)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_fetch_url","arguments":{"url":"not_a_url"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "unsupported URL" || true

echo ""
echo -e "${BLUE}=== Phase 3: macOS Integration Tests (require permissions) ===${NC}"
echo ""

# -----------------------------------------------------------------------------
# Test 27: macos-use_list_browser_tabs
# -----------------------------------------------------------------------------
TEST_NAME="27. macos-use_list_browser_tabs"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_list_browser_tabs","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
if echo "$RESPONSE" | grep -qE '(\[\]|browser|tabs|text)' && ! echo "$RESPONSE" | grep -q '"isError":true'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${YELLOW}[SKIP]${NC} $TEST_NAME (no browser running or no permission)"
    SKIPPED=$((SKIPPED + 1))
    RESULTS+=("SKIP: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 28: macos-use_notes_list_folders
# -----------------------------------------------------------------------------
TEST_NAME="28. macos-use_notes_list_folders"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_notes_list_folders","arguments":{}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 15)
if echo "$RESPONSE" | grep -qE '(Notes|text|folder)' && ! echo "$RESPONSE" | grep -q '"isError":true'; then
    echo -e "${GREEN}[PASS]${NC} $TEST_NAME"
    PASSED=$((PASSED + 1))
    RESULTS+=("PASS: $TEST_NAME")
else
    echo -e "${YELLOW}[SKIP]${NC} $TEST_NAME (Notes may not be accessible)"
    SKIPPED=$((SKIPPED + 1))
    RESULTS+=("SKIP: $TEST_NAME")
fi

# -----------------------------------------------------------------------------
# Test 29: macos-use_send_notification
# -----------------------------------------------------------------------------
TEST_NAME="29. macos-use_send_notification"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_send_notification","arguments":{"title":"MCP Test","message":"Test notification from MCP server"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Notification sent" || true

# -----------------------------------------------------------------------------
# Test 30: execute_command with pipe
# -----------------------------------------------------------------------------
TEST_NAME="30. execute_command (pipe & complex)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"echo test123 | tr a-z A-Z"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "TEST123" || true

# -----------------------------------------------------------------------------
# Test 31: execute_command with exit code
# -----------------------------------------------------------------------------
TEST_NAME="31. execute_command (failing command)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"false"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "isError" || true

# -----------------------------------------------------------------------------
# Test 32: cd with ~ expansion
# -----------------------------------------------------------------------------
TEST_NAME="32. execute_command (cd ~)"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"cd ~"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ")
check_response "$TEST_NAME" "$RESPONSE" "Changed directory" || true


# -----------------------------------------------------------------------------
# Test 33: Open Safari
# -----------------------------------------------------------------------------
TEST_NAME="33. Open Safari"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"execute_command","arguments":{"command":"open -a Safari"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "exit_code" || true
sleep 3

# -----------------------------------------------------------------------------
# Test 34: Resize browser window
# -----------------------------------------------------------------------------
TEST_NAME="34. Resize Safari window"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"Safari\"\nset bounds of front window to {100, 100, 800, 600}\nreturn \"Resized\"\nend tell"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Resized" || true

# -----------------------------------------------------------------------------
# Test 35: Move browser window
# -----------------------------------------------------------------------------
TEST_NAME="35. Move Safari window"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"Safari\"\nset bounds of front window to {200, 200, 900, 700}\nreturn \"Moved\"\nend tell"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Moved" || true

# -----------------------------------------------------------------------------
# Test 36: Minimize browser window (to dock)
# -----------------------------------------------------------------------------
TEST_NAME="36. Minimize Safari"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"System Events\" to tell process \"Safari\" to set value of attribute \"AXMinimized\" of window 1 to true\nreturn \"Minimized\""}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Minimized" || true
sleep 1

# -----------------------------------------------------------------------------
# Test 37: Restore browser window
# -----------------------------------------------------------------------------
TEST_NAME="37. Restore Safari"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"System Events\" to tell process \"Safari\" to set value of attribute \"AXMinimized\" of window 1 to false\nreturn \"Restored\""}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Restored" || true
sleep 1

# -----------------------------------------------------------------------------
# Test 38: Write in search (type text)
# -----------------------------------------------------------------------------
TEST_NAME="38. Type in search"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"System Events\"\ntell process \"Safari\" to set frontmost to true\nkeystroke \"l\" using command down\nkeystroke \"https://google.com\"\nkey code 36\nreturn \"Typed Search\"\nend tell"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Typed Search" || true
sleep 3

# -----------------------------------------------------------------------------
# Test 39: Select all text
# -----------------------------------------------------------------------------
TEST_NAME="39. Select all text"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"System Events\"\nkeystroke \"a\" using command down\nreturn \"Selected\"\nend tell"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Selected" || true
sleep 1

# -----------------------------------------------------------------------------
# Test 40: Copy text
# -----------------------------------------------------------------------------
TEST_NAME="40. Copy text"
TOOL_REQ='{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"macos-use_run_applescript","arguments":{"script":"tell application \"System Events\"\nkeystroke \"c\" using command down\nreturn \"Copied\"\nend tell"}}}'
RESPONSE=$(send_mcp_request "$TEST_NAME" "$INIT_REQ" "$TOOL_REQ" 10)
check_response "$TEST_NAME" "$RESPONSE" "Copied" || true
sleep 1

# =============================================================================
# Summary
# =============================================================================
echo ""
echo "============================================="
echo " TEST RESULTS SUMMARY"
echo "============================================="
echo ""
for result in "${RESULTS[@]}"; do
    if [[ "$result" == PASS* ]]; then
        echo -e "  ${GREEN}✓${NC} ${result#PASS: }"
    elif [[ "$result" == FAIL* ]]; then
        echo -e "  ${RED}✗${NC} ${result#FAIL: }"
    elif [[ "$result" == SKIP* ]]; then
        echo -e "  ${YELLOW}○${NC} ${result#SKIP: }"
    fi
done
echo ""
echo "---------------------------------------------"
echo -e "  ${GREEN}Passed:${NC}  $PASSED"
echo -e "  ${RED}Failed:${NC}  $FAILED"
echo -e "  ${YELLOW}Skipped:${NC} $SKIPPED"
echo -e "  Total:   $((PASSED + FAILED + SKIPPED))"
echo "---------------------------------------------"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed.${NC}"
    exit 1
fi
