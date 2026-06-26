---
name: apple-development-workflow
description: Strategic guide for iOS and macOS development. Defines the strict boundaries between compilation (xcode-bridge/ios-simulator) and project configuration (xcode-project-setup).
---

# Apple Platform Development Workflow

You are an autonomous agent working on an iOS, macOS, or other Apple platform project. You have multiple specialized tools at your disposal, but you MUST follow these strict boundaries to avoid failing or asking the user for manual help.

## 1. Tool Boundaries

### A. xcode-bridge та ios-simulator (Compilation, Testing, Simulators, Documentation)
The `xcode-bridge` and `ios-simulator` MCP servers are your primary bridge to Apple's developer tools. They expose native Xcode actions as tool calls, completely removing the need to write complex Bash or Ruby scripts for compilation.
- **Core Functionality:** `xcode-bridge` allows you to build projects, run tests, and **access official Apple Developer Documentation**. `ios-simulator` allows you to manage simulators (boot, shutdown, list via `mcp__ios-simulator__*` tools), and install/launch `.app` bundles securely.
- **Use for:** Compiling code, running unit/UI tests, booting simulators, installing `.app` bundles, and querying official Apple documentation (e.g., SwiftUI, MapKit).
- **DO NOT use for:** Modifying project structure. The `xcodebuild` utility CANNOT edit `project.pbxproj` to add dependencies, link libraries, or change target settings.

### B. XcodeGen (Project Configuration & Linking)
You must manage project configuration exclusively via `project.yml` and XcodeGen. 
- **Use for:** Adding Swift Package dependencies, linking Frameworks/Libraries to specific Targets, adding new source files to the Xcode target, and modifying Build Settings.
- **Rule:** If a user or a task requires adding a new package dependency, linking a feature module, or changing project settings, you **MUST** edit the `project.yml` file and then execute `xcodegen generate` to update the `.pbxproj`. Never attempt to modify the `.pbxproj` file directly or use legacy Ruby/Python scripts.

### C. xcode-bridge Documentation Tools (Crucial for Swift & Apple Frameworks)
Because you have access to `xcode-bridge` (which connects to `xcrun mcpbridge`), you have **direct access to the Apple Developer Documentation, Xcode SourceKit, and WWDC transcripts**. You must use these tools automatically in the background whenever you need to work with Swift, SwiftUI, or Apple APIs to ensure you write accurate, up-to-date code.
The available tools are:
- `search_documentation` (or `searchDocumentation`): Performs full-text or semantic search across the Apple Developer Documentation. Use this when you don't know the exact API (e.g., searching for "SwiftUI Map custom markers iOS 17").
- `get_documentation_detail` (or `getDocumentation`): Retrieves the full text, parameter descriptions, data types, and **Code Samples** for a specific class or method. Use this when you have an ID or symbol (e.g., `SwiftUI/View/onAppear(perform:)`) and need to know exactly how to use it.
- `get_symbol_info` (or `lookupSymbol`): Uses Xcode's local index (SourceKit) to pull Quick Help for any system type. Use this to instantly check what errors a method throws or what data type it returns (e.g., checking `URLSession.shared.data(from:)`).
- `search_wwdc_transcripts`: Searches text transcripts of WWDC video lectures. Use this if official documentation is sparse and you need real-world architectural advice or best practices straight from Apple engineers.

## 2. Strict Rules

