# Claw Code System Architecture Diagrams

**Date:** 2026-06-26
**Purpose:** Visual representation of system architecture and workflows

---

## 🏗️ High-Level Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER INTERFACE                                   │
│                                                                              │
│  Terminal / IDE / Script Call                                               │
└────────────────────────────┬────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         LAUNCHER SCRIPTS                                      │
│                                                                              │
│  ┌──────────────────────┐         ┌──────────────────────────┐              │
│  │  run_claw.sh         │         │ run_claw_new_session.sh  │              │
│  │                      │         │                          │              │
│  │  - Load .env         │         │  - Load .env             │              │
│  │  - Cleanup zombies   │         │  - Cleanup zombies       │              │
│  │  - Model selection   │         │  - Model selection       │              │
│  │  - Start Xcode       │         │  - Start Xcode           │              │
│  │  - Start RAG service │         │  - Start RAG service     │              │
│  │  - Auto-restart loop │         │  - Auto-restart loop     │              │
│  └──────────┬───────────┘         └──────────┬───────────────┘              │
└─────────────┼───────────────────────────────┼───────────────────────────────┘
              │                               │
              ▼                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CLAW CLI AGENT (RUST)                                  │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                    rusty-claude-cli                                  │    │
│  │                                                                      │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌──────────────────────────┐    │    │
│  │  │   REPL UI   │  │  Session    │  │   Tool Orchestration     │    │    │
│  │  │  Terminal   │  │  Manager    │  │   Engine                 │    │    │
│  │  └─────────────┘  └─────────────┘  └──────────────────────────┘    │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                       SKILL SYSTEM (MCP)                                     │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                    SKILL ATTACHMENT                                   │    │
│  │                                                                      │    │
│  │  ┌──────────────────────────┐    ┌──────────────────────────┐       │    │
│  │  │ apple-development-       │    │ xcode_project_setup      │       │    │
│  │  │ workflow                 │    │                          │       │    │
│  │  │                          │    │  - XcodeGenKit            │       │    │
│  │  │  - Recursive Planning    │    │  - MCP Integration       │       │    │
│  │  │  - Tool Boundaries       │    │  - Firebase MCP           │       │    │
│  │  │  - Documentation First   │    │  - Project Configuration │       │    │
│  │  └──────────────────────────┘    └──────────────────────────┘       │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        MCP SERVER LAYER                                      │
│                                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                     │
│  │ xcode-bridge │  │ ios-simulator │  │ Firebase MCP  │                     │
│  │ MCP Server   │  │ MCP Server   │  │ MCP Server   │                     │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                     │
│         │                 │                 │                             │
│         ▼                 ▼                 ▼                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                     │
│  │ xcrun mcpbridge│ │ simctl MCP  │  │ firebase-tools│                     │
│  │              │  │             │  │ MCP Bridge   │                     │
│  └──────────────┘  └──────────────┘  └──────────────┘                     │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         RAG SERVICE (HTTP)                            │  │
│  │                                                                        │  │
│  │  claw-rag-service                                                     │  │
│  │                                                                        │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌────────────────────────────┐    │  │
│  │  │ Embeddings  │  │ Semantic    │  │  HTTP API                  │    │  │
│  │  │ Generator   │  │ Search      │  │  - POST /embed             │    │  │
│  │  └─────────────┘  └─────────────┘  │  - POST /search            │    │  │
│  │                                     │  - GET /status             │    │  │
│  └────────────────────────────────────┴────────────────────────────┘    │  │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 🔄 Main Execution Workflow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LAUNCH SEQUENCE                                    │
└─────────────────────────────────────────────────────────────────────────────┘

Step 1: Environment Setup
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  1.1 Load .env file                                                        │
│      └─> Read API keys, config paths, environment variables                │
│                                                                              │
│  1.2 Change to script directory                                            │
│      └─> SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"                        │
│                                                                              │
│  1.3 Check for zombie processes                                            │
│      └─> pkill -f "claw-rag-service"                                        │
│      └─> pkill -f "mcpbridge"                                              │
│      └─> pkill -f "ios-simulator-mcp"                                      │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘

