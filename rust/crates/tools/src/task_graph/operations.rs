use crate::task_graph::propagation::propagate_task_statuses;
use crate::task_graph::store::{
    parse_task_md_to_nodes, save_task_graph_output_with_review, task_graph_store_path,
};
use crate::task_graph::types::{
    TaskGraphInput, TaskGraphOperation, TaskGraphOutput, TaskNode, TaskStatus,
};
use crate::task_graph::validation::validate_task_graph;

pub fn run_task_graph(input: TaskGraphInput) -> Result<String, String> {
    let output = execute_task_graph(input)?;
    serde_json::to_string(&output).map_err(|e| e.to_string())
}

pub fn execute_task_graph(input: TaskGraphInput) -> Result<TaskGraphOutput, String> {
    let store_path = task_graph_store_path()?;
    let mut current_nodes = Vec::new();

    let mut stored_nodes: Vec<TaskNode> = Vec::new();
    if store_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&store_path) {
            if let Ok(nodes) = serde_json::from_str::<Vec<TaskNode>>(&content) {
                stored_nodes = nodes;
            }
        }
    }

    if let Some(parent) = store_path.parent() {
        let task_md_path = parent.join("task.md");
        if task_md_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&task_md_path) {
                current_nodes = parse_task_md_to_nodes(&content);
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
            }
        }
    }

    if current_nodes.is_empty() {
        current_nodes = stored_nodes;
    }

    let mut updated_count = 0;
    let mut grisha_remarks = None;
    let mut newly_failed_nodes = Vec::new();

    match input.operation {
        TaskGraphOperation::Add => {
            // Grisha Plan Review & Enrichment Gatekeeper
            let review_outcome = crate::grisha::GrishaSupervisor::review_plan(&input.nodes, &current_nodes)?;
            if !review_outcome.remarks.is_empty() {
                grisha_remarks = Some(review_outcome.remarks);
            }

            for node in review_outcome.enriched_nodes {
                if let Some(existing) = current_nodes.iter_mut().find(|n| n.id == node.id) {
                    if let Some(content) = node.content {
                        existing.content = Some(content);
                    }
                    if let Some(status) = node.status {
                        existing.status = Some(status);
                    }
                    updated_count += 1;
                } else {
                    current_nodes.push(node);
                    updated_count += 1;
                }
            }

            // Auto-repair parent status: if a parent is marked Completed but has active children (InProgress/Pending),
            // automatically set parent status to InProgress so parent-child hierarchy remains consistent!
            for i in 0..current_nodes.len() {
                let pid = current_nodes[i].id.clone();
                let has_active_children = current_nodes.iter().any(|n| {
                    n.parent_id.as_ref() == Some(&pid)
                        && (n.status == Some(TaskStatus::InProgress)
                            || n.status == Some(TaskStatus::Pending))
                });
                if has_active_children
                    && (current_nodes[i].status == Some(TaskStatus::Completed)
                        || current_nodes[i].status == Some(TaskStatus::Failed))
                {
                    current_nodes[i].status = Some(TaskStatus::InProgress);
                }
            }
        }
        TaskGraphOperation::UpdateStatus => {
            // Grisha Phase Transition Gate: prevent starting Phase N if Phase N-1 is incomplete
            crate::grisha::GrishaSupervisor::validate_phase_transition(&input.nodes, &current_nodes)?;

            let mut cascade_completed = Vec::new();

            for node in input.nodes {
                if let Some(existing) = current_nodes.iter_mut().find(|n| n.id == node.id) {
                    if let Some(new_status) = node.status {
                        if existing.status.as_ref() == Some(&new_status) {
                            continue;
                        }
                        existing.status = Some(new_status);
                        updated_count += 1;
                        if new_status == TaskStatus::Completed {
                            cascade_completed.push(node.id.clone());
                        } else if new_status == TaskStatus::Failed {
                            newly_failed_nodes.push(node.id.clone());
                        }
                    }
                } else {
                    // Auto-upsert missing nodes so models never get stuck if a node is not yet added
                    let parts: Vec<&str> = node.id.split('.').collect();
                    let parent_id = if parts.len() > 1 {
                        Some(parts[..parts.len() - 1].join("."))
                    } else {
                        None
                    };
                    let new_status = node.status.unwrap_or(TaskStatus::Pending);
                    let content = node
                        .content
                        .clone()
                        .or_else(|| Some(format!("Task {}", node.id)));
                    current_nodes.push(TaskNode {
                        id: node.id.clone(),
                        parent_id: node.parent_id.clone().or(parent_id),
                        content,
                        status: Some(new_status),
                    });
                    updated_count += 1;
                    if new_status == TaskStatus::Completed {
                        cascade_completed.push(node.id.clone());
                    } else if new_status == TaskStatus::Failed {
                        newly_failed_nodes.push(node.id.clone());
                    }
                }
            }

            // Cascade completion to non-terminal sub-tasks when parent is explicitly set to Completed
            while !cascade_completed.is_empty() {
                let current_parent_ids = std::mem::take(&mut cascade_completed);
                for n in &mut current_nodes {
                    if let Some(ref pid) = n.parent_id {
                        if current_parent_ids.contains(pid)
                            && n.status != Some(TaskStatus::Completed)
                            && n.status != Some(TaskStatus::Failed)
                        {
                            n.status = Some(TaskStatus::Completed);
                            cascade_completed.push(n.id.clone());
                        }
                    }
                }
            }
        }
    }

    // Auto-repair parent status: if a parent has active children (InProgress/Pending),
    // its status MUST be InProgress so parent-child hierarchy remains consistent and children are never skipped!
    for i in 0..current_nodes.len() {
        let pid = current_nodes[i].id.clone();
        let has_active_children = current_nodes.iter().any(|n| {
            n.parent_id.as_ref() == Some(&pid)
                && (n.status == Some(TaskStatus::InProgress)
                    || n.status == Some(TaskStatus::Pending))
        });
        if has_active_children
            && (current_nodes[i].status == Some(TaskStatus::Completed)
                || current_nodes[i].status == Some(TaskStatus::Failed))
        {
            current_nodes[i].status = Some(TaskStatus::InProgress);
        }
    }

    // Auto-create missing parent nodes based on the ID structure of existing nodes
    let mut missing_parents = Vec::new();
    for node in &current_nodes {
        let parts: Vec<&str> = node.id.split('.').collect();
        for i in 1..parts.len() {
            let parent_id = parts[..i].join(".");
            if !current_nodes.iter().any(|n| n.id == parent_id)
                && !missing_parents.iter().any(|n: &TaskNode| n.id == parent_id)
            {
                let content = if parent_id.split('.').count() == 1 {
                    format!("Phase {}", parent_id)
                } else {
                    format!("Task {}", parent_id)
                };
                missing_parents.push(TaskNode {
                    id: parent_id.clone(),
                    parent_id: if i > 1 {
                        Some(parts[..i - 1].join("."))
                    } else {
                        None
                    },
                    content: Some(content),
                    status: Some(TaskStatus::Pending),
                });
            }
        }
    }
    if !missing_parents.is_empty() {
        updated_count += missing_parents.len();
        current_nodes.extend(missing_parents);
    }

    // Auto-repair parent_ids based on id structure
    for node in &mut current_nodes {
        let parts: Vec<&str> = node.id.split('.').collect();
        if parts.len() > 1 {
            node.parent_id = Some(parts[..parts.len() - 1].join("."));
        } else {
            node.parent_id = None;
        }
    }

    // Auto-sort nodes semantically by id (e.g., 5.8.1)
    current_nodes.sort_by(|a, b| {
        let a_parts: Vec<u32> = a.id.split('.').filter_map(|s| s.parse().ok()).collect();
        let b_parts: Vec<u32> = b.id.split('.').filter_map(|s| s.parse().ok()).collect();
        let cmp = a_parts.cmp(&b_parts);
        if cmp == std::cmp::Ordering::Equal {
            a.id.cmp(&b.id)
        } else {
            cmp
        }
    });

    // Auto-propagate statuses up & down the hierarchy
    propagate_task_statuses(&mut current_nodes);

    // Validate transitions
    validate_task_graph(&current_nodes)?;

    // Check for alerts (all children under an InProgress parent are Completed/Failed)
    let mut finished_parent_ids = Vec::new();
    for node in &current_nodes {
        if node.status == Some(TaskStatus::InProgress) {
            let children: Vec<&TaskNode> = current_nodes
                .iter()
                .filter(|n| n.parent_id.as_ref() == Some(&node.id))
                .collect();
            if !children.is_empty() {
                let all_finished = children.iter().all(|c| {
                    c.status == Some(TaskStatus::Completed)
                        || c.status == Some(TaskStatus::Failed)
                });
                if all_finished {
                    finished_parent_ids.push(node.id.clone());
                }
            }
        }
    }

    let mut alerts = Vec::new();
    if !newly_failed_nodes.is_empty() {
        for fid in &newly_failed_nodes {
            let has_children = current_nodes.iter().any(|n| n.parent_id.as_ref() == Some(fid));
            if !has_children {
                let depth = fid.split('.').count();
                if depth < 4 {
                    alerts.push(format!(
                        "🛡️ Grisha Root-Cause Recovery Advisory: Task '{}' failed. You MUST NOT abandon this phase immediately. Decompose '{}' into deeper subtasks (e.g. '{}.1' [Diagnose Root Cause: inspect logs/environment/devices], '{}.2' [Apply Parameter Fix/Alternative], '{}.3' [Verify]) to attempt root-cause resolution before admitting permanent failure.",
                        fid, fid, fid, fid, fid
                    ));
                } else {
                    alerts.push(format!(
                        "🛡️ Grisha Root-Cause Recovery Advisory: Maximum decomposition depth reached (level {} for '{}'). Do NOT create deeper subtasks. Apply the direct corrective action, or mark this task completed after verification, or proceed to the next sibling task.",
                        depth, fid
                    ));
                }
            }
        }
    }

    if !finished_parent_ids.is_empty() {
        alerts.push(format!(
            "⚠️ Alert: All subtasks under parent task(s) [{}] are completed or failed. Please verify the work, update the parent task status using 'update_status', and proceed to the next sibling task.",
            finished_parent_ids.join(", ")
        ));
    }

    if let Some(active) = current_nodes.iter().find(|n| n.status == Some(TaskStatus::InProgress)) {
        let has_children = current_nodes.iter().any(|n| n.parent_id.as_ref() == Some(&active.id));
        if !has_children {
            let depth = active.id.split('.').count();
            if depth < 3 {
                alerts.push(format!(
                    "⚠️ Deep Recursion Alert: The active task '{}' is at level {} and has no sub-tasks. For deep recursion planning, you MUST decompose tasks to at least level 3 (Phase -> Sub-task -> Micro-action). Please use TaskGraph operation: \"add\" to break this down.",
                    active.id, depth
                ));
            }
        }
    }

    let all_graph_finished = !current_nodes.is_empty()
        && current_nodes.iter().all(|n| {
            n.status == Some(TaskStatus::Completed) || n.status == Some(TaskStatus::Failed)
        });
    if all_graph_finished {
        alerts.push(
            "🎉 All tasks in the task graph are now COMPLETED or finished. Please summarize the completed work for the user and ask for the next goal or task.".to_string()
        );
    }

    let alert = if !alerts.is_empty() {
        Some(alerts.join("\n\n"))
    } else {
        None
    };

    save_task_graph_output_with_review(&store_path, &current_nodes, updated_count, alert, grisha_remarks)
}