1. **Project Generation via XcodeGen (CRITICAL):** You are STRICTLY FORBIDDEN from generating custom Bash (`.sh`), Ruby (`.rb`), or Python (`.py`) scripts to manipulate or create Xcode projects (`.pbxproj`). If a project does not exist or is corrupted, you MUST create a standard `project.yml` file and run `xcodegen generate`. XcodeGen is the ONLY acceptable way to programmatically generate an Xcode project. For all other project manipulation, building, testing, or running, rely EXCLUSIVELY on the two available MCP servers (`xcode-bridge` and `ios-simulator`).
2. **Never Ask the User for Manual IDE Steps:** You are fully equipped to configure Xcode projects yourself. Do not output step-by-step UI instructions asking the user to click "+" in Xcode. Instead, modify the `project.yml` file and run XcodeGen.
3. **Apple Documentation Check (CRITICAL):** You have access to official Apple technical documentation via the `xcode-bridge` MCP tools (`search_documentation`, `get_documentation_detail`, etc.). **ALWAYS** use these tools to look up documentation for APIs (like MapKit, SwiftUI updates, or framework availability) BEFORE writing code. This is an essential step to prevent hallucinations, avoid using deprecated APIs, and ensure code is compatible with the target SDK. 
4. **SDK Version Check (CRITICAL):** ALWAYS check the installed SDK versions (`xcodebuild -showsdks` or similar) before starting. Do NOT hardcode `@available(iOS 17.0, *)` — instead, use **iOS 26** as the baseline for now. However, you MUST dynamically verify the available SDKs and adjust your Swift compiler targets and availability checks based on the actual installed environment.
5. **Mandatory Simulator Testing:** After creating or modifying an iOS project, you MUST ALWAYS test it via the iOS Simulator using the `ios-simulator` MCP. Boot the simulator, build the project, install the `.app` bundle, and launch it to verify the app doesn't crash on startup.
6. **Simulator Selection (CRITICAL):** Before launching tests, you MUST check the available simulators (using simulator tools or `xcrun simctl list`). You must explicitly identify the correct phone model and iOS version. Unless otherwise specified, your primary testing target is **iPhone 16 Pro Max running iOS 26.6** (or the closest available iOS 26 version).
7. **Swift Package Manager (SPM):** Apple platforms rely heavily on SPM. If you create a new Swift Package locally, remember that it is useless to the main app until it is explicitly linked to the app's target. Link it by updating the dependencies block in `project.yml` and running `xcodegen generate`.
8. **Recursive Planning & Task Tracking (CRITICAL):** You MUST strictly create and follow a plan (`task.md`). Break down complex features into smaller sub-tasks, recursively going as deep as necessary. You must strictly follow this plan, marking items as completed (`[x]`), in progress (`[/]`), and dynamically adding new sub-tasks as you discover new requirements during development. Never write code without first breaking the task down in your plan.
9. **Tool Discovery and Usage (CRITICAL):** You MUST use the `ToolSearch` tool to discover available tools and MCP server capabilities. Before using any newly discovered tool (especially from `xcode-bridge` or `ios-simulator`), you are required to retrieve its schema, carefully read its internal instructions, and analyze any provided examples. Never guess or hallucinate tool arguments.
10. **Path Verification and Directory Trees (CRITICAL):** Before executing any tool or command that requires a file path, you MUST verify your current location and the target path. Use the available directory listing tools (e.g., `list_dir` or a terminal `tree` command) to fetch the file structure into your context. Never hallucinate absolute or relative paths without confirming the directory tree first.

## 3. Standard Workflow (Adding a Feature Package)
1. Agent queries `xcode-bridge` for official Apple documentation on the required APIs (e.g., checking latest SwiftUI or MapKit syntax).
2. Agent generates the Swift code for the new local SPM package based on the official docs and the current SDK version.
3. Agent edits `project.yml` to declare the new local SPM package and link it to the main App Target.
4. Agent executes `xcodegen generate` to safely update the `.xcodeproj`.
5. Agent calls `xcode-bridge` tools to verify the project compiles successfully.
6. **Thorough UI & Functional Testing (Simulator):** Agent runs the `.app` on the iOS Simulator using `ios-simulator` tools. Testing MUST follow a two-step process:
   - *Phase 1 (Visual Audit):* Navigate through ALL pages/screens and capture screenshots via the MCP. Visually verify the presence, layout, and contrast of all buttons, text, and interface elements.
   - *Phase 2 (Interactive Test):* Only AFTER the visual audit is confirmed, the agent must test every single active element (buttons, toggles, navigation links) interactively (via taps/swipes) to ensure they execute the correct logic without crashing.
7. **Continuous Creative Improvement (Premium UI/UX):** After successful final testing in the iOS simulator, the agent MUST independently develop a plan for functional, creative, and interesting enhancements. You are expected to use the **most cutting-edge, experimental frameworks and solutions** (e.g., latest SwiftUI features, Metal shaders, advanced spatial UI elements, etc.) to add incredible visual depth, a modern and elegant look, and powerful functionality. This includes proposing significant improvements to the UI/UX design, fluid micro-animations, advanced visual effects (e.g., Glassmorphism, 3D particles, dynamic shadows), and deep sound/haptic integrations to create a premium, state-of-the-art user experience.
