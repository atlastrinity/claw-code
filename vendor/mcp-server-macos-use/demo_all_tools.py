# ruff: noqa: T201
#!/usr/bin/env python3
"""
🚀 macOS MCP Server - All 50+ Tools Demonstration
This script demonstrates all available tools with real examples
"""

import asyncio
import json
import sys
from pathlib import Path

# Add project root to path
sys.path.append(str(Path(__file__).parent))

from brain.src.brain.mcp_manager import MCPManager


async def demo_all_tools():
    """Demonstrate all 50+ macOS MCP tools"""

    print("🚀 Starting macOS MCP Server - All Tools Demonstration")
    print("=" * 60)

    manager = MCPManager()

    # All 50 tools with their parameters
    tools_to_test = [
        # Basic Automation (10 tools)
        ("macos-use_open_application_and_traverse", {"identifier": "Finder"}),
        ("macos-use_click_and_traverse", {"x": 100, "y": 100, "pid": 0}),
        ("macos-use_right_click_and_traverse", {"x": 100, "y": 100, "pid": 0}),
        ("macos-use_double_click_and_traverse", {"x": 100, "y": 100, "pid": 0}),
        (
            "macos-use_drag_and_drop_and_traverse",
            {"startX": 100, "startY": 100, "endX": 200, "endY": 200, "pid": 0},
        ),
        ("macos-use_type_and_traverse", {"text": "Hello World", "pid": 0}),
        ("macos-use_press_key_and_traverse", {"keyName": "return", "pid": 0}),
        ("macos-use_scroll_and_traverse", {"direction": "down", "amount": 3, "pid": 0}),
        ("macos-use_refresh_traversal", {"pid": 0}),
        ("macos-use_window_management", {"action": "list"}),
        # System Control (5 tools)
        ("macos-use_system_control", {"action": "get_info"}),
        ("macos-use_fetch_url", {"url": "https://www.apple.com"}),
        ("macos-use_get_time", {"timezone": "UTC", "format": "readable"}),
        ("macos-use_countdown", {"seconds": 5, "message": "Demo complete!"}),
        # AppleScript (2 tools)
        ("macos-use_applescript", {"script": 'tell application "Finder" to get name'}),
        ("macos-use_applescript_templates", {"list": True}),
        # Calendar & Reminders (4 tools)
        (
            "macos-use_calendar_events",
            {"start": "2026-02-10T00:00:00Z", "end": "2026-02-10T23:59:59Z"},
        ),
        ("macos-use_create_event", {"title": "Demo Event", "date": "2026-02-10T15:00:00Z"}),
        ("macos-use_reminders", {}),
        ("macos-use_create_reminder", {"title": "Demo Reminder"}),
        # Notifications (2 tools)
        ("macos-use_notification", {"title": "Demo", "body": "Testing notifications"}),
        (
            "macos-use_notification_schedule",
            {"title": "Scheduled", "body": "In 5 seconds", "delay": 5},
        ),
        # Notes (3 tools)
        ("macos-use_notes_list_folders", {}),
        (
            "macos-use_notes_create",
            {"folder": "Demo", "title": "Test Note", "content": "Demo content"},
        ),
        ("macos-use_notes_get", {"note": "Test Note"}),
        # Mail (2 tools)
        ("macos-use_mail_send", {"to": "test@example.com", "subject": "Test", "body": "Demo"}),
        ("macos-use_mail_read", {"limit": 5}),
        # Finder (4 tools)
        ("macos-use_finder_list_files", {"path": "/tmp", "limit": 5}),
        ("macos-use_finder_get_selection", {}),
        ("macos-use_finder_open_path", {"path": "/tmp"}),
        ("macos-use_finder_move_to_trash", {"path": "/tmp/test_file.txt"}),
        # System Info (3 tools)
        ("macos-use_list_running_apps", {}),
        ("macos-use_list_browser_tabs", {"browser": "Safari"}),
        ("macos-use_list_all_windows", {}),
        # Clipboard (3 tools)
        ("macos-use_set_clipboard", {"content": "Demo clipboard content", "addToHistory": True}),
        ("macos-use_get_clipboard", {}),
        ("macos-use_clipboard_history", {"action": "list", "limit": 5}),
        # Screenshots & OCR (3 tools)
        ("macos-use_take_screenshot", {"path": "/tmp/demo_screenshot.png", "format": "png"}),
        ("macos-use_perform_ocr", {"imagePath": "/tmp/demo_screenshot.png"}),
        ("macos-use_analyze_ui", {"imagePath": "/tmp/demo_screenshot.png"}),
        # NEW: Advanced Tools (5 tools)
        (
            "macos-use_voice_control",
            {"command": "open safari", "language": "en-US", "confidence": 0.7},
        ),
        ("macos-use_process_management", {"action": "list"}),
        (
            "macos-use_file_encryption",
            {
                "action": "encrypt",
                "path": "/tmp/test.txt",
                "password": "demo123",
                "algorithm": "AES256",
            },
        ),
        (
            "macos-use_system_monitoring",
            {"metric": "cpu", "duration": 3, "alert": False, "threshold": 80.0},
        ),
        ("macos-use_list_tools_dynamic", {}),
        # Shell & Terminal (2 tools)
        ("macos-use_execute_command", {"command": "echo 'Hello from shell'", "timeout": 5}),
        ("macos-use_open_terminal", {"command": "echo 'Terminal demo'", "execute": False}),
        # Spotlight Search (1 tool)
        ("macos-use_spotlight_search", {"query": "demo", "limit": 5}),
    ]

    print(f"📊 Total tools to demonstrate: {len(tools_to_test)}")
    print()

    results = []
    success_count = 0
    error_count = 0

    for i, (tool_name, params) in enumerate(tools_to_test, 1):
        print(f"🔧 Tool {i:2d}: {tool_name}")
        print(f"   📋 Parameters: {params}")

        try:
            result = await manager.call_tool("macos-use", tool_name, params)

            # Extract content from result
            if hasattr(result, "content") and result.content:
                content = result.content[0].text if result.content else "No content"
                # Truncate long content
                if len(content) > 200:
                    content = content[:200] + "..."
            else:
                content = str(result)[:200] + "..." if len(str(result)) > 200 else str(result)

            print(f"   ✅ Result: {content}")
            success_count += 1
            results.append({"tool": tool_name, "status": "success", "result": content})

        except Exception as e:
            error_msg = str(e)[:200] + "..." if len(str(e)) > 200 else str(e)
            print(f"   ❌ Error: {error_msg}")
            error_count += 1
            results.append({"tool": tool_name, "status": "error", "error": error_msg})

        print()

        # Small delay between tools
        await asyncio.sleep(0.1)

    # Final summary
    print("=" * 60)
    print("🎉 Demonstration Complete!")
    print("📊 Summary:")
    print(f"   ✅ Successful: {success_count}")
    print(f"   ❌ Errors: {error_count}")
    print(f"   📈 Total: {len(tools_to_test)}")
    print(f"   🎯 Success Rate: {(success_count / len(tools_to_test) * 100):.1f}%")

    # Save results
    results_data = {
        "timestamp": "2026-02-10T01:45:00Z",
        "total_tools": len(tools_to_test),
        "success_count": success_count,
        "error_count": error_count,
        "success_rate": success_count / len(tools_to_test) * 100,
        "results": results,
    }

    with open("/tmp/macos_tools_demo_results.json", "w") as f:
        json.dump(results_data, f, indent=2)

    print("📄 Results saved to: /tmp/macos_tools_demo_results.json")
    print("=" * 60)

    return results_data


if __name__ == "__main__":
    asyncio.run(demo_all_tools())
