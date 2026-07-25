use crate::task_graph::types::{TaskNode, TaskStatus};
use std::collections::HashSet;

pub fn validate_task_graph(nodes: &[TaskNode]) -> Result<(), String> {
    // 1. Parent-child state checks:
    for node in nodes {
        let node_status = node.status.unwrap_or(TaskStatus::Pending);
        let children: Vec<&TaskNode> = nodes
            .iter()
            .filter(|n| n.parent_id.as_ref() == Some(&node.id))
            .collect();

        if !children.is_empty() {
            if node_status == TaskStatus::Completed {
                for child in &children {
                    let child_status = child.status.unwrap_or(TaskStatus::Pending);
                    if child_status != TaskStatus::Completed && child_status != TaskStatus::Failed {
                        return Err(format!(
                            "Error: TaskGraph Inconsistency. Parent task '{}' is marked as Completed, but its sub-task '{}' is currently '{:?}'. All sub-tasks must be Completed or Failed before the parent task can be Completed.",
                            node.id, child.id, child_status
                        ));
                    }
                }
            } else if node_status == TaskStatus::Pending {
                for child in &children {
                    let child_status = child.status.unwrap_or(TaskStatus::Pending);
                    if child_status != TaskStatus::Pending {
                        return Err(format!(
                            "Error: TaskGraph Inconsistency. Parent task '{}' is marked as Pending, but its sub-task '{}' is currently '{:?}'. Sub-tasks cannot be InProgress or Completed while the parent task is Pending.",
                            node.id, child.id, child_status
                        ));
                    }
                }
            }
        }
    }

    // 2. Sequential sibling progression:
    let mut parent_ids: HashSet<Option<String>> = HashSet::new();
    for node in nodes {
        parent_ids.insert(node.parent_id.clone());
    }

    for pid in parent_ids {
        let mut siblings: Vec<&TaskNode> = nodes.iter().filter(|n| n.parent_id == pid).collect();

        siblings.sort_by(|a, b| {
            let a_parts: Vec<u32> = a.id.split('.').filter_map(|s| s.parse().ok()).collect();
            let b_parts: Vec<u32> = b.id.split('.').filter_map(|s| s.parse().ok()).collect();
            let cmp = a_parts.cmp(&b_parts);
            if cmp == std::cmp::Ordering::Equal {
                a.id.cmp(&b.id)
            } else {
                cmp
            }
        });

        for (idx, sibling) in siblings.iter().enumerate() {
            let sib_status = sibling.status.unwrap_or(TaskStatus::Pending);
            if sib_status == TaskStatus::InProgress || sib_status == TaskStatus::Completed {
                for prev_sibling in siblings.iter().take(idx) {
                    let prev_status = prev_sibling.status.unwrap_or(TaskStatus::Pending);
                    if prev_status != TaskStatus::Completed && prev_status != TaskStatus::Failed {
                        return Err(format!(
                            "Error: Sequential Control Enforcement. You cannot start or complete task '{}' because a preceding sibling task '{}' is currently '{:?}'. You must complete preceding tasks in sequential order.",
                            sibling.id, prev_sibling.id, prev_status
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}
