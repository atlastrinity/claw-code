use std::path::PathBuf;
use crate::task_graph::types::{TaskNode, TaskStatus, TaskGraphOutput};

pub fn task_graph_store_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("CLAWD_TASK_GRAPH_STORE") {
        return Ok(PathBuf::from(path));
    }
    Ok(runtime::workspace::workspace_root().join(".clawd-task-graph.json"))
}

pub fn parse_task_md_to_nodes(content: &str) -> Vec<TaskNode> {
    let mut nodes = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- ") {
            continue;
        }

        let status = if trimmed.contains("[x]") {
            Some(TaskStatus::Completed)
        } else if trimmed.contains("[/]") {
            Some(TaskStatus::InProgress)
        } else if trimmed.contains("[-]") {
            Some(TaskStatus::Failed)
        } else {
            Some(TaskStatus::Pending)
        };

        if let Some(first_star) = trimmed.find("**") {
            if let Some(second_star) = trimmed[first_star + 2..].find("**") {
                let id = trimmed[first_star + 2..first_star + 2 + second_star]
                    .trim()
                    .to_string();
                let after_id = &trimmed[first_star + 2 + second_star + 2..];
                let content_str = after_id.trim_start_matches(':').trim().to_string();

                let parts: Vec<&str> = id.split('.').collect();
                let parent_id = if parts.len() > 1 {
                    Some(parts[..parts.len() - 1].join("."))
                } else {
                    None
                };

                nodes.push(TaskNode {
                    id,
                    parent_id,
                    content: Some(content_str),
                    status,
                });
            }
        }
    }
    nodes
}

pub fn normalize_single_active_leaf(nodes: &mut [TaskNode]) {
    // Find all leaf nodes currently set to InProgress
    let leaf_in_progress_indices: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            n.status == Some(TaskStatus::InProgress) && {
                let id = &n.id;
                !nodes.iter().any(|other| other.parent_id.as_deref() == Some(id))
            }
        })
        .map(|(idx, _)| idx)
        .collect();

    // If more than 1 leaf node is marked InProgress, keep only the latest/deepest one active
    if leaf_in_progress_indices.len() > 1 {
        let active_index = *leaf_in_progress_indices.last().unwrap();
        for &idx in &leaf_in_progress_indices {
            if idx != active_index {
                nodes[idx].status = Some(TaskStatus::Pending);
            }
        }
    }
}