Step 2: Model Selection
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  2.1 Read .claw.json settings                                               │
│      └─> Parse model aliases                                                │
│                                                                              │
│  2.2 Display available models                                              │
│      └─> Show numbered list with model names and provider URLs            │
│                                                                              │
│  2.3 User selection                                                        │
│      └─> Read choice from stdin                                            │
│      └─> Default to "gemini-lite"                                          │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘

Step 3: Xcode Launch
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  3.1 Check if Xcode is running                                             │
│      └─> pgrep -q -x "Xcode"                                                │
│                                                                              │
│  3.2 If not running, start Xcode                                           │
│      └─> open -a Xcode                                                      │
│      └─> Wait 3 seconds for startup                                        │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘

Step 4: RAG Service Start
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  4.1 Start RAG service in background                                        │
│      └─> claw-rag-service serve >> ~/.claw/logs/claw-rag-startup.err       │
│      └─> Capture PID ($RAG_PID)                                            │
│                                                                              │
│  4.2 Verify service started                                                │
│      └─> sleep 1                                                            │
│      └─> kill -0 $RAG_PID                                                  │
│      └─> If failed, exit with error                                        │
│                                                                              │
│  4.3 Set trap for cleanup on exit                                          │
│      └─> trap "kill $RAG_PID" EXIT                                         │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘

Step 5: Main Loop
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  5.1 Launch Claw CLI                                                       │
│      └─> claw --model $SELECTED_MODEL \                                    │
│          --skip-permissions \                                              │
│          --accept-danger-non-interactive \                                 │
│          --attach-skill "apple-development-workflow/SKILL.md" \            │
│          --attach-skill "xcode_project_setup/SKILL.md" \                   │
│          $RESUME_ARGS "$@"                                                  │
│                                                                              │
│  5.2 Capture exit code                                                    │
│      └─> EXIT_CODE=$?                                                      │
│                                                                              │
│  5.3 Handle exit conditions                                               │
│      ├─> Exit 0 → Success, break loop                                      │
│      ├─> Exit 130/143/137 → Manual stop, break loop                        │
│      └─> Other → Auto-restart in 3 seconds                                  │
│                                                                              │
│  5.4 Restart with resume                                                  │
│      └─> RESUME_ARGS="--resume latest"                                     │
│      └─> sleep 3                                                           │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 🎯 Xcode Project Setup Workflow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    XCODE PROJECT SETUP WORKFLOW                             │
└─────────────────────────────────────────────────────────────────────────────┘

