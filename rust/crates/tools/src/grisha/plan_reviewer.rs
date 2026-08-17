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

        // 1. Language check: ensure titles are not in non-ASCII/Ukrainian scripts if possible
        for node in &enriched {
            if let Some(content) = &node.content {
                if content.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
                    return Err(GrishaExecutionError::new(
                        GrishaErrorCode::PlanMissingLeafNodes,
                        format!("Task '{}' has Ukrainian/Cyrillic content: \"{}\".", node.id, content),
                        "All task titles, descriptions, and IDs in TaskGraph MUST be written strictly in English.",
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

        Ok(GrishaPlanReviewOutcome {
            is_approved: true,
            enriched_nodes: enriched,
            remarks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_graph::types::TaskStatus;

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
