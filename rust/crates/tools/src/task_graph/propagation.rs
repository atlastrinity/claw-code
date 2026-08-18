use crate::task_graph::types::{TaskNode, TaskStatus};

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

        // 1. Upward InProgress propagation
        let mut parents_to_update = Vec::new();
        for node in &*current_nodes {
            if node.status == Some(TaskStatus::InProgress)
                || node.status == Some(TaskStatus::Completed)
            {
                if let Some(ref p_id) = node.parent_id {
                    if let Some(parent) = current_nodes.iter().find(|n| n.id == *p_id) {
                        if parent.status != Some(TaskStatus::InProgress)
                            && parent.status != Some(TaskStatus::Completed)
                        {
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

                if all_completed && parent_node.status != Some(TaskStatus::Completed) {
                    parents_to_complete.push(parent_node.id.clone());
                } else if all_finished
                    && any_failed
                    && parent_node.status != Some(TaskStatus::Failed)
                {
                    parents_to_fail.push(parent_node.id.clone());
                } else if !all_finished
                    && (parent_node.status == Some(TaskStatus::Completed)
                        || parent_node.status == Some(TaskStatus::Failed))
                {
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
                        if let Some(first_pending) =
                            children.iter().find(|c| c.status == Some(TaskStatus::Pending))
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
            if node.status == Some(TaskStatus::Completed) {
                // Find siblings under the same parent, sorted by id
                let mut siblings: Vec<&TaskNode> = current_nodes
                    .iter()
                    .filter(|n| n.parent_id == node.parent_id && n.id != node.id)
                    .collect();
                siblings.sort_by(|a, b| {
                    let a_parts: Vec<u32> = a.id.split('.').filter_map(|s| s.parse().ok()).collect();
                    let b_parts: Vec<u32> = b.id.split('.').filter_map(|s| s.parse().ok()).collect();
                    a_parts.cmp(&b_parts)
                });

                // Check if any sibling is already InProgress
                let has_active_sibling = siblings.iter().any(|s| s.status == Some(TaskStatus::InProgress));
                if !has_active_sibling {
                    // Find the next sibling that comes after this node and is Pending
                    let node_parts: Vec<u32> = node.id.split('.').filter_map(|s| s.parse().ok()).collect();
                    if let Some(next_sib) = siblings.iter().find(|s| {
                        s.status == Some(TaskStatus::Pending) && {
                            let s_parts: Vec<u32> = s.id.split('.').filter_map(|p| p.parse().ok()).collect();
                            s_parts > node_parts
                        }
                    }) {
                        if !siblings_to_advance.contains(&next_sib.id) {
                            siblings_to_advance.push(next_sib.id.clone());
                        }
                    }
                }
            }
        }
        for sib_id in siblings_to_advance {
            if let Some(sib) = current_nodes.iter_mut().find(|n| n.id == sib_id) {
                sib.status = Some(TaskStatus::InProgress);
                changed = true;
            }
        }
    }
}