1. User Request
┌─────────────────────────────────────────────────────────────────────────────┐
│  User: "Add Firebase Authentication to my app"                              │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
2. Agent Attaches Skills
┌─────────────────────────────────────────────────────────────────────────────┐
│  Agent loads:                                                               │
│  - apple-development-workflow (iOS development guidelines)                 │
│  - xcode_project_setup (project configuration)                             │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
3. Recursive Planning
┌─────────────────────────────────────────────────────────────────────────────┐
│  Agent creates task.md:                                                     │
│                                                                              │
│  1. Query Apple Documentation                                               │
│     - Use xcode-bridge MCP to search Firebase Auth docs                    │
│                                                                              │
│  2. Generate Swift Code                                                     │
│     - Create Firebase setup code based on official docs                    │
│                                                                              │
│  3. Update project.yml                                                      │
│     - Add Firebase package dependency                                      │
│     - Link Firebase Auth product                                           │
│     - Add -ObjC linker flag                                                 │
│                                                                              │
│  4. Generate Xcode Project                                                  │
│     - Run xcodegen generate                                                 │
│                                                                              │
│  5. Verify Build                                                            │
│     - Use xcode-bridge MCP to build project                                │
│                                                                              │
│  6. Test in Simulator                                                       │
│     - Boot iPhone 16 Pro Max                                                │
│     - Build and install app                                                 │
│     - Test Firebase Auth flow                                               │
│                                                                              │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
4. MCP Integration (xcode_spm_setup_mcp)
┌─────────────────────────────────────────────────────────────────────────────┐
│  Swift Script: xcode_spm_setup_mcp.swift                                   │
│                                                                              │
│  Step 1: Firebase MCP Client Setup                                          │
│      └─> Create MCPServerConfig                                            │
│      └─> Command: "npx -y firebase-tools@latest mcp"                       │
│                                                                              │
│  Step 2: Login to Firebase                                                  │
│      └─> Execute MCP command: firebase:login                                │
│                                                                              │
│  Step 3: Deploy Configuration                                              │
│      └─> Execute MCP command: firebase:deploy                              │
│                                                                              │
│  Step 4: Get Project ID                                                    │
│      └─> Simulate: "my-app-9249"                                           │
│                                                                              │
│  Step 5: Fetch Configuration                                               │
│      └─> Return simulated config structure                                │
│                                                                              │
│  Step 6: Update project.yml                                                │
│      └─> Add Firebase package to packages section                          │
│      └─> Add FirebaseAuth to targets.dependencies                           │
│                                                                              │
│  Step 7: Save project.yml                                                  │
│      └─> Write YAML file                                                   │
│                                                                              │
│  Step 8: Generate Xcode Project                                            │
│      └─> Execute: xcodegen generate --project project.yml                  │
│                                                                              │
│  Step 9: Create GoogleService-Info.plist                                   │
│      └─> Generate plist with simulated Firebase config                    │
│                                                                              │
│  Step 10: Provide Next Steps                                               │
│      └─> Show user what to do next                                         │
│                                                                              │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
5. XcodeGen Integration
┌─────────────────────────────────────────────────────────────────────────────┐
│  project.yml structure:                                                     │
│                                                                              │
│  name: MyApp                                                                │
│  options:                                                                   │
│    bundleIdPrefix: com.example                                              │
│    deploymentTarget:                                                        │
│      iOS: "16.0"                                                            │
│  targets:                                                                   │
│    MyApp:                                                                   │
│      type: application                                                      │
│      platform: iOS                                                          │
│      sources: [Sources]                                                     │
│      dependencies: []                                                       │
│  packages:                                                                  │
│    - name: Firebase                                                         │
│      url: https://github.com/firebase/firebase-ios-sdk                     │
│      from: "11.0.0"                                                         │
│                                                                              │
│  └─> Execute: xcodegen generate --project project.yml                     │
│      └─> Generates: project.xcodeproj/.pbxproj                             │
│                                                                              │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
6. Build Verification
┌─────────────────────────────────────────────────────────────────────────────┐
│  Use xcode-bridge MCP:                                                      │
│      └─> xcodebuild -project project.xcodeproj -scheme MyApp build         │
│      └─> Check for compilation errors                                       │
│                                                                              │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
7. Simulator Testing
┌─────────────────────────────────────────────────────────────────────────────┐
│  Use ios-simulator MCP:                                                     │
│      └─> mcp__ios-simulator__get_booted_sim_id                              │
│      └─> mcp__ios-simulator__launch_app                                     │
│      └─> mcp__ios-simulator__ui_tap (test interactive elements)            │
│      └─> mcp__ios-simulator__screenshot (visual verification)             │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 🌐 MCP Server Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MCP CLIENT (Swift Script)                                │
│                                                                              │
│  xcode_spm_setup_mcp.swift                                                 │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  FirebaseMCPClient Class                                            │    │
│  │                                                                      │    │
│  │  - execute(_ command: MCPCommand, arguments: [String])              │    │
│  │  - login()                                                          │    │
│  │  - deploy(_ projectId: String, _ configPath: String)                │    │
│  │  - getConfig(projectId: String) -> [String: Any]                    │    │
│  │  - startEmulators()                                                 │    │
│  │  - getAnalytics(...)                                                │    │
│  │  - sendCrashlyticsReport(...)                                        │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  ProjectConfigManager Class                                          │    │
│  │                                                                      │    │
│  │  - getProjectName() -> String                                        │    │
│  │  - getBundleId() -> String                                           │    │
│  │  - addPackage(name, url, version)                                    │    │
│  │  - addTargetDependency(packageName, productName)                     │    │
│  │  - save() -> Void                                                     │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    MCP SERVER (NPM Package)                                 │
│                                                                              │
│  firebase-tools MCP Server (npx -y firebase-tools@latest mcp)               │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  MCP Protocol (stdio)                                               │    │
│  │                                                                      │    │
│  │  Request:                                                           │    │
│  │    {                                                                 │    │
│  │      "jsonrpc": "2.0",                                              │    │
│  │      "id": 1,                                                        │    │
│  │      "method": "tools/list",                                        │    │
│  │      "params": {}                                                   │    │
│  │    }                                                                 │    │
│  │                                                                      │    │
│  │  Response:                                                           │    │
│  │    {                                                                 │    │
│  │      "jsonrpc": "2.0",                                              │    │
│  │      "id": 1,                                                        │    │
│  │      "result": {                                                     │    │
│  │        "tools": [                                                   │    │
│  │          {                                                           │    │
│  │            "name": "firebase:login",                                │    │
│  │            "description": "Login to Firebase",                      │    │
│  │            "inputSchema": { ... }                                    │    │
│  │          },                                                          │    │
│  │          {                                                           │    │
│  │            "name": "firebase:deploy",                               │    │
│  │            "description": "Deploy Firebase configuration",          │    │
│  │            "inputSchema": { ... }                                    │    │
│  │          },                                                          │    │
│  │          ...                                                         │    │
│  │        ]                                                             │    │
│  │      }                                                               │    │
│  │    }                                                                 │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
└───────────────────────────────┬─────────────────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                    FIREBASE TOOLS (Underlying SDK)                          │
│                                                                              │
│  firebase-tools npm package                                                 │
│                                                                              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  Firebase CLI                                                       │    │
│  │                                                                      │    │
│  │  - firebase login                                                   │    │
│  │  - firebase deploy                                                  │    │
│  │  - firebase functions:deploy                                        │    │
│  │  - firebase firestore:start (emulators)                             │    │
│  │  - firebase analytics:get                                           │    │
│  │  - firebase crashlytics:send                                        │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 📊 Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           DATA FLOW OVERVIEW                                 │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                           USER INPUT FLOW                                   │
│                                                                              │
│  Terminal / Script                                                          │
│       │                                                                     │
│       ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ run_claw.sh  │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  Claw CLI    │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  Skills      │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  MCP Tools   │                                                          │
│  └──────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                           DOCUMENTATION FLOW                                │
│                                                                              │
│  Agent needs Swift API info                                                 │
│       │                                                                     │
│       ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ xcode-bridge │                                                          │
│  │   MCP        │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  xcrun       │                                                          │
│  │  mcpbridge   │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  SourceKit   │                                                          │
│  │  (Xcode)     │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ Apple Docs   │                                                          │
│  │  + WWDC      │                                                          │
│  └──────────────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  Swift Code Generated                                                     │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                           PROJECT CONFIGURATION                              │
│                                                                              │
│  Agent needs to add Firebase                                                │
│       │                                                                     │
│       ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ xcode_spm_   │                                                          │
│  │ setup_mcp    │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ Firebase MCP │                                                          │
│  │   Client     │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ Firebase MCP │                                                          │
│  │   Server     │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ firebase-    │                                                          │
│  │ tools CLI    │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ project.yml  │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ XcodeGen     │                                                          │
│  │ generate     │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ .pbxproj     │                                                          │
│  │ (Xcode)      │                                                          │
│  └──────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                           TESTING FLOW                                       │
│                                                                              │
│  Agent needs to test app                                                    │
│       │                                                                     │
│       ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ ios-simulator │                                                          │
│  │   MCP        │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ simctl MCP   │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ Xcode Sim    │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ iPhone 16    │                                                          │
│  │ Pro Max      │                                                          │
│  └──────────────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │ .app Bundle  │                                                          │
│  └──────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 📈 Performance Metrics Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                       PERFORMANCE METRICS                                    │
└─────────────────────────────────────────────────────────────────────────────┘

