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

        // 4b. Meta-task check: a step whose object is the tracker itself.
        //
        // "Verify the current leaf task is in progress" and "Mark the current leaf
        // task as completed" were both planned, both completed, and both produced
        // nothing. Bookkeeping is what the tool does on its own; a plan node spent
        // on it reads as progress and pushes the real work one leaf further away.
        fn is_tracker_bookkeeping(title: &str) -> bool {
            let lc = title.to_lowercase();
            let names_tracker = [
                "leaf task",
                "current leaf",
                "active task",
                "active leaf",
                "task graph",
                "taskgraph",
                "task.md",
            ]
            .iter()
            .any(|marker| lc.contains(marker));
            let bookkeeping = [
                "verify", "checked", "checking", "confirm", "mark", "update", "set ", "ensure",
            ]
            .iter()
            .any(|verb| lc.contains(verb));
            names_tracker && bookkeeping
        }

        for node in &enriched {
            if let Some(content) = node.content.as_deref() {
                if is_tracker_bookkeeping(content) {
                    return Err(GrishaExecutionError::new(
                        GrishaErrorCode::PlanMetaTask,
                        format!(
                            "Task '{}' (\"{}\") is bookkeeping about the task tracker, not work on \
                             the user's goal.",
                            node.id, content
                        ),
                        "Do not plan steps whose object is the task graph itself — status updates \
                         are what the tool does by itself. Name the artifact the step produces \
                         (a file, a build, a running command) and plan that instead.",
                    ));
                }
            }
        }

        // 4c. Directive echo: a leaf whose title is a line of system guidance
        // rather than a description of work.
        //
        // A live run filled its plan with "Reach the same goal without this call
        // at all: …", "Apply fix or alternative: …" and "Determine the
        // precondition: …" — the bifurcation and give-up texts pasted straight
        // into `add`. The damage shows up later: a title is what the verification
        // gate reads as the promise the step must keep, and a promise copied from
        // an instruction names no artifact and no command that could satisfy it.
        // The leaf can never close, the siblings behind it can never start, and
        // the run circles until every gate is refusing. Refusing the title at the
        // gate that creates it costs one turn; letting it in costs the session.
        fn is_directive_echo(title: &str) -> bool {
            let lc = title.to_lowercase();
            [
                "reach the same goal",
                "without this call",
                "determine the precondition",
                "apply fix or alternative",
                "mark the blocked approach",
                "split the orbit",
                "the precondition as the thing",
            ]
            .iter()
            .any(|marker| lc.contains(marker))
        }

        for node in &enriched {
            if let Some(content) = node.content.as_deref() {
                if is_directive_echo(content) {
                    return Err(GrishaExecutionError::new(
                        GrishaErrorCode::PlanDirectiveEcho,
                        format!(
                            "Task '{}' (\"{}\") is a line copied from a system directive, not a \
                             step of work.",
                            node.id, content
                        ),
                        "Name the step by what it produces — the file that will exist, the \
                         command that will exit 0, the behaviour a user can see. A leaf titled \
                         with the words of an instruction is read back later as the promise the \
                         step must keep, and nothing in the workspace can satisfy it.",
                    ));
                }
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

        // The words that identify *what* a check looks at, with retry wording
        // and verb tense removed. "Verify the brightness engine" and "Retry
        // verification of the brightness engine after building" describe one
        // check, so they must land on one task.
        fn verification_subject(title: &str) -> std::collections::BTreeSet<String> {
            const RETRY_MARKERS: [&str; 10] = [
                "retry", "again", "repeat", "another", "second", "attempt", "rerun", "redo",
                "after", "before",
            ];
            title
                .to_lowercase()
                .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_'))
                .filter(|w| !w.is_empty() && w.chars().count() > 3 && !STOPWORDS.contains(w))
                .filter(|w| !RETRY_MARKERS.contains(w))
                .map(|w| {
                    ["verif", "check", "test", "validat", "review", "inspect"]
                        .iter()
                        .find(|stem| w.starts_with(*stem))
                        .map_or_else(|| w.to_owned(), |stem| (*stem).to_owned())
                })
                .collect()
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

        // A verification step is real work every time it runs, so it is exempt
        // from the clone gate above. A *retry* of a verification that already
        // failed is different: wording it "Retry verification of X" let a live
        // run add two siblings that each repeated the check the previous one had
        // just failed, and the graph grew while the evidence did not. A retry
        // that restates a failed check reopens it, exactly as a verbatim clone
        // reopens abandoned work.
        for node in nodes {
            let Some(content) = &node.content else {
                continue;
            };
            if content.trim().is_empty() || !is_verification(content) {
                continue;
            }
            let subject = verification_subject(content);
            if subject.is_empty() {
                continue;
            }
            let twin = existing_nodes.iter().chain(enriched.iter()).find(|other| {
                other.id != node.id
                    && other.status == Some(TaskStatus::Failed)
                    && other.content.as_deref().is_some_and(|old| {
                        is_verification(old) && {
                            let old_subject = verification_subject(old);
                            // One shared word ("engine") is a topic, not a
                            // repeat; an empty subject would match everything.
                            old_subject.len() >= 2
                                && old_subject.iter().all(|word| subject.contains(word))
                        }
                    })
            });
            let Some(twin) = twin else {
                continue;
            };
            if !retried_abandoned.iter().any(|(_, id)| id == &twin.id) {
                retried_abandoned.push((node.id.clone(), twin.id.clone()));
            }
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
            let pool: Vec<TaskNode> = existing_nodes
                .iter()
                .chain(enriched.iter())
                .cloned()
                .collect();
            let reopen_id = match reopen_target(&twin_id, &pool) {
                Some(id) => id,
                None => {
                    remarks.push(format!(
                        "Grisha Retry Redirection: '{}' repeats task '{}', and every step under it is already recorded as done, so there is nothing to reopen. Nothing was added. Say which artifact is actually missing and add it as a new sub-task of '{}' with a title that branch does not already state.",
                        clone_id, twin_id, twin_id
                    ));
                    continue;
                }
            };
            match enriched.iter_mut().find(|n| n.id == reopen_id) {
                Some(node) => node.status = Some(TaskStatus::InProgress),
                None => enriched.push(TaskNode {
                    id: reopen_id.clone(),
                    parent_id: pool
                        .iter()
                        .find(|n| n.id == reopen_id)
                        .and_then(|n| n.parent_id.clone()),
                    content: None,
                    status: Some(TaskStatus::InProgress),
                }),
            }
            let step_moved = if reopen_id == twin_id {
                String::new()
            } else {
                format!(
                    " (the retry restarts at abandoned step '{}', not at the phase head)",
                    reopen_id
                )
            };
            remarks.push(format!(
                "Grisha Retry Redirection: '{}' repeats failed task '{}'. '{}' is reopened as 'in_progress'{} and the duplicate was not added. Execute the step under '{}' with a different command, path or parameter than the attempt that failed, and say in the call what you changed.",
                clone_id, twin_id, reopen_id, step_moved, reopen_id
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

/// The node a redirected retry restarts from: the abandoned twin when it is a
/// leaf, otherwise the shallowest unfinished step still hanging under it.
///
/// Reopening the phase head does not survive the store write. Status propagation
/// fails a parent whose every child is terminal, so the redirect used to reopen a
/// node the very same call closed again: the remark promised `in_progress`, the
/// graph stayed all-terminal, and the agent's next mutating action was refused
/// with "all tasks are completed or failed" — a loop it escaped only by abandoning
/// the branch.
///
/// `None` means the branch has no unfinished step left to hand back, so the caller
/// must not claim a reopen happened.
fn reopen_target(twin_id: &str, pool: &[TaskNode]) -> Option<String> {
    let children_of = |id: &str| -> Vec<TaskNode> {
        pool.iter()
            .filter(|n| n.parent_id.as_deref() == Some(id))
            .cloned()
            .collect()
    };
    if children_of(twin_id).is_empty() {
        return Some(twin_id.to_string());
    }
    let mut frontier: Vec<TaskNode> = children_of(twin_id);
    let mut steps: Vec<String> = Vec::new();
    let mut depth = 0;
    while !frontier.is_empty() && depth < 5 {
        depth += 1;
        let mut next = Vec::new();
        for node in frontier {
            let kids = children_of(&node.id);
            if kids.is_empty() {
                if node.status != Some(TaskStatus::Completed) {
                    steps.push(node.id);
                }
            } else {
                next.extend(kids);
            }
        }
        frontier = next;
    }
    steps.sort_unstable_by(|a, b| crate::task_graph::types::compare_task_ids(a, b));
    steps.into_iter().next()
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

    /// A leaf named after a line of system guidance can never be closed: its
    /// title promises no artifact, so the verification gate has nothing to look
    /// for and the run circles behind a refusal it cannot answer.
    #[test]
    fn a_title_copied_from_a_system_directive_is_not_a_step() {
        for title in [
            "Reach the same goal without this call at all: use swift build directly",
            "Apply fix or alternative: modify the Package.swift file",
            "Determine the precondition: the swift build command must succeed",
        ] {
            let res = GrishaPlanReviewer::review_and_enhance(
                &[TaskNode {
                    id: "7.2.2.1".to_string(),
                    parent_id: Some("7.2.2".to_string()),
                    content: Some(title.to_string()),
                    status: Some(TaskStatus::Pending),
                }],
                &[],
            );
            let error = res.expect_err("a directive line is not a planned step");
            assert_eq!(error.code, GrishaErrorCode::PlanDirectiveEcho, "{title}");
        }

        // Real work, including a feature whose own words sit next to the
        // directive vocabulary, still plans.
        for title in [
            "Add colour inversion to the display profile",
            "Write the build script that runs swift build and signs the app",
        ] {
            let res = GrishaPlanReviewer::review_and_enhance(
                &[TaskNode {
                    id: "7.2.2.1".to_string(),
                    parent_id: Some("7.2.2".to_string()),
                    content: Some(title.to_string()),
                    status: Some(TaskStatus::Pending),
                }],
                &[],
            );
            assert!(res.is_ok(), "{title} is a step of work: {:?}", res.err());
        }
    }

    #[test]
    fn a_step_whose_object_is_the_tracker_is_not_a_step() {
        for title in [
            "Verify the current leaf task is in progress",
            "Mark the current leaf task as completed",
        ] {
            let res = GrishaPlanReviewer::review_and_enhance(
                &[TaskNode {
                    id: "7.2.2.1".to_string(),
                    parent_id: Some("7.2.2".to_string()),
                    content: Some(title.to_string()),
                    status: Some(TaskStatus::Pending),
                }],
                &[],
            );
            let error = res.expect_err("a step whose object is the tracker must be rejected");
            assert_eq!(error.code, GrishaErrorCode::PlanMetaTask, "{title}");
        }

        // A real check that happens to be the last step of a phase still plans.
        let res = GrishaPlanReviewer::review_and_enhance(
            &[TaskNode {
                id: "7.2.2.1".to_string(),
                parent_id: Some("7.2.2".to_string()),
                content: Some(
                    "Verify the app writes brightness changes to the log file".to_string(),
                ),
                status: Some(TaskStatus::Pending),
            }],
            &[],
        );
        assert!(res.is_ok());
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
