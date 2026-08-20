use crate::task_graph::types::TaskNode;
use super::types::{GrishaErrorCode, GrishaExecutionError};

/// Outcome of Grisha's plan inspection and review.
#[derive(Debug, Clone)]
pub struct GrishaPlanReviewOutcome {
    pub is_approved: bool,
    pub enriched_nodes: Vec<TaskNode>,
    pub remarks: Vec<String>,
}

pub struct GrishaPlanReviewer;

impl GrishaPlanReviewer {
    /// Review and optionally enrich a list of task nodes before they are added to the task graph.
    pub fn review_and_enhance(
        nodes: &[TaskNode],
        existing_nodes: &[TaskNode],
    ) -> Result<GrishaPlanReviewOutcome, GrishaExecutionError> {
        let enriched = nodes.to_vec();
        let mut remarks = Vec::new();

        if enriched.is_empty() {
            return Ok(GrishaPlanReviewOutcome {
                is_approved: true,
                enriched_nodes: enriched,
                remarks: vec!["Empty node list provided; no modifications needed.".to_string()],
            });
        }

        // 1. Language check: ensure titles are primarily in English (allow Cyrillic literals within quotes)
        for node in &enriched {
            if let Some(content) = &node.content {
                let unquoted = strip_quotes_for_lang_check(content);
                if unquoted.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
                    return Err(GrishaExecutionError::new(
                        GrishaErrorCode::PlanMissingLeafNodes,
                        format!("Task '{}' has Ukrainian/Cyrillic content: \"{}\".", node.id, content),
                        "All task titles, descriptions, and IDs in TaskGraph MUST be written strictly in English (except for quoted user search literals).",
                    ));
                }
            }
        }

        // 2. Ensure parent nodes exist (either in submitted list or in existing nodes)
        for node in &enriched {
            if let Some(ref pid) = node.parent_id {
                let parent_exists_in_input = enriched.iter().any(|n| n.id == *pid);
                let parent_exists_in_stored = existing_nodes.iter().any(|n| n.id == *pid);
                if !parent_exists_in_input && !parent_exists_in_stored {
                    // Auto-create missing parent node with default title
                    remarks.push(format!("Auto-created missing parent node '{}' for child '{}'.", pid, node.id));
                }
            }
        }

        // 3. Recursive leaf breakdown review: check if phases have sub-tasks
        for node in &enriched {
            let depth = node.id.split('.').count();
            // If top-level node (depth == 1) and no children in plan
            if depth == 1 {
                let has_children_in_input = enriched.iter().any(|n| n.parent_id.as_deref() == Some(&node.id));
                let has_children_in_stored = existing_nodes.iter().any(|n| n.parent_id.as_deref() == Some(&node.id));
                if !has_children_in_input && !has_children_in_stored {
                    remarks.push(format!(
                        "Grisha Advisory: Phase '{}' has no leaf sub-tasks. You must decompose it recursively (e.g. '{}.1') using TaskGraph operation 'add' before executing mutating actions under it.",
                        node.id, node.id
                    ));
                }
            }
        }

        // 4. Max recursion depth check: prevent pathological infinite task tree recursion (depth > 5)
        for node in &enriched {
            let depth = node.id.split('.').count();
            if depth > 5 {
                return Err(GrishaExecutionError::new(
                    GrishaErrorCode::PlanMaxDepthExceeded,
                    format!("Task '{}' exceeds maximum allowed hierarchy depth (depth {} > 5).", node.id, depth),
                    "Do NOT recursively decompose tasks beyond 5 levels. Execute the action directly under the existing leaf task.",
                ));
            }
        }

