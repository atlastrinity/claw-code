use crate::task_graph::types::{compare_task_ids, TaskNode, TaskStatus};
use std::cmp::Ordering;

pub fn propagate_task_statuses(current_nodes: &mut Vec<TaskNode>) {
    let mut changed = true;
    let mut iterations = 0;
    // Cap propagation iterations to prevent infinite oscillation loops in cyclic/conflicting graphs
    while changed && iterations < 50 {
        iterations += 1;
        changed = false;

        // 0. Demote Completed parents back to InProgress when they have
        //    unclosed children. This handles the case where new subtasks are
        //    added (via 'add' or auto-upsert in 'update_status') under
        //    already-completed parents, preventing a validation deadlock.
        let mut parents_to_demote = Vec::new();
        for node in &*current_nodes {
            if node.status == Some(TaskStatus::Completed) {
                let has_unclosed_child = current_nodes.iter().any(|n| {
                    n.parent_id.as_ref() == Some(&node.id) && {
                        let st = n.status.unwrap_or(TaskStatus::Pending);
                        st != TaskStatus::Completed && st != TaskStatus::Failed
                    }
                });
                if has_unclosed_child {
                    parents_to_demote.push(node.id.clone());
                }
            }
        }
        for p_id in parents_to_demote {
            if let Some(parent) = current_nodes.iter_mut().find(|n| n.id == p_id) {
                parent.status = Some(TaskStatus::InProgress);
                changed = true;
            }
        }

        // 1. Upward InProgress propagation: if a child is InProgress,
        // its parent must also be InProgress.
        let mut parents_to_update = Vec::new();
        for node in &*current_nodes {
            if node.status == Some(TaskStatus::InProgress) {
                if let Some(ref p_id) = node.parent_id {
                    if let Some(parent) = current_nodes.iter().find(|n| n.id == *p_id) {
                        if parent.status != Some(TaskStatus::InProgress) {
                            parents_to_update.push(p_id.clone());
                        }
                    }
                }
            }
        }
        for p_id in parents_to_update {
            if let Some(parent) = current_nodes.iter_mut().find(|n| n.id == p_id) {
                parent.status = Some(TaskStatus::InProgress);
                changed = true;
            }
        }

        // 2. Upward Completed & Failed propagation (when all children are finished)
        let mut parents_to_complete = Vec::new();
        let mut parents_to_fail = Vec::new();
        let mut parents_to_reopen = Vec::new();

        for parent_node in &*current_nodes {
            let children: Vec<&TaskNode> = current_nodes
                .iter()
                .filter(|n| n.parent_id.as_ref() == Some(&parent_node.id))
                .collect();

            if !children.is_empty() {
                let all_completed = children
                    .iter()
                    .all(|c| c.status == Some(TaskStatus::Completed));
                let all_finished = children.iter().all(|c| {
                    c.status == Some(TaskStatus::Completed) || c.status == Some(TaskStatus::Failed)
                });
                let any_failed = children
                    .iter()
                    .any(|c| c.status == Some(TaskStatus::Failed));
                // Work recorded under a parent makes that parent in-progress by
                // definition. Without this the `Pending` parent of an already
                // `Completed` child is unreachable by repair: validate_task_graph
                // rejects it, and because validation runs before the store write
                // the rejection discards the agent's update outright.
                let any_started = children
                    .iter()
                    .any(|c| c.status.unwrap_or(TaskStatus::Pending) != TaskStatus::Pending);
                let parent_status = parent_node.status.unwrap_or(TaskStatus::Pending);

                if all_completed && parent_node.status != Some(TaskStatus::Completed) {
                    parents_to_complete.push(parent_node.id.clone());
                } else if all_finished
                    && any_failed
                    && parent_node.status != Some(TaskStatus::Failed)
                {
                    parents_to_fail.push(parent_node.id.clone());
                } else if !all_finished && any_started && parent_status != TaskStatus::InProgress {
                    parents_to_reopen.push(parent_node.id.clone());
                }
            }
        }

        for p_id in parents_to_complete {
            if let Some(parent) = current_nodes.iter_mut().find(|n| n.id == p_id) {
                parent.status = Some(TaskStatus::Completed);
                changed = true;
            }
        }
        for p_id in parents_to_fail {
            if let Some(parent) = current_nodes.iter_mut().find(|n| n.id == p_id) {
                parent.status = Some(TaskStatus::Failed);
                changed = true;
            }
        }
        for p_id in parents_to_reopen {
            if let Some(parent) = current_nodes.iter_mut().find(|n| n.id == p_id) {
                parent.status = Some(TaskStatus::InProgress);
                changed = true;
            }
        }

        // 3. Downward InProgress propagation: if a parent is InProgress but has no active InProgress child,
        // automatically activate its first pending child to InProgress so a leaf sub-task is always active!
        let mut children_to_activate = Vec::new();
        for node in &*current_nodes {
            if node.status == Some(TaskStatus::InProgress) {
                let children: Vec<&TaskNode> = current_nodes
                    .iter()
                    .filter(|n| n.parent_id.as_ref() == Some(&node.id))
                    .collect();
                if !children.is_empty() {
                    let has_active_child = children
                        .iter()
                        .any(|c| c.status == Some(TaskStatus::InProgress));
                    if !has_active_child {
                        if let Some(first_pending) = children
                            .iter()
                            .find(|c| c.status == Some(TaskStatus::Pending))
                        {
                            children_to_activate.push(first_pending.id.clone());
                        }
                    }
                }
            }
        }
        for c_id in children_to_activate {
            if let Some(child) = current_nodes.iter_mut().find(|n| n.id == c_id) {
                child.status = Some(TaskStatus::InProgress);
                changed = true;
            }
        }

        // 4. Horizontal sibling auto-advance: when a node is auto-completed
        //    (all children finished), promote its next pending sibling under
        //    the same parent to InProgress. This eliminates wasted turns where
        //    the agent manually transitions between siblings.
        let mut siblings_to_advance = Vec::new();
        for node in &*current_nodes {
            if node.status == Some(TaskStatus::Completed) || node.status == Some(TaskStatus::Failed)
            {
                // Find siblings under the same parent, sorted by id
                let mut siblings: Vec<&TaskNode> = current_nodes
                    .iter()
                    .filter(|n| n.parent_id == node.parent_id && n.id != node.id)
                    .collect();
                siblings.sort_by(|a, b| compare_task_ids(&a.id, &b.id));

                // Check if any sibling is already InProgress
                let has_active_sibling = siblings
                    .iter()
                    .any(|s| s.status == Some(TaskStatus::InProgress));
                if !has_active_sibling {
                    // Find the next sibling that comes after this node and is Pending
                    if let Some(next_sib) = siblings.iter().find(|s| {
                        s.status == Some(TaskStatus::Pending)
                            && compare_task_ids(&s.id, &node.id) == Ordering::Greater
                    }) {
                        if !siblings_to_advance.contains(&next_sib.id) {
                            siblings_to_advance.push(next_sib.id.clone());
                        }
                    }
                }
            }
        }
        for s_id in siblings_to_advance {
            if let Some(next_sibling) = current_nodes.iter_mut().find(|n| n.id == s_id) {
                if next_sibling.status == Some(TaskStatus::Pending) {
                    next_sibling.status = Some(TaskStatus::InProgress);
                    changed = true;
                }
            }
        }

        // 5. Enforce single active sibling branch:
        // If an earlier sibling is InProgress (or has active children), all subsequent siblings
        // (and their descendant trees) must be Pending to prevent concurrent active phases and satisfy Sequential Control.
        let mut nodes_to_reset_to_pending = Vec::new();
        let unique_parents: std::collections::HashSet<Option<String>> =
            current_nodes.iter().map(|n| n.parent_id.clone()).collect();

        for pid in unique_parents {
            let mut siblings: Vec<&TaskNode> = current_nodes
                .iter()
                .filter(|n| n.parent_id == pid)
                .collect();
            siblings.sort_by(|a, b| compare_task_ids(&a.id, &b.id));

            let mut found_active = false;
            for sib in siblings {
                let is_active = sib.status == Some(TaskStatus::InProgress);
                if is_active {
                    if !found_active {
                        found_active = true;
                    } else if !subtree_has_finished_work(current_nodes, &sib.id) {
                        nodes_to_reset_to_pending.push(sib.id.clone());
                    }
                }
            }
        }

        while !nodes_to_reset_to_pending.is_empty() {
            let to_reset = std::mem::take(&mut nodes_to_reset_to_pending);
            for n in &mut *current_nodes {
                if to_reset.contains(&n.id) && n.status == Some(TaskStatus::InProgress) {
                    n.status = Some(TaskStatus::Pending);
                    changed = true;
                }
                if let Some(ref pid) = n.parent_id {
                    if to_reset.contains(pid) && n.status == Some(TaskStatus::InProgress) {
                        n.status = Some(TaskStatus::Pending);
                        nodes_to_reset_to_pending.push(n.id.clone());
                        changed = true;
                    }
                }
            }
        }
    }
}

/// Whether `id` or anything below it is already Completed/Failed.
///
/// Demoting such a branch to Pending produces a Pending parent with finished
/// children, which `validate_task_graph` rejects and no propagation rule can
/// repair — the loop then oscillates between promoting and demoting until its
/// iteration cap and settles on the rejected shape.
fn subtree_has_finished_work(nodes: &[TaskNode], id: &str) -> bool {
    let status = nodes
        .iter()
        .find(|n| n.id == id)
        .and_then(|n| n.status)
        .unwrap_or(TaskStatus::Pending);
    if status == TaskStatus::Completed || status == TaskStatus::Failed {
        return true;
    }
    nodes
        .iter()
        .filter(|n| n.parent_id.as_deref() == Some(id))
        .any(|child| subtree_has_finished_work(nodes, &child.id))
}
