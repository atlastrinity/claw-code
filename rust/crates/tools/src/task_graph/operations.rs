use crate::task_graph::propagation::propagate_task_statuses;
use crate::task_graph::store::{
    parse_task_md_to_nodes, save_task_graph_output, task_graph_store_path,
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
                // Restore any nodes that were deleted from task.md as failed
                for stored in &stored_nodes {
                    if !current_nodes.iter().any(|n| n.id == stored.id) {
                        let mut failed_node = stored.clone();
                        failed_node.status = Some(TaskStatus::Failed);
                        current_nodes.push(failed_node);
                    }
                }
            }
        }
    }

    if current_nodes.is_empty() {
        current_nodes = stored_nodes;
    }

    let mut updated_count = 0;

    match input.operation {
        TaskGraphOperation::Add => {
            // Bulk Rewrite Guard: block 'add' if the graph already has nodes AND the user submits more than 3 nodes
            let existing_graph_size = current_nodes.len();
            let submitted_size = input.nodes.len();
            if existing_graph_size >= 4 && submitted_size >= 4 {
                let duplicate_ids: Vec<String> = input
                    .nodes
                    .iter()
                    .filter(|n| current_nodes.iter().any(|existing| existing.id == n.id))
                    .map(|n| n.id.clone())
                    .collect();
                if duplicate_ids.len() >= 2 {
                    return Err(format!(
                        "Error: TaskGraph bulk rewrite detected. {} of your submitted nodes already exist: [{}]. \
                        The 'add' operation is ONLY for adding NEW sub-tasks to an existing graph. \
                        Do NOT resubmit the entire graph via 'add'. To update statuses of existing nodes, use operation: \"update_status\".",
                        duplicate_ids.len(), duplicate_ids.join(", ")
                    ));
                }
            }

            for node in input.nodes {
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
                if has_active_children && current_nodes[i].status == Some(TaskStatus::Completed) {
                    current_nodes[i].status = Some(TaskStatus::InProgress);
                }
            }
        }
        TaskGraphOperation::UpdateStatus => {
            // Guard: block bulk status updates — send only the 1-5 nodes that actually changed
            let submitted_count = input.nodes.len();
            if submitted_count > 5 {
                return Err(format!(
                    "Error: Bulk update_status detected. You submitted {} nodes but only status CHANGES should be sent. \
                    Do NOT resend the entire graph. Send ONLY the 1-3 nodes whose status is actually changing. \
                    Example: {{\"operation\":\"update_status\",\"nodes\":[{{\"id\":\"3.1\",\"status\":\"in_progress\"}}]}}",
                    submitted_count
                ));
            }

            let mut cascade_completed = Vec::new();
            let mut cascade_failed = Vec::new();

            // Collect IDs of new nodes that don't exist yet
            let mut missing_ids: Vec<String> = Vec::new();

            for node in input.nodes {
                if let Some(existing) = current_nodes.iter_mut().find(|n| n.id == node.id) {
                    if let Some(new_status) = node.status {
                        // Skip nodes whose status hasn't actually changed
                        if existing.status.as_ref() == Some(&new_status) {
                            continue;
                        }
                        existing.status = Some(new_status.clone());
                        updated_count += 1;
                        if new_status == TaskStatus::Completed {
                            cascade_completed.push(node.id.clone());
                        } else if new_status == TaskStatus::Failed {
                            cascade_failed.push(node.id.clone());
                        }
                    }
                } else {
                    missing_ids.push(node.id.clone());
                }
            }

            // If there are missing nodes, return a clear error with 2-step instructions
            if !missing_ids.is_empty() {
                return Err(format!(
                    "Node(s) not found in the task graph: [{}]. You cannot update status of nodes that don't exist yet. \
                    SOLUTION (2 steps): \
                    Step 1: Call TaskGraph with operation: \"add\" to create ONLY the new nodes (e.g. {{\"operation\":\"add\",\"nodes\":[{{\"id\":\"{}\",\"parent_id\":\"...\",\"content\":\"...\"}}]}}). \
                    Step 2: Then call TaskGraph with operation: \"update_status\" to change ONLY the status of existing nodes that need updating.",
                    missing_ids.join(", "), missing_ids[0]
                ));
            }

            // Cascade completion to non-terminal sub-tasks when parent is set to Completed
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

            // Cascade failure to non-terminal sub-tasks when parent is set to Failed
            while !cascade_failed.is_empty() {
                let current_parent_ids = std::mem::take(&mut cascade_failed);
                for n in &mut current_nodes {
                    if let Some(ref pid) = n.parent_id {
                        if current_parent_ids.contains(pid)
                            && n.status != Some(TaskStatus::Completed)
                            && n.status != Some(TaskStatus::Failed)
                        {
                            n.status = Some(TaskStatus::Failed);
                            cascade_failed.push(n.id.clone());
                        }
                    }
                }
            }
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

    let alert = if !finished_parent_ids.is_empty() {
        Some(format!(
            "⚠️ Alert: All subtasks under parent task(s) [{}] are completed or failed. Please verify the work, update the parent task status using 'update_status', and proceed to the next sibling task.",
            finished_parent_ids.join(", ")
        ))
    } else {
        None
    };

    save_task_graph_output(&store_path, &current_nodes, updated_count, alert)
}