pub fn create_task_checkpoint(store_path: &PathBuf, completed_nodes: &[TaskNode]) {
    if completed_nodes.is_empty() { return; }
    let Some(parent) = store_path.parent() else { return; };
    let checkpoints_dir = parent.join(".claw").join("checkpoints");
    let _ = std::fs::create_dir_all(&checkpoints_dir);

    for node in completed_nodes {
        let checkpoint_file = checkpoints_dir.join(format!("task_{}.json", node.id.replace('.', "_")));
        let snapshot = serde_json::json!({
            "task_id": node.id,
            "content": node.content,
            "status": "completed",
            "timestamp_secs": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(checkpoint_file, json);
        }
    }
}

pub fn compute_active_recursion_branch(nodes: &[TaskNode]) -> (Option<String>, Vec<String>, Option<String>) {
    let mut in_progress_nodes: Vec<&TaskNode> = nodes
        .iter()
        .filter(|node| node.status == Some(TaskStatus::InProgress))
        .collect();
    in_progress_nodes.sort_by(|a, b| b.id.len().cmp(&a.id.len()));

    let Some(active_leaf) = in_progress_nodes.first().copied() else {
        return (None, Vec::new(), None);
    };

    let leaf_id = active_leaf.id.clone();
    let mut chain_nodes = Vec::new();
    let mut current = active_leaf;
    chain_nodes.push(current);

    while let Some(ref parent_id) = current.parent_id {
        if let Some(parent) = nodes.iter().find(|n| &n.id == parent_id) {
            chain_nodes.push(parent);
            current = parent;
        } else {
            break;
        }
    }
    chain_nodes.reverse();

    let chain_ids: Vec<String> = chain_nodes.iter().map(|n| n.id.clone()).collect();
    let mut summary_lines = Vec::new();

    for (idx, node) in chain_nodes.iter().enumerate() {
        let is_leaf = idx == chain_nodes.len() - 1;
        let tag = if is_leaf { "⚡ ACTIVE LEAF (Lowest Order)" } else { "📂 PARENT PHASE" };
        summary_lines.push(format!(
            "[{}] \"{}\" ({})",
            node.id,
            node.content.as_deref().unwrap_or(""),
            tag
        ));
    }

    let summary = format!(
        "Recursive Task Chain (Finish lowest order leaf task first):\n{}\n\nStrict recursive dependency active. Full graph available in task.md or via TaskGraph operation 'view'.",
        summary_lines.join(" ->\n  ")
    );

    (Some(leaf_id), chain_ids, Some(summary))
}

pub fn save_task_graph_output(
    store_path: &PathBuf,
    current_nodes: &[TaskNode],
    updated_count: usize,
    alert: Option<String>,
) -> Result<TaskGraphOutput, String> {
    save_task_graph_output_with_review(store_path, current_nodes, updated_count, alert, None)
}

pub fn save_task_graph_output_with_review(
    store_path: &PathBuf,
    current_nodes: &[TaskNode],
    updated_count: usize,
    alert: Option<String>,
    grisha_review: Option<Vec<String>>,
) -> Result<TaskGraphOutput, String> {
    let mut mutable_nodes = current_nodes.to_vec();
    normalize_single_active_leaf(&mut mutable_nodes);

    let completed_nodes: Vec<TaskNode> = mutable_nodes
        .iter()
        .filter(|n| n.status == Some(TaskStatus::Completed))
        .cloned()
        .collect();
    create_task_checkpoint(store_path, &completed_nodes);

    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;

        let task_md_path = parent.join("task.md");
        let mut markdown = String::from(
            r#"> [!IMPORTANT]
> **AGENT INSTRUCTIONS FOR UPDATING THIS TASK GRAPH:**
> 1. Do NOT edit this file directly. You MUST use the `TaskGraph` tool.
> 2. The `TaskGraph` tool has two operations: `add` and `update_status`.
> 3. To update an existing task, use `operation: "update_status"`. CRITICAL: ONLY send the nodes whose status is changing (provide only `id` and `status`). Do NOT send the entire graph, unchanged nodes, or the `content`/`parent_id` fields. Example: `{"operation":"update_status","nodes":[{"id":"2.1","status":"completed"}]}` — this is correct. Sending 10+ nodes is WRONG.
> 4. **RECURSIVE DECOMPOSITION & RIGOROUS EXECUTION**: Formulate the task graph clearly and logically. Whenever an active sub-task (e.g. "1.7") involves multiple distinct phases or complex operations, you MUST expand it recursively into leaf sub-tasks (e.g. "1.7.1", "1.7.2") using operation: "add" (with parent_id: "1.7") BEFORE executing mutating actions under it. DO NOT decompose single atomic actions (like editing a file or running a single command) into redundant sub-tasks, and NEVER create sub-tasks with descriptions identical to parent tasks. Maximum recursion depth is 5. Read-only diagnostic commands do NOT need separate tasks.
> 5. Node IDs MUST be strings (e.g. "1.1").
> 6. Do NOT prefix the `content` field with the node ID (e.g., write "Task description", NOT "1.1: Task description"). Ensure parent nodes exist before creating deep children (e.g., create 1.1 before 1.1.1).
> 7. **ANTI-REWRITE**: Do NOT use `add` to resubmit the entire graph. Only add genuinely NEW nodes. Parent statuses propagate AUTOMATICALLY — you do NOT need to manually set parent status when completing children.
> 8. **LANGUAGE MANDATE**: All node content, task titles, and sub-steps MUST be written strictly in English (e.g. "Analyze Swift code", NOT Ukrainian/other languages).
> 9. **AUTO-VERIFICATION MANDATE**: Before marking any task as `completed`, you MUST verify your work (e.g. read the modified file, compile, run tests, check status). Do not mark as completed based solely on a successful mutating tool call.

# Task List

"#,
        );

        for node in &mutable_nodes {
            let depth = node.id.split('.').count().saturating_sub(1);
            let checkbox = match node.status {
                Some(TaskStatus::Completed) => "[x]",
                Some(TaskStatus::InProgress) => "[/]",
                Some(TaskStatus::Failed) => "[-]",
                _ => "[ ]",
            };
            let indent = "  ".repeat(depth);
            markdown.push_str(&format!(
                "{}- {} **{}**: {}\n",
                indent,
                checkbox,
                node.id,
                node.content.as_deref().unwrap_or("")
            ));
        }

        let _ = std::fs::write(&task_md_path, markdown);
    }

    std::fs::write(
        store_path,
        serde_json::to_string_pretty(&mutable_nodes).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let (active_leaf_id, active_recursion_chain, active_branch_summary) =
        compute_active_recursion_branch(&mutable_nodes);

    Ok(TaskGraphOutput {
        nodes_updated: updated_count,
        active_leaf_id,
        active_recursion_chain,
        active_branch_summary,
        grisha_review,
        alert,
    })
}

pub fn build_active_hierarchy_prompt() -> Option<String> {
    let store_path = task_graph_store_path().ok()?;
    let mut nodes = Vec::new();

    if store_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&store_path) {
            if let Ok(n) = serde_json::from_str::<Vec<TaskNode>>(&content) {
                nodes = n;
            }
        }
    }

    if nodes.is_empty() {
        if let Some(parent) = store_path.parent() {
            let task_md_path = parent.join("task.md");
            if task_md_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&task_md_path) {
                    nodes = parse_task_md_to_nodes(&content);
                }
            }
        }
    }

    if nodes.is_empty() {
        return None;
    }

    // Find the currently active in_progress node (prefer deepest leaf node)
    let mut in_progress_nodes: Vec<&TaskNode> = nodes
        .iter()
        .filter(|node| node.status == Some(TaskStatus::InProgress))
        .collect();
    in_progress_nodes.sort_by(|a, b| b.id.len().cmp(&a.id.len()));

    let active_leaf = in_progress_nodes.first()?;
    
    // Find Root node (e.g. "1" or "2" or top level parent)
    let root_id = active_leaf.id.split('.').next().unwrap_or(&active_leaf.id);
    let root_node = nodes.iter().find(|n| n.id == root_id);

    // Build parent chain
    let mut chain = Vec::new();
    let mut current = *active_leaf;
    chain.push(current);

    while let Some(ref parent_id) = current.parent_id {
        if let Some(parent) = nodes.iter().find(|n| &n.id == parent_id) {
            chain.push(parent);
            current = parent;
        } else {
            break;
        }
    }
    chain.reverse();

    let mut out = String::from("<active-task-hierarchy>\n");
    out.push_str("🎯 RECURSIVE TASK HIERARCHY & GOAL FOCUS:\n");

    if let Some(root) = root_node {
        if root.id != active_leaf.id {
            out.push_str(&format!(
                "  • ROOT GOAL (#{}): {}\n",
                root.id,
                root.content.as_deref().unwrap_or("Active Goal")
            ));
        }
    }

    for (idx, node) in chain.iter().enumerate() {
        let is_leaf = idx == chain.len() - 1;
        let prefix = if is_leaf {
            "  ↳ ⚡ ACTIVE LEAF TASK"
        } else {
            "  ↳ 📂 PARENT TASK"
        };
        out.push_str(&format!(
            "{} (#{}): {}\n",
            prefix,
            node.id,
            node.content.as_deref().unwrap_or("")
        ));
    }

    out.push_str(
        "\nCRITICAL HIERARCHICAL DIRECTIVES:\n\
         1. Maintain strict execution alignment with Root & Parent goals above while executing Active Leaf Task.\n\
         2. All internal task graph titles, sub-steps, and descriptions MUST be in English.\n\
         3. When performing searches (web/grep), preserve the original query language of the user.\n\
         </active-task-hierarchy>",
    );

    Some(out)
}
