use crate::task_graph::types::{
    compare_task_ids, parent_id_from_task_id, TaskGraphOutput, TaskNode, TaskStatus,
};
use std::path::{Path, PathBuf};

pub fn task_graph_store_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("CLAWD_TASK_GRAPH_STORE") {
        return Ok(PathBuf::from(path));
    }
    Ok(runtime::workspace::workspace_root().join(".clawd-task-graph.json"))
}

/// Load the authoritative graph state: `task.md` when present (agents may edit
/// the human-readable roadmap), merged with any stored nodes that vanished from
/// it (a removed node counts as abandoned unless already completed).
///
/// Returns an empty Vec when neither source exists.
pub fn load_graph_nodes(store_path: &Path) -> Vec<TaskNode> {
    let stored_nodes = read_stored_nodes(store_path);

    let Some(parent) = store_path.parent() else {
        return stored_nodes;
    };
    let task_md_path = parent.join("task.md");
    let Ok(content) = std::fs::read_to_string(&task_md_path) else {
        return stored_nodes;
    };

    let mut current_nodes = parse_task_md_to_nodes(&content);
    // Restore any nodes that were omitted or deleted from task.md
    for stored in &stored_nodes {
        if !current_nodes.iter().any(|n| n.id == stored.id) {
            let mut node = stored.clone();
            if node.status != Some(TaskStatus::Completed) {
                node.status = Some(TaskStatus::Failed);
            }
            current_nodes.push(node);
        }
    }

    if current_nodes.is_empty() {
        return stored_nodes;
    }
    current_nodes
}

