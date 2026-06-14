# ruff: noqa: T201
#!/usr/bin/env python3
"""
🚀 Simple macOS MCP Server Demo - All 50+ Tools
"""

import json
import subprocess
import time


def run_tool(tool_name, params):
    """Run a single tool and return result"""
    try:
        # Create the command
        cmd = ["./mcp-server-macos-use"]

        # Create input JSON
        input_data = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": tool_name, "arguments": params},
        }

        # Run the command
        process = subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
        )

        # Send input
        stdout, stderr = process.communicate(input=json.dumps(input_data) + "\n", timeout=10)

        if process.returncode == 0:
            return {"success": True, "output": stdout}
        return {"success": False, "error": stderr}

    except Exception as e:
        return {"success": False, "error": str(e)}


def main():
    """Demonstrate all tools"""
    print("🚀 macOS MCP Server - All 50+ Tools Demonstration")
    print("=" * 60)

    # List of all tools to demonstrate
    tools = [
        ("macos-use_open_application_and_traverse", {"identifier": "Finder"}),
        ("macos-use_click_and_traverse", {"x": 100, "y": 100}),
        ("macos-use_type_and_traverse", {"text": "Hello"}),
        ("macos-use_press_key_and_traverse", {"keyName": "return"}),
        ("macos-use_system_control", {"action": "get_info"}),
        ("macos-use_get_time", {"timezone": "UTC"}),
        ("macos-use_applescript", {"script": 'tell application "Finder" to get name'}),
        (
            "macos-use_calendar_events",
            {"start": "2026-02-10T00:00:00Z", "end": "2026-02-10T23:59:59Z"},
        ),
        ("macos-use_reminders", {}),
        ("macos-use_notification", {"title": "Demo", "body": "Testing"}),
        ("macos-use_notes_list_folders", {}),
        ("macos-use_mail_read", {"limit": 5}),
        ("macos-use_finder_list_files", {"path": "/tmp", "limit": 5}),
        ("macos-use_list_running_apps", {}),
        ("macos-use_set_clipboard", {"content": "Demo content"}),
        ("macos-use_take_screenshot", {"path": "/tmp/demo.png"}),
        ("macos-use_perform_ocr", {"imagePath": "/tmp/demo.png"}),
        ("macos-use_voice_control", {"command": "open safari"}),
        ("macos-use_process_management", {"action": "list"}),
        (
            "macos-use_file_encryption",
            {"action": "encrypt", "path": "/tmp/test.txt", "password": "demo123"},
        ),
        ("macos-use_system_monitoring", {"metric": "cpu"}),
        ("macos-use_list_tools_dynamic", {}),
        ("macos-use_execute_command", {"command": "echo 'Hello World'"}),
        ("macos-use_spotlight_search", {"query": "demo"}),
    ]

    print(f"📊 Demonstrating {len(tools)} key tools...")
    print()

    success_count = 0
    error_count = 0

    for i, (tool_name, params) in enumerate(tools, 1):
        print(f"🔧 Tool {i:2d}: {tool_name}")
        print(f"   📋 Params: {params}")

        result = run_tool(tool_name, params)

        if result["success"]:
            # Truncate output
            output = (
                result["output"][:200] + "..." if len(result["output"]) > 200 else result["output"]
            )
            print(f"   ✅ Result: {output}")
            success_count += 1
        else:
            error_msg = (
                result["error"][:200] + "..." if len(result["error"]) > 200 else result["error"]
            )
            print(f"   ❌ Error: {error_msg}")
            error_count += 1

        print()
        time.sleep(0.5)  # Small delay

    # Summary
    print("=" * 60)
    print("🎉 Demo Complete!")
    print("📊 Summary:")
    print(f"   ✅ Successful: {success_count}")
    print(f"   ❌ Errors: {error_count}")
    print(f"   📈 Total: {len(tools)}")
    print(f"   🎯 Success Rate: {(success_count / len(tools) * 100):.1f}%")
    print("=" * 60)


if __name__ == "__main__":
    main()
