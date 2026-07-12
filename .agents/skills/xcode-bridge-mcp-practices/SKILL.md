---
name: xcode-bridge-mcp-practices
description: "Mandatory instructions for interacting with the XcodeBuildMCP (xcode-bridge) server. Use this skill when building macOS/iOS applications to avoid hallucinating raw terminal commands."
---

# XcodeBuildMCP / Xcode-Bridge Best Practices

When building applications for macOS or iOS, you **MUST NOT** use raw terminal commands like `xcodebuild`, `xcrun`, or `simctl` if XcodeBuildMCP is available. Raw terminal commands for Xcode are highly complex and prone to hallucinations.

Instead, always use the tools provided by the XcodeBuildMCP server.

## 1. Capabilities
XcodeBuildMCP provides tools for:
- Session defaults (configuring project, scheme, simulator, and device)
- Project discovery
- Simulator/Device workflows (Build, run, test, install, launch)
- macOS workflows
- LLDB debugging & UI Automation
- SwiftPM management

**IMPORTANT**: If a capability (like macOS workflows or debugging) is not available in the tool list, check `.xcodebuildmcp/config.yaml` to enable the workflow, and ask the user to reload the session.

## 2. Step-by-step Execution

### Step 1: Establish Context
- Always call `session_show_defaults` **before** performing the first build/run/test action in a session.
- Only use `discover_projs` if the defaults show missing or incorrect project/workspace context. Do not run discovery speculatively.
- For running on a simulator, prefer the combined "build-and-run" tool instead of separate build then run calls.

### Step 2: Avoid Hallucinations
- NEVER guess MCP tool names or command arguments. Read the schemas provided by the server.
- NEVER attempt to run `xcodebuild -create-xcodeproj` (this command was removed by Apple). Use XcodeBuildMCP's scaffolding tools or `xcodebuild -scheme` directly if absolutely necessary, but prefer the MCP tools.

## 3. Reporting Context
When using the bridge tools, clearly report the active defaults context (project/workspace, scheme, simulator/device) back to the user. For failures, state exactly which step failed and what tool will be used next to resolve it.