        // 5. Duplicate child & ancestor cycle check: prevent recreating identical tasks under parent
        for node in &enriched {
            if let Some(node_content) = &node.content {
                let norm_node_content = node_content.trim().to_lowercase();
                if norm_node_content.is_empty() {
                    continue;
                }

                // Check ancestor chain in input + existing nodes
                let mut current_parent_id = node.parent_id.clone();
                while let Some(pid) = current_parent_id {
                    let parent_node = enriched
                        .iter()
                        .find(|n| n.id == pid)
                        .or_else(|| existing_nodes.iter().find(|n| n.id == pid));

                    if let Some(parent) = parent_node {
                        if let Some(parent_content) = &parent.content {
                            let norm_parent_content = parent_content.trim().to_lowercase();
                            if norm_node_content == norm_parent_content {
                                return Err(GrishaExecutionError::new(
                                    GrishaErrorCode::PlanRecursiveDuplicate,
                                    format!(
                                        "Task '{}' (\"{}\") is a recursive duplicate of ancestor task '{}' (\"{}\").",
                                        node.id, node_content, parent.id, parent_content
                                    ),
                                    "Decomposing a task into a child with an identical description is not permitted. Execute the necessary actions directly instead of creating duplicate subtasks.",
                                ));
                            }
                        }
                        current_parent_id = parent.parent_id.clone();
                    } else {
                        break;
                    }
                }
            }
        }

        Ok(GrishaPlanReviewOutcome {
            is_approved: true,
            enriched_nodes: enriched,
            remarks,
        })
    }
}

fn strip_quotes_for_lang_check(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut in_guillemet = false;
    let mut in_fancy_single = false;
    let mut in_fancy_double = false;

    for c in s.chars() {
        match c {
            '\'' => in_single = !in_single,
            '"' => in_double = !in_double,
            '«' => in_guillemet = true,
            '»' => in_guillemet = false,
            '‘' => in_fancy_single = true,
            '’' => in_fancy_single = false,
            '“' => in_fancy_double = true,
            '”' => in_fancy_double = false,
            _ => {
                if !in_single && !in_double && !in_guillemet && !in_fancy_single && !in_fancy_double {
                    result.push(c);
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_graph::types::TaskStatus;

    #[test]
    fn test_allows_cyrillic_search_literal_in_quotes() {
        let nodes = vec![TaskNode {
            id: "1.4".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Search for 'фільми онлайн' on Google".to_string()),
            status: Some(TaskStatus::Pending),
        }];
        let res = GrishaPlanReviewer::review_and_enhance(&nodes, &[]);
        assert!(res.is_ok());
    }

    #[test]
    fn test_rejects_cyrillic_task_content() {
        let nodes = vec![TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Проаналізувати код".to_string()),
            status: Some(TaskStatus::Pending),
        }];
        let res = GrishaPlanReviewer::review_and_enhance(&nodes, &[]);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, GrishaErrorCode::PlanMissingLeafNodes);
    }

    #[test]
    fn test_rejects_max_depth_exceeded() {
        let nodes = vec![TaskNode {
            id: "1.2.2.3.3.1".to_string(),
            parent_id: Some("1.2.2.3.3".to_string()),
            content: Some("Apply fix".to_string()),
            status: Some(TaskStatus::Pending),
        }];
        let res = GrishaPlanReviewer::review_and_enhance(&nodes, &[]);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, GrishaErrorCode::PlanMaxDepthExceeded);
    }

    #[test]
    fn test_rejects_recursive_duplicate_of_ancestor() {
        let existing = vec![TaskNode {
            id: "1.2".to_string(),
            parent_id: Some("1".to_string()),
            content: Some("Fix SpeedTrafficService.swift".to_string()),
            status: Some(TaskStatus::InProgress),
        }];
        let nodes = vec![TaskNode {
            id: "1.2.1".to_string(),
            parent_id: Some("1.2".to_string()),
            content: Some("Fix SpeedTrafficService.swift".to_string()),
            status: Some(TaskStatus::Pending),
        }];
        let res = GrishaPlanReviewer::review_and_enhance(&nodes, &existing);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code, GrishaErrorCode::PlanRecursiveDuplicate);
    }

    #[test]
    fn test_approves_and_enriches_valid_plan() {
        let nodes = vec![TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Analyze system components".to_string()),
            status: Some(TaskStatus::Pending),
        }];
        let res = GrishaPlanReviewer::review_and_enhance(&nodes, &[]).unwrap();
        assert!(res.is_approved);
        assert!(!res.remarks.is_empty());
        assert!(res.remarks[0].contains("Grisha Advisory"));
    }
}