Startup Sequence:
┌─────────────────────────────────────────────────────────────────────────────┐
│  1. Environment Load: < 100ms                                               │
│  2. Zombie Cleanup: < 500ms                                                │
│  3. Model Selection: ~1s                                                   │
│  4. Xcode Launch: ~3s                                                      │
│  5. RAG Service Start: ~2s                                                 │
│  ────────────────────────────────────────────────────────────────────────  │
│  Total Startup Time: ~7s                                                   │
│                                                                              │
│  Target: < 10s                                                             │
└─────────────────────────────────────────────────────────────────────────────┘

Model Selection:
┌─────────────────────────────────────────────────────────────────────────────┐
│  - Read .claw.json: < 50ms                                                 │
│  - Parse JSON: < 10ms                                                      │
│  - Display list: < 100ms                                                   │
│  ────────────────────────────────────────────────────────────────────────  │
│  Total: ~1s (mostly I/O)                                                   │
│                                                                              │
│  Optimization: Cache model list to reduce to < 500ms                       │
└─────────────────────────────────────────────────────────────────────────────┘

RAG Service:
┌─────────────────────────────────────────────────────────────────────────────┐
│  Current: ~2s                                                              │
│  Target: < 1s                                                              │
│  Optimization: Pre-warm embeddings cache                                   │
└─────────────────────────────────────────────────────────────────────────────┘

