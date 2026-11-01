use super::types::{GrishaErrorCode, GrishaExecutionError};
use crate::task_graph::types::{parent_id_from_task_id, TaskNode, TaskStatus};

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
        let mut enriched = nodes.to_vec();
        let mut remarks = Vec::new();

        if enriched.is_empty() {
            return Ok(GrishaPlanReviewOutcome {
                is_approved: true,
                enriched_nodes: enriched,
                remarks: vec!["Empty node list provided; no modifications needed.".to_string()],
            });
        }

        // 1. Language check: ensure titles are primarily in English while permitting Ukrainian terms / user queries
        for node in &enriched {
            if let Some(content) = &node.content {
                let unquoted = strip_quotes_for_lang_check(content);
                if unquoted
                    .chars()
                    .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
                {
                    remarks.push(format!(
                        "Grisha Note: Task '{}' contains Ukrainian terms as requested by the user. Proceeding with execution.",
                        node.id
                    ));
                }
            }
        }

        // 2. Ensure parent nodes exist (either in submitted list or in existing
        // nodes) and actually synthesize the missing ones — a remark alone used
        // to promise an auto-create that only happened later in operations.rs.
        let missing_parents: Vec<TaskNode> = nodes
            .iter()
            .filter_map(|node| {
                let pid = node.parent_id.clone()?;
                let parent_exists = enriched.iter().any(|n| n.id == pid)
                    || existing_nodes.iter().any(|n| n.id == pid);
                if parent_exists {
                    return None;
                }
                remarks.push(format!(
                    "Auto-created missing parent node '{}' for child '{}'.",
                    pid, node.id
                ));
                let content = if pid.contains('.') {
                    format!("Task {pid}")
                } else {
                    format!("Phase {pid}")
                };
                Some(TaskNode {
                    id: pid.clone(),
                    parent_id: parent_id_from_task_id(&pid),
                    content: Some(content),
                    status: Some(TaskStatus::Pending),
                })
            })
            .collect();
        for parent in missing_parents {
            if !enriched.iter().any(|n| n.id == parent.id) {
                enriched.push(parent);
            }
        }

        // 3. Recursive leaf breakdown review: check if phases have sub-tasks
        for node in &enriched {
            let depth = node.id.split('.').count();
            // If top-level node (depth == 1) and no children in plan
            if depth == 1 {
                let has_children_in_input = enriched
                    .iter()
                    .any(|n| n.parent_id.as_deref() == Some(&node.id));
                let has_children_in_stored = existing_nodes
                    .iter()
                    .any(|n| n.parent_id.as_deref() == Some(&node.id));
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
        //
        // `retried_abandoned` collects the clones that are really second attempts:
        // a task repeated against one already marked 'failed'. Those are not
        // rejected below — see the redirect that explains why.
        let mut retried_abandoned: Vec<(String, String)> = Vec::new();
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
                                if parent.status == Some(TaskStatus::Failed) {
                                    if !retried_abandoned.iter().any(|(_, t)| t == &parent.id) {
                                        retried_abandoned
                                            .push((node.id.clone(), parent.id.clone()));
                                    }
                                    break;
                                }
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

        // 5.5 Graph-wide duplicate check. Comparing only inside one parent left
        // the escape hatch a live run actually used: the same diagnostic subtree
        // re-created verbatim under a cousin parent. The comparison key is the
        // sorted set of significant words, so case, word order and filler
        // articles no longer hide a clone. Verification steps are exempt —
        // "run the tests" after every phase is not the same work twice.
        fn clone_signature(title: &str) -> String {
            let mut words: Vec<String> = title
                .to_lowercase()
                .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_'))
                .filter(|w| {
                    w.chars().any(|c| c.is_ascii_digit())
                        || (w.chars().count() > 3 && !STOPWORDS.contains(w))
                })
                .map(str::to_owned)
                .collect();
            words.sort();
            words.dedup();
            words.join("+")
        }

        const STOPWORDS: [&str; 6] = ["with", "from", "that", "this", "into", "your"];

        fn is_verification(title: &str) -> bool {
            let lc = title.to_lowercase();
            ["verif", "test", "check", "validat", "review"]
                .iter()
                .any(|marker| lc.contains(marker))
        }

        let mut occupied: std::collections::BTreeMap<String, &TaskNode> =
            std::collections::BTreeMap::new();
        for node in existing_nodes.iter().chain(enriched.iter()) {
            let Some(content) = node.content.as_deref() else {
                continue;
            };
            if content.trim().is_empty() || is_verification(content) {
                continue;
            }
            let signature = clone_signature(content);
            if signature.is_empty() {
                continue;
            }
            occupied.entry(signature).or_insert(node);
        }

        for node in nodes {
            let Some(content) = &node.content else {
                continue;
            };
            if content.trim().is_empty() || is_verification(content) {
                continue;
            }
            let signature = clone_signature(content);
            let Some(twin) = occupied.get(&signature).copied() else {
                continue;
            };
            if twin.id == node.id {
                continue;
            }
            if twin.status == Some(TaskStatus::Failed) {
                if !retried_abandoned.iter().any(|(_, t)| t == &twin.id) {
                    retried_abandoned.push((node.id.clone(), twin.id.clone()));
                }
                continue;
            }
            let same_parent = twin.parent_id == node.parent_id;
            return Err(GrishaExecutionError::new(
                GrishaErrorCode::PlanRecursiveDuplicate,
                format!(
                    "Task '{}' (\"{}\") is a clone of {}task '{}' (\"{}\") already in the graph.",
                    node.id,
                    content,
                    if same_parent { "sibling " } else { "another " },
                    twin.id,
                    twin.content.as_deref().unwrap_or("")
                ),
                if same_parent {
                    "Do not add the same work twice under one parent. Execute under the existing task, or give the new task a genuinely distinct title."
                } else {
                    "Re-planning the same step under a different parent does not change the outcome — the previous attempt failed on the world, not on the plan. Execute an alternative action directly, or mark the existing task 'failed' with the reason and move to the next sibling."
                },
            ));
        }

        // A clone of an abandoned task is not a planning mistake — it is the agent
        // asking for a second attempt at work that failed against the world.
        // Refusing it used to strand a run in both directions: to be allowed to
        // execute the step at all it had to be worded differently, which this same
        // gate then accepted as brand-new work, so the graph grew a synonym while
        // the action stayed undone. The retry therefore reopens the original node
        // and the duplicate is never added.
        for (clone_id, twin_id) in retried_abandoned {
            enriched.retain(|n| n.id != clone_id);
            match enriched.iter_mut().find(|n| n.id == twin_id) {
                Some(twin_node) => twin_node.status = Some(TaskStatus::InProgress),
                None => enriched.push(TaskNode {
                    id: twin_id.clone(),
                    parent_id: existing_nodes
                        .iter()
                        .find(|n| n.id == twin_id)
                        .and_then(|n| n.parent_id.clone()),
                    content: None,
                    status: Some(TaskStatus::InProgress),
                }),
            }
            remarks.push(format!(
                "Grisha Retry Redirection: '{}' repeats abandoned task '{}'. '{}' is reopened as 'in_progress' and the duplicate was not added. Execute the step under '{}' with a different command, path or parameter than the attempt that failed, and say in the call what you changed.",
                clone_id, twin_id, twin_id, twin_id
            ));
        }

        // 6. Verification advisory: every root phase should end with a check
        // that its work actually landed (tests, compile, read-back).
        let merged: Vec<&TaskNode> = existing_nodes.iter().chain(enriched.iter()).collect();
        for node in nodes {
            if node.depth() != 1 {
                continue;
            }
            let family: Vec<&TaskNode> = merged
                .iter()
                .filter(|n| {
                    n.id == node.id || crate::task_graph::types::is_descendant_id(&n.id, &node.id)
                })
                .copied()
                .collect();
            let has_verification = family.iter().any(|n| {
                n.content
                    .as_deref()
                    .map(|c| {
                        let lc = c.to_lowercase();
                        lc.contains("verif")
                            || lc.contains("test")
                            || lc.contains("check")
                            || lc.contains("validate")
                            || lc.contains("review")
                            || lc.contains("compile")
                    })
                    .unwrap_or(false)
            });
            if !has_verification && !family.is_empty() {
                remarks.push(format!(
                    "Grisha Note [{}]: Phase '{}' (\"{}\") has no verification step. Consider appending a final sub-task such as \"{}.{}\" (\"Verify: run tests/build and confirm the outcome\") so the phase cannot be closed on unverified work.",
                    GrishaErrorCode::PlanMissingVerification,
                    node.id,
                    node.content.as_deref().unwrap_or(""),
                    node.id,
                    family.len() + 1
                ));
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
                if !in_single && !in_double && !in_guillemet && !in_fancy_single && !in_fancy_double
                {
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
    fn test_allows_cyrillic_task_content_with_advisory() {
        let nodes = vec![TaskNode {
            id: "1".to_string(),
            parent_id: None,
            content: Some("Проаналізувати код".to_string()),
            status: Some(TaskStatus::Pending),
        }];
        let res = GrishaPlanReviewer::review_and_enhance(&nodes, &[]);
        assert!(res.is_ok());
        let outcome = res.unwrap();
        assert!(outcome.is_approved);
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
        assert_eq!(
            res.unwrap_err().code,
            GrishaErrorCode::PlanRecursiveDuplicate
        );
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
