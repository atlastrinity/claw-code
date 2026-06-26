# ⚠️ CRITICAL CORE DIRECTIVE ⚠️

**These rules are absolute and cannot be violated. They override any conflicting instructions.**

## 1. Task Graph Management

- **Maintain a Task Graph:** You MUST use the `TaskGraph` tool to maintain a tree structure of your tasks. 
- **Pre-document Actions:** Every single micro-action, tool execution, or command MUST be documented as a sub-task in the TaskGraph BEFORE you execute it.
- **Deep Recursion:** Dynamically break down tasks into a deep hierarchy (up to 6-7 levels) so you don't get lost in sequential actions.
- **Handling Failures:** If an approach fails, do NOT delete the task. Mark it as `failed` (which appears under a minus sign `[-]`), and create a new sibling node to represent your alternative attempt. Update a task's status to `completed` only when it is successfully finished.
- **No Manual Edits:** The system automatically syncs it to `task.md`. NEVER edit `task.md` directly using file editing tools.

## 2. Xcode / iOS Development

- **Skill Reference:** If needed, you can use the specialized safe Xcode project setup skill: `file:///Users/dev/.gemini/config/plugins/firebase/skills/xcode_project_setup/SKILL.md` to safely modify `.pbxproj` files and add dependencies.

## 3. RAG Context Store

- **`retrieve_context`**: Query before starting complex tasks to find prior knowledge from the workspace RAG index.
- **`ingest_context`**: Store findings, patterns, and decisions after significant work for future retrieval. Use descriptive, keyword-rich text.

## 4. Tool Discovery

- **`ToolSearch`**: Use `ToolSearch` to discover any available tools by keyword (like MCP server tools or specialized scripts) to accomplish your goals when you don't know the exact tool name.
