use crate::task_graph::propagation::propagate_task_statuses;
use crate::task_graph::store::{
    compute_active_recursion_branch, compute_progress, find_active_leaf, load_graph_nodes,
    render_nodes_markdown, save_task_graph_output_with_review, task_graph_store_path,
};
use crate::task_graph::types::{
    compare_task_ids, parent_id_from_task_id, TaskGraphInput, TaskGraphOperation, TaskGraphOutput,
    TaskNode, TaskStatus,
};
use crate::task_graph::validation::validate_task_graph;

pub fn run_task_graph(input: TaskGraphInput) -> Result<String, String> {
    let output = execute_task_graph(input)?;
    serde_json::to_string(&output).map_err(|e| e.to_string())
}

pub fn execute_task_graph(input: TaskGraphInput) -> Result<TaskGraphOutput, String> {
    let store_path = task_graph_store_path()?;
    let mut current_nodes = load_graph_nodes(&store_path);

    let mut updated_count = 0;
    let mut grisha_remarks = None;
    let mut newly_failed_nodes = Vec::new();

    match input.operation {
        // 'view' is a read-only snapshot: no gates, no propagation, no store write.
        TaskGraphOperation::View => {
            let (active_leaf_id, active_recursion_chain, active_branch_summary) =
                compute_active_recursion_branch(&current_nodes);
            let snapshot = if current_nodes.is_empty() {
                "Task graph is empty. Create a plan with operation: \"add\".".to_string()
            } else {
                format!(
                    "Current task graph (read-only snapshot):\n\n{}",
                    render_nodes_markdown(&current_nodes)
                )
            };
            return Ok(TaskGraphOutput {
                nodes_updated: 0,
                active_leaf_id,
                active_recursion_chain,
                active_branch_summary,
                grisha_review: None,
                alert: Some(snapshot),
                progress: compute_progress(&current_nodes),
            });
        }
        TaskGraphOperation::Add => {
            // Grisha Plan Review & Enrichment Gatekeeper
            let review_outcome =
                crate::grisha::GrishaSupervisor::review_plan(&input.nodes, &current_nodes)?;
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
        }
        TaskGraphOperation::UpdateStatus => {
            // Grisha Phase Transition Gate: prevent starting Phase N if Phase N-1 is incomplete
            crate::grisha::GrishaSupervisor::validate_phase_transition(
                &input.nodes,
                &current_nodes,
            )?;

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
                    let new_status = node.status.unwrap_or(TaskStatus::Pending);
                    let content = node
                        .content
                        .clone()
                        .or_else(|| Some(format!("Task {}", node.id)));
                    current_nodes.push(TaskNode {
                        id: node.id.clone(),
                        parent_id: node
                            .parent_id
                            .clone()
                            .or_else(|| parent_id_from_task_id(&node.id)),
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
        TaskGraphOperation::Remove => {
            // Validate the whole batch first so a bad id cannot half-apply a removal.
            for node in &input.nodes {
                let Some(pos) = current_nodes.iter().position(|n| n.id == node.id) else {
                    return Err(format!(
                        "Error: TaskGraph remove: task '{}' does not exist. Nothing was removed — use operation: \"view\" to read the current node ids.",
                        node.id
                    ));
                };
                if current_nodes
                    .iter()
                    .any(|n| n.parent_id.as_deref() == Some(&node.id))
                {
                    return Err(format!(
                        "Error: TaskGraph remove: task '{}' has sub-tasks. Remove its leaves first, or mark '{}' as 'failed' to abandon the whole branch.",
                        node.id, node.id
                    ));
                }
                let status = current_nodes[pos].status.unwrap_or(TaskStatus::Pending);
                if status != TaskStatus::Pending {
                    return Err(format!(
                        "Error: TaskGraph remove: task '{}' is '{:?}'. Only 'pending' tasks can be removed — executed or abandoned work stays in the graph so the history is preserved. Use operation: \"update_status\" with 'failed' to skip a branch.",
                        node.id, status
                    ));
                }
            }
            for node in &input.nodes {
                if let Some(pos) = current_nodes.iter().position(|n| n.id == node.id) {
                    current_nodes.remove(pos);
                    updated_count += 1;
                }
            }
            // Prune auto-created placeholder parents that just lost their last child.
            let childless: std::collections::HashSet<String> = current_nodes
                .iter()
                .filter(|n| {
                    !current_nodes
                        .iter()
                        .any(|c| c.parent_id.as_deref() == Some(&n.id))
                })
                .map(|n| n.id.clone())
                .collect();
            current_nodes.retain(|n| {
                if !childless.contains(&n.id) {
                    return true;
                }
                let placeholder = if n.id.contains('.') {
                    format!("Task {}", n.id)
                } else {
                    format!("Phase {}", n.id)
                };
                n.content.as_deref() != Some(placeholder.as_str())
            });
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
        node.parent_id = parent_id_from_task_id(&node.id);
    }

    // Auto-sort nodes semantically by id (e.g., 5.8.1)
    current_nodes.sort_by(|a, b| compare_task_ids(&a.id, &b.id));

    // Auto-propagate statuses up & down the hierarchy
    propagate_task_statuses(&mut current_nodes);

    // Validate transitions.
    //
    // A rejection used to return early and skip the store write, so the statuses
    // the agent had just recorded were thrown away. Because the inconsistency
    // lives in the stored graph rather than in the rejected call, the next call
    // hit the same wall: the graph could not be repaired by retrying, and every
    // attempt cost a full model round-trip. Persist the normalized graph first so
    // recorded progress survives the ordering complaint.
    if let Err(problem) = validate_task_graph(&current_nodes) {
        save_task_graph_output_with_review(
            &store_path,
            &current_nodes,
            updated_count,
            None,
            grisha_remarks,
        )?;
        return Err(format!(
            "{problem}\n\n\
             NOTE: the status changes from this call were still recorded — do not repeat them. \
             Fix the ordering described above in your next TaskGraph call."
        ));
    }

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
                    c.status == Some(TaskStatus::Completed) || c.status == Some(TaskStatus::Failed)
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
            let has_children = current_nodes
                .iter()
                .any(|n| n.parent_id.as_ref() == Some(fid));
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

    if let Some(active) = find_active_leaf(&current_nodes) {
        let has_children = current_nodes
            .iter()
            .any(|n| n.parent_id.as_ref() == Some(&active.id));
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

    // A graph whose nodes are all terminal is *closed*, which is not the same as
    // *done*. Congratulating a graph that still holds `Failed` leaves is how a
    // run reports success having produced nothing, so the abandoned titles have
    // to be in front of the model — and through it, the user — instead of a 🎉.
    let graph_is_terminal = !current_nodes.is_empty()
        && current_nodes.iter().all(|n| {
            n.status == Some(TaskStatus::Completed) || n.status == Some(TaskStatus::Failed)
        });
    if graph_is_terminal {
        let failed_leaves: Vec<&TaskNode> = current_nodes
            .iter()
            .filter(|n| {
                n.status == Some(TaskStatus::Failed)
                    && !current_nodes
                        .iter()
                        .any(|c| c.parent_id.as_deref() == Some(&n.id))
            })
            .collect();
        if failed_leaves.is_empty() {
            alerts.push(
                "🎉 Every task in the graph is COMPLETED. Summarize the delivered work for the \
                 user — name each artifact and how it was verified — then ask for the next goal."
                    .to_string(),
            );
        } else {
            let completed = current_nodes
                .iter()
                .filter(|n| n.status == Some(TaskStatus::Completed))
                .count();
            let lost = failed_leaves
                .iter()
                .map(|n| {
                    format!(
                        "- {} — \"{}\"",
                        n.id,
                        n.content.as_deref().unwrap_or("(untitled)")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            alerts.push(format!(
                "⛔ GRAPH CLOSED WITH UNFINISHED WORK — this is not a success.\n\
                 {completed} of {} tasks completed; these were abandoned:\n{lost}\n\
                 Do NOT tell the user the goal was achieved. Report the completed part and this \
                 list of unfinished work plainly, state why each item failed, and name one \
                 concrete way to finish each one. Do not ask the user which trade-off they want — \
                 a closed graph is a report, not a question; only a decision the user alone can \
                 make (authority, credential, money) belongs in a request.",
                current_nodes.len()
            ));
        }
    }

    let alert = if !alerts.is_empty() {
        Some(alerts.join("\n\n"))
    } else {
        None
    };

    save_task_graph_output_with_review(
        &store_path,
        &current_nodes,
        updated_count,
        alert,
        grisha_remarks,
    )
}