Xcode Launch:
┌─────────────────────────────────────────────────────────────────────────────┐
│  Current: ~3s                                                              │
│  Target: < 2s                                                              │
│  Optimization: Keep Xcode running in background                            │
└─────────────────────────────────────────────────────────────────────────────┘

Auto-Restart:
┌─────────────────────────────────────────────────────────────────────────────┐
│  Current: 3s wait + startup time                                           │
│  Target: < 5s total                                                        │
│  Optimization: Faster startup for resumed sessions                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 🔒 Security Flow

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        SECURITY FLOW                                         │
└─────────────────────────────────────────────────────────────────────────────┘

1. Environment Variables
┌─────────────────────────────────────────────────────────────────────────────┐
│  .env file                                                                  │
│       │                                                                     │
│       ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  Load .env   │                                                          │
│  │  set -a      │                                                          │
│  │  source .env │                                                          │
│  │  set +a      │                                                          │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  Process     │                                                          │
│  │  Variables   │                                                          │
│  └──────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘

2. Process Isolation
┌─────────────────────────────────────────────────────────────────────────────┐
│  Main Script                                                                │
│       │                                                                     │
│       ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  RAG Service │  (Background Process)                                    │
│  │  (PID: $RAG_PID)                                                     │
│  └──────┬───────┘                                                          │
│         │                                                                     │
│         ▼                                                                     │
│  ┌──────────────┐                                                          │
│  │  Claw CLI    │  (Main Process)                                         │
│  └──────────────┘                                                          │
│                                                                              │
│  Cleanup on exit: trap "kill $RAG_PID" EXIT                               │
└─────────────────────────────────────────────────────────────────────────────┘

3. File Permissions
┌─────────────────────────────────────────────────────────────────────────────┐
│  Scripts: chmod 700                                                         │
│  .env file: chmod 600                                                       │
│  No hardcoded secrets in scripts                                           │
└─────────────────────────────────────────────────────────────────────────────┘