fn read_stored_nodes(store_path: &Path) -> Vec<TaskNode> {
    let Ok(content) = std::fs::read_to_string(store_path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<TaskNode>>(&content).unwrap_or_default()
}

/// Deepest currently-active leaf: the node the agent should be working on.
/// Comparing by string length (`id.len()`) mis-ranked ids once the graph grew
/// past 9 siblings ("1.10" vs "1.9.1"), so ranking is by real hierarchy depth,
/// then by numeric id position.
pub fn find_active_leaf(nodes: &[TaskNode]) -> Option<&TaskNode> {
    nodes
        .iter()
        .filter(|node| node.status == Some(TaskStatus::InProgress))
        .max_by(|a, b| {
            a.depth()
                .cmp(&b.depth())
                .then_with(|| compare_task_ids(&a.id, &b.id))
        })
}

fn write_atomically(path: &Path, content: &str) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Invalid store path: {}", path.display()))?;
    let tmp_path = path.with_file_name(format!("{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp_path, content).map_err(|error| error.to_string())?;
    std::fs::rename(&tmp_path, path).map_err(|error| error.to_string())
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

                nodes.push(TaskNode {
                    id: id.clone(),
                    parent_id: parent_id_from_task_id(&id),
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
                !nodes
                    .iter()
                    .any(|other| other.parent_id.as_deref() == Some(id))
            }
        })
        .map(|(idx, _)| idx)
        .collect();

    // If more than 1 leaf node is marked InProgress, keep only the deepest /
    // highest-positioned one active (ids compare numerically per segment).
    if leaf_in_progress_indices.len() > 1 {
        let active_index = *leaf_in_progress_indices
            .iter()
            .max_by(|&&a, &&b| {
                nodes[a]
                    .depth()
                    .cmp(&nodes[b].depth())
                    .then_with(|| compare_task_ids(&nodes[a].id, &nodes[b].id))
            })
            .unwrap();
        for &idx in &leaf_in_progress_indices {
            if idx != active_index {
                nodes[idx].status = Some(TaskStatus::Pending);
            }
        }
    }
}

pub fn create_task_checkpoint(store_path: &Path, completed_nodes: &[TaskNode]) {
    if completed_nodes.is_empty() {
        return;
    }
    let Some(parent) = store_path.parent() else {
        return;
    };
    let checkpoints_dir = parent.join(".claw").join("checkpoints");
    let _ = std::fs::create_dir_all(&checkpoints_dir);

    for node in completed_nodes {
        let checkpoint_file =
            checkpoints_dir.join(format!("task_{}.json", node.id.replace('.', "_")));
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

pub fn compute_active_recursion_branch(
    nodes: &[TaskNode],
) -> (Option<String>, Vec<String>, Option<String>) {
    let Some(active_leaf) = find_active_leaf(nodes) else {
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
    // The old rendering printed every ancestor's full title on every graph
    // response, so a deep plan spent most of its tokens re-reading a plan the
    // model had already seen. The leaf and its immediate parent are what the next
    // action depends on; the rest is one line of ids.
    let leaf_title = active_leaf.content.as_deref().unwrap_or("");
    let mut summary_lines = vec![format!(
        "⚡ ACTIVE LEAF [{leaf_id}] \"{leaf_title}\" — finish this before any sibling."
    )];
    if chain_nodes.len() >= 2 {
        let parent = chain_nodes[chain_nodes.len() - 2];
        summary_lines.push(format!(
            "   under [{}] \"{}\"",
            parent.id,
            parent.content.as_deref().unwrap_or("")
        ));
    }
    if chain_ids.len() >= 2 {
        summary_lines.push(format!("   chain: {}", chain_ids.join(" → ")));
    }
    let summary = format!(
        "{}\nFull graph: task.md or TaskGraph operation 'view'.",
        summary_lines.join("\n")
    );

    (Some(leaf_id), chain_ids, Some(summary))
}

pub fn save_task_graph_output(
    store_path: &Path,
    current_nodes: &[TaskNode],
    updated_count: usize,
    alert: Option<String>,
) -> Result<TaskGraphOutput, String> {
    save_task_graph_output_with_review(store_path, current_nodes, updated_count, alert, None)
}

pub fn save_task_graph_output_with_review(
    store_path: &Path,
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
> 2. The `TaskGraph` tool has four operations: `add`, `update_status`, `view`, `remove`.
> 3. To update an existing task, use `operation: "update_status"`. CRITICAL: ONLY send the nodes whose status is changing (provide only `id` and `status`). Do NOT send the entire graph, unchanged nodes, or the `content`/`parent_id` fields. Example: `{"operation":"update_status","nodes":[{"id":"2.1","status":"completed"}]}` — this is correct. Sending 10+ nodes is WRONG.
> 4. **RECURSIVE DECOMPOSITION & RIGOROUS EXECUTION**: Formulate the task graph clearly and logically. Whenever an active sub-task (e.g. "1.7") involves multiple distinct phases or complex operations, you MUST expand it recursively into leaf sub-tasks (e.g. "1.7.1", "1.7.2") using operation: "add" (with parent_id: "1.7") BEFORE executing mutating actions under it. DO NOT decompose single atomic actions (like editing a file or running a single command) into redundant sub-tasks, and NEVER create sub-tasks with descriptions identical to parent tasks. Maximum recursion depth is 5. Read-only diagnostic commands do NOT need separate tasks.
> 5. Node IDs MUST be strings (e.g. "1.1").
> 6. Do NOT prefix the `content` field with the node ID (e.g., write "Task description", NOT "1.1: Task description"). Ensure parent nodes exist before creating deep children (e.g., create 1.1 before 1.1.1).
> 7. **ANTI-REWRITE**: Do NOT use `add` to resubmit the entire graph. Only add genuinely NEW nodes. Parent statuses propagate AUTOMATICALLY — you do NOT need to manually set parent status when completing children.
> 8. **LANGUAGE MANDATE**: All node content, task titles, and sub-steps MUST be written strictly in English (e.g. "Analyze Swift code", NOT Ukrainian/other languages).
> 9. **AUTO-VERIFICATION MANDATE**: Before marking any task as `completed`, you MUST verify your work (e.g. read the modified file, compile, run tests, check status). Do not mark as completed based solely on a successful mutating tool call.
> 10. **VIEW**: To re-read the graph without touching it, call `{"operation":"view","nodes":[]}`. It never runs gates or rewrites the store.
> 11. **REMOVE**: To drop a genuinely obsolete `pending` leaf, use `{"operation":"remove","nodes":[{"id":"2.1.3"}]}`. Only pending leaves can be removed; never remove work that was executed (use `failed` for abandoned branches so history is preserved).

# Task List

"#,
        );

        markdown.push_str(&render_nodes_markdown(&mutable_nodes));
        let _ = write_atomically(&task_md_path, &markdown);
    }

    write_atomically(
        store_path,
        &serde_json::to_string_pretty(&mutable_nodes).map_err(|e| e.to_string())?,
    )?;

    let (active_leaf_id, active_recursion_chain, active_branch_summary) =
        compute_active_recursion_branch(&mutable_nodes);

    Ok(TaskGraphOutput {
        nodes_updated: updated_count,
        active_leaf_id,
        active_recursion_chain,
        active_branch_summary,
        grisha_review,
        alert,
        progress: compute_progress(&mutable_nodes),
    })
}

/// Render the checklist body of task.md (one line per node, indented by depth).
pub fn render_nodes_markdown(nodes: &[TaskNode]) -> String {
    let mut markdown = String::new();
    for node in nodes {
        let depth = node.depth().saturating_sub(1);
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
    markdown
}

/// One-line scoreboard appended to every TaskGraph tool result so progress is
/// visible without re-reading task.md.
pub fn compute_progress(nodes: &[TaskNode]) -> Option<String> {
    if nodes.is_empty() {
        return None;
    }
    let total = nodes.len();
    let done = nodes
        .iter()
        .filter(|n| n.status == Some(TaskStatus::Completed))
        .count();
    let failed = nodes
        .iter()
        .filter(|n| n.status == Some(TaskStatus::Failed))
        .count();
    let percent = done.saturating_mul(100) / total;
    let mut line = format!("📊 Progress: {done}/{total} complete ({percent}%)");
    if failed > 0 {
        line.push_str(&format!(" · {failed} failed"));
    }
    if let Some(active) = find_active_leaf(nodes) {
        line.push_str(&format!(" · active: #{}", active.id));
    }
    Some(line)
}

pub fn build_active_hierarchy_prompt() -> Option<String> {
    let store_path = task_graph_store_path().ok()?;
    let nodes = load_graph_nodes(&store_path);

    if nodes.is_empty() {
        return None;
    }

    let active_leaf = find_active_leaf(&nodes)?;

    // Find Root node (e.g. "1" or "2" or top level parent)
    let root_id = active_leaf.id.split('.').next().unwrap_or(&active_leaf.id);
    let root_node = nodes.iter().find(|n| n.id == root_id);

    // Build parent chain
    let mut chain: Vec<&TaskNode> = Vec::new();
    let mut current = active_leaf;
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
