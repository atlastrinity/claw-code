---
name: apple-development-workflow
description: Strategic guide for iOS and macOS development. Defines the strict boundaries between compilation (XcodeBuildMCP) and project configuration (xcode-project-setup).
---

# Apple Platform Development Workflow

You are an autonomous agent working on an iOS, macOS, or other Apple platform project. You have multiple specialized tools at your disposal, but you MUST follow these strict boundaries to avoid failing or asking the user for manual help.

## 1. Tool Boundaries

### A. XcodeBuildMCP (Compilation, Testing, Simulators)
The `XcodeBuildMCP` server provides tools like `xcode_build`, `xcode_test`, and `simctl_*`. 
- **Use for:** Compiling code, running unit/UI tests, booting simulators, and installing `.app` bundles onto simulators.
- **DO NOT use for:** Modifying project structure. The `xcodebuild` utility CANNOT edit `project.pbxproj` to add dependencies, link libraries, or change target settings.

### B. xcode-project-setup (Project Configuration & Linking)
You have access to a skill named `xcode-project-setup`. This skill contains logic (via Python/Ruby scripts) to safely modify Xcode projects (`.pbxproj`).
- **Use for:** Adding Swift Package dependencies, linking Frameworks/Libraries to specific Targets, adding new source files to the Xcode target, and modifying Build Settings.
- **Rule:** If a user or a task requires adding a new package dependency or linking a feature module, you **MUST** call the `xcode-project-setup` skill. 

## 2. Strict Rules

1. **Never Ask the User for Manual IDE Steps:** You are fully equipped to configure Xcode projects yourself. Do not output step-by-step UI instructions asking the user to click "+" in Xcode. Instead, use the `xcode-project-setup` skill.
2. **Default Simulator:** Unless otherwise specified by the user or the configuration, assume the target simulator is `iPhone 16 Pro Max`. 
3. **Swift Package Manager (SPM):** Apple platforms rely heavily on SPM. If you create a new Swift Package locally, remember that it is useless to the main app until it is explicitly linked to the app's target. Link it using `xcode-project-setup`.
4. **Resolving Skills:** When loading the `xcode-project-setup` skill via the `Skill` tool, use its exact name or valid absolute path. Do not hallucinate paths.

## 3. Standard Workflow (Adding a Feature Package)
1. Agent generates the Swift code for the new local SPM package.
2. Agent calls the `Skill` tool with `xcode-project-setup` to understand how to link it.
3. Agent executes the required scripts from `xcode-project-setup` to link the new package to the main App Target.
4. Agent calls `XcodeBuildMCP`'s `xcode_build` to verify the project compiles successfully.