4. MCP Server Security
┌─────────────────────────────────────────────────────────────────────────────┐
│  Firebase MCP: Uses npm package, authenticated via Firebase CLI            │
│  iOS Simulator MCP: Uses simctl, sandboxed                                 │
│  xcode-bridge MCP: Uses xcrun mcpbridge, system integration                │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 📝 Key Design Decisions

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                   DESIGN DECISIONS & RATIONALE                               │
└─────────────────────────────────────────────────────────────────────────────┘

1. Rust Core (rusty-claude-cli)
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: High performance, memory safety, cross-platform compatibility   │
   │ BENEFIT: Fast startup, low memory footprint, reliable execution         │
   │ ALTERNATIVES CONSIDERED: Go, Node.js (rejected for performance)        │
   └─────────────────────────────────────────────────────────────────────────┘

2. Separate RAG Service
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: Decouple indexing/search from CLI for performance               │
   │ BENEFIT: Independent scaling, better resource utilization, faster CLI   │
   │ ALTERNATIVES CONSIDERED: In-process (rejected for memory usage)        │
   └─────────────────────────────────────────────────────────────────────────┘

3. MCP Protocol Integration
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: Standardized interface for tool discovery and execution         │
   │ BENEFIT: Easy integration with new tools, consistent API               │
   │ ALTERNATIVES CONSIDERED: REST API, GraphQL (rejected for complexity)   │
   └─────────────────────────────────────────────────────────────────────────┘

4. XcodeGen for Project Management
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: Programmatic project generation, YAML-based configuration     │
   │ BENEFIT: Version control friendly, easy automation, clean structure    │
   │ ALTERNATIVES CONSIDERED: Ruby xcodeproj gem (rejected for maintainability)│
   └─────────────────────────────────────────────────────────────────────────┘

5. Recursive Planning Enforcement
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: Ensure comprehensive task breakdown before execution           │
   │ BENEFIT: Fewer errors, better code quality, systematic development     │
   │ ALTERNATIVES CONSIDERED: Optional planning (rejected for consistency)   │
   └─────────────────────────────────────────────────────────────────────────┘

6. MCP for Firebase Integration
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: Leverage existing Firebase MCP server, no custom implementation │
   │ BENEFIT: Faster development, better testing, official Firebase support  │
   │ ALTERNATIVES CONSIDERED: Custom Swift Firebase client (rejected for time)│
   └─────────────────────────────────────────────────────────────────────────┘

7. iOS 26 Design Guidelines
   ┌─────────────────────────────────────────────────────────────────────────┐
   │ REASON: Target latest Apple platforms, modern UI/UX patterns           │
   │ BENEFIT: Future-proof code, premium user experience, design awards     │
   │ ALTERNATIVES CONSIDERED: Legacy iOS versions (rejected for obsolescence) │
   └─────────────────────────────────────────────────────────────────────────┘
```

---

## 🎯 Architecture Goals Achieved

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     GOALS & SUCCESS CRITERIA                                │
└─────────────────────────────────────────────────────────────────────────────┘

✅ High Performance
   - Rust core for speed
   - Separate RAG service for efficiency
   - Optimized startup time (< 10s)

✅ Modular Design
   - Separate CLI and RAG service
   - Skill-based extensibility
   - MCP protocol for tool integration

✅ Developer Experience
   - Clear error messages
   - Visual status indicators
   - Comprehensive documentation

✅ Production Readiness
   - Error handling
   - Health checks
   - Auto-recovery

✅ Security
   - Environment variable management
   - Process isolation
   - No hardcoded secrets

✅ Maintainability
   - Clear architecture
   - Well-documented workflows
   - Testable components
```

---

**Diagrams Generated:** 2026-06-26
**Version:** 1.0
**Status:** ✅ Complete
