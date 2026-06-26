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

### B. xcode-project-setup (Project Configuration & Linking)
You have access to a skill named `xcode-project-setup`. This skill contains logic (via Python/Ruby scripts) to safely modify Xcode projects (`.pbxproj`).
- **Use for:** Adding Swift Package dependencies, linking Frameworks/Libraries to specific Targets, adding new source files to the Xcode target, and modifying Build Settings.
- **Rule:** If a user or a task requires adding a new package dependency or linking a feature module, you **MUST** call the `xcode-project-setup` skill. 

### C. xcode-bridge Documentation Tools (Crucial for Swift & Apple Frameworks)
Because you have access to `xcode-bridge` (which connects to `xcrun mcpbridge`), you have **direct access to the Apple Developer Documentation, Xcode SourceKit, and WWDC transcripts**. You must use these tools automatically in the background whenever you need to work with Swift, SwiftUI, or Apple APIs to ensure you write accurate, up-to-date code.
The available tools are:
- `search_documentation` (or `searchDocumentation`): Performs full-text or semantic search across the Apple Developer Documentation. Use this when you don't know the exact API (e.g., searching for "SwiftUI Map custom markers iOS 17").
- `get_documentation_detail` (or `getDocumentation`): Retrieves the full text, parameter descriptions, data types, and **Code Samples** for a specific class or method. Use this when you have an ID or symbol (e.g., `SwiftUI/View/onAppear(perform:)`) and need to know exactly how to use it.
- `get_symbol_info` (or `lookupSymbol`): Uses Xcode's local index (SourceKit) to pull Quick Help for any system type. Use this to instantly check what errors a method throws or what data type it returns (e.g., checking `URLSession.shared.data(from:)`).
- `search_wwdc_transcripts`: Searches text transcripts of WWDC video lectures. Use this if official documentation is sparse and you need real-world architectural advice or best practices straight from Apple engineers.

## 2. Strict Rules

1. **NO Ad-hoc Script Generation (CRITICAL):** You are STRICTLY FORBIDDEN from generating custom Bash (`.sh`), Ruby (`.rb`), or Python (`.py`) scripts to manipulate Xcode projects, build the app, or simulate features. Do not attempt to manually generate or parse `project.pbxproj` using scripts. You MUST write all your logic in **Swift** and rely EXCLUSIVELY on the two available MCP servers (`xcode-bridge` and `ios-simulator`) for any project manipulation, building, testing, or running.
2. **Never Ask the User for Manual IDE Steps:** You are fully equipped to configure Xcode projects yourself. Do not output step-by-step UI instructions asking the user to click "+" in Xcode. Instead, use the `xcode-project-setup` skill.
3. **Apple Documentation Check (CRITICAL):** You have access to official Apple technical documentation via the `xcode-bridge` MCP tools (`search_documentation`, `get_documentation_detail`, etc.). **ALWAYS** use these tools to look up documentation for APIs (like MapKit, SwiftUI updates, or framework availability) BEFORE writing code. This is an essential step to prevent hallucinations, avoid using deprecated APIs, and ensure code is compatible with the target SDK. 
4. **SDK Version Check (CRITICAL):** ALWAYS check the installed SDK versions (`xcodebuild -showsdks` or similar) before starting. Do NOT hardcode `@available(iOS 17.0, *)` if the user's environment has a different version (like iOS SDK 18 or 26). Adjust your Swift compiler targets and availability checks dynamically based on the actual installed SDK.
5. **Mandatory Simulator Testing:** After creating or modifying an iOS project, you MUST ALWAYS test it via the iOS Simulator using the `ios-simulator` MCP. Boot the simulator, build the project, install the `.app` bundle, and launch it to verify the app doesn't crash on startup.
6. **Default Simulator:** Unless otherwise specified by the user or the configuration, assume the target simulator is `iPhone 16 Pro Max`. 
7. **Swift Package Manager (SPM):** Apple platforms rely heavily on SPM. If you create a new Swift Package locally, remember that it is useless to the main app until it is explicitly linked to the app's target. Link it using `xcode-project-setup`.
8. **Resolving Skills:** When loading the `xcode-project-setup` skill via the `Skill` tool, use its exact name or valid absolute path. Do not hallucinate paths.

## 3. Standard Workflow (Adding a Feature Package)
1. Agent queries `xcode-bridge` for official Apple documentation on the required APIs (e.g., checking latest SwiftUI or MapKit syntax).
2. Agent generates the Swift code for the new local SPM package based on the official docs and the current SDK version.
3. Agent calls the `Skill` tool with `xcode-project-setup` to understand how to link it.
4. Agent executes the required scripts from `xcode-project-setup` to link the new package to the main App Target.
5. Agent calls `xcode-bridge` tools to verify the project compiles successfully.
6. **Thorough UI & Functional Testing (Simulator):** Agent runs the `.app` on the iOS Simulator using `ios-simulator` tools. Testing MUST follow a two-step process:
   - *Phase 1 (Visual Audit):* Navigate through ALL pages/screens and capture screenshots via the MCP. Visually verify the presence, layout, and contrast of all buttons, text, and interface elements.
   - *Phase 2 (Interactive Test):* Only AFTER the visual audit is confirmed, the agent must test every single active element (buttons, toggles, navigation links) interactively (via taps/swipes) to ensure they execute the correct logic without crashing.
7. **Continuous Creative Improvement (Premium UI/UX):** After successful final testing in the iOS simulator, the agent MUST independently develop a plan for functional, creative, and interesting enhancements. You are expected to use the **most cutting-edge, experimental frameworks and solutions** (e.g., latest SwiftUI features, Metal shaders, advanced spatial UI elements, etc.) to add incredible visual depth, a modern and elegant look, and powerful functionality. This includes proposing significant improvements to the UI/UX design, fluid micro-animations, advanced visual effects (e.g., Glassmorphism, 3D particles, dynamic shadows), and deep sound/haptic integrations to create a premium, state-of-the-art user experience.
