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
                let unclosed: Vec<&TaskNode> = children
                    .iter()
                    .copied()
                    .filter(|c| {
                        let st = c.status.unwrap_or(TaskStatus::Pending);
                        st != TaskStatus::Completed && st != TaskStatus::Failed
                    })
                    .collect();

                if !unclosed.is_empty() {
                    let first_unclosed = &unclosed[0];
                    let unclosed_list: Vec<String> = unclosed
                        .iter()
                        .map(|c| {
                            format!(
                                "  - [{}] {:?}: {}",
                                c.id,
                                c.status.unwrap_or(TaskStatus::Pending),
                                c.content.as_deref().unwrap_or("No description")
                            )
                        })
                        .collect();

                    return Err(format!(
                        "Error: TaskGraph Inconsistency. Parent task '{}' is marked as Completed, but its sub-task '{}' is currently '{:?}'.\n\
                         Parent task '{}' (\"{}\") has {} unclosed sub-task(s):\n{}\n\n\
                         HOW TO PROCEED:\n\
                         1. Execute and complete remaining sub-tasks in sequential order.\n\
                         2. If a sub-task is unnecessary or unneeded, update its status to 'failed' (or remove it) to mark it as skipped (-).\n\
                         3. If a sub-task is complex, break it down recursively into smaller sub-subtasks using operation: \"add\" (with parent_id: \"<subtask_id>\").",
                        node.id,
                        first_unclosed.id,
                        first_unclosed.status.unwrap_or(TaskStatus::Pending),
                        node.id,
                        node.content.as_deref().unwrap_or(""),
                        unclosed.len(),
                        unclosed_list.join("\n")
                    ));
                }
            } else if node_status == TaskStatus::Pending {
                for child in &children {
                    let child_status = child.status.unwrap_or(TaskStatus::Pending);
                    if child_status != TaskStatus::Pending {
                        return Err(format!(
                            "Error: TaskGraph Inconsistency. Parent task '{}' is marked as Pending, but its sub-task '{}' is currently '{:?}'. Sub-tasks cannot be InProgress or Completed while the parent task is Pending. Set parent task '{}' (\"{}\") to 'in_progress' first.",
                            node.id,
                            child.id,
                            child_status,
                            node.id,
                            node.content.as_deref().unwrap_or("")
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
            if sib_status == TaskStatus::InProgress {
                for prev_sibling in siblings.iter().take(idx) {
                    let prev_status = prev_sibling.status.unwrap_or(TaskStatus::Pending);
                    if prev_status != TaskStatus::Completed && prev_status != TaskStatus::Failed {
                        let parent_info = pid
                            .as_ref()
                            .and_then(|parent_id| {
                                nodes.iter().find(|n| &n.id == parent_id).map(|p| {
                                    format!(
                                        " (Parent Phase '{}': \"{}\")",
                                        p.id,
                                        p.content.as_deref().unwrap_or("")
                                    )
                                })
                            })
                            .unwrap_or_default();

                        return Err(format!(
                            "Error: Sequential Control Enforcement. You cannot start or complete task '{}' because a preceding sibling task '{}' is currently '{:?}'{}.\n\n\
                             HOW TO PROCEED:\n\
                             1. Execute preceding task '{}' (\"{}\") first.\n\
                             2. If the work for preceding task '{}' was already fulfilled in a subtask or earlier step, mark '{}' as 'completed' (or 'failed' to skip) in the same TaskGraph 'update_status' call.\n\
                             3. If task '{}' needs further breakdown, add sub-tasks under it using operation: \"add\" (with parent_id: \"{}\").",
                            sibling.id,
                            prev_sibling.id,
                            prev_status,
                            parent_info,
                            prev_sibling.id,
                            prev_sibling.content.as_deref().unwrap_or(""),
                            prev_sibling.id,
                            prev_sibling.id,
                            prev_sibling.id,
                            prev_sibling.id
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}
