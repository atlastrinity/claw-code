---
trigger: always_on
glob:
description:
---

# iOS & Xcode MCP Guidelines

When developing, building, or debugging iOS/macOS applications, you MUST rely on the configured MCP servers rather than executing raw terminal commands (`xcodebuild`, `xcrun`, `simctl`, etc.). Raw terminal commands for Xcode are highly complex and prone to hallucinations.

## Available MCP Servers

1. **`xcode-bridge`**
   - **Capabilities:**
     - Session defaults (configuring project, scheme, simulator, and device)
     - Project discovery
     - Simulator/Device workflows (Build, run, test, install, launch)
     - macOS workflows
     - LLDB debugging & UI Automation
     - SwiftPM management
   - **Usage Rules:**
     - Always call the relevant defaults/session tool before performing the first build/run/test action in a session.
     - Only use project discovery if defaults show missing or incorrect project/workspace context. Do not run discovery speculatively.
     - For running on a simulator, prefer the combined "build-and-run" tool instead of separate build then run calls.
     - If tools are missing, remind the user to check `.xcodebuildmcp/config.yaml` to enable the workflow and reload the session.

2. **`ios-simulator`**
   - **Capabilities:**
     - Tools for interacting with the iOS simulator.
   - **Usage Rules:**
     - Use this server to inspect simulator states, take screenshots, or manage simulator environments when testing iOS apps.

## General Best Practices

- NEVER guess MCP tool names or command arguments. Always refer to the schema provided by the MCP server.
- NEVER attempt to run `xcodebuild -create-xcodeproj` (this command was removed by Apple).
- Clearly report the active defaults context (project/workspace, scheme, simulator/device) back to the user when using bridge tools.
- For failures, state exactly which step failed and what tool will be used next to resolve it.
