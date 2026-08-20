pub mod types;
pub mod detector;
pub mod plan_reviewer;

pub use types::{GrishaErrorCode, GrishaExecutionError};
pub use detector::GrishaSimulationDetector;
pub use plan_reviewer::{GrishaPlanReviewer, GrishaPlanReviewOutcome};

/// Central Grisha Supervisor interface.
pub struct GrishaSupervisor;

impl GrishaSupervisor {
    /// Perform simulation detection on bash commands.
    pub fn inspect_command(command: &str) -> Result<(), String> {
        GrishaSimulationDetector::check_bash_command(command).map_err(|e| e.format_error())
    }

    /// Perform plan review and enhancement on task nodes.
    pub fn review_plan(
        nodes: &[crate::task_graph::types::TaskNode],
        existing_nodes: &[crate::task_graph::types::TaskNode],
    ) -> Result<GrishaPlanReviewOutcome, String> {
        GrishaPlanReviewer::review_and_enhance(nodes, existing_nodes).map_err(|e| e.format_error())
    }

    /// Validate phase transitions: when a root phase N is being set to InProgress,
    /// ensure all children of root phase N-1 are Completed or Failed.
    pub fn validate_phase_transition(
        update_nodes: &[crate::task_graph::types::TaskNode],
        current_nodes: &[crate::task_graph::types::TaskNode],
    ) -> Result<(), String> {
        use crate::task_graph::types::TaskStatus;

        for update in update_nodes {
            // Only check root-level nodes (no dots in id) being set to InProgress
            if update.id.contains('.') {
                continue;
            }
            if update.status != Some(TaskStatus::InProgress) {
                continue;
            }

            // Parse root phase number
            let phase_num: u32 = match update.id.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if phase_num <= 1 {
                continue; // Phase 1 has no predecessor
            }

            let prev_phase_id = (phase_num - 1).to_string();

            // Find previous root phase
            let prev_phase = current_nodes.iter().find(|n| n.id == prev_phase_id);
            if prev_phase.is_none() {
                continue; // No previous phase found, skip validation
            }

            // Collect all descendants of the previous phase
            let prev_children: Vec<&crate::task_graph::types::TaskNode> = current_nodes
                .iter()
                .filter(|n| {
                    n.id.starts_with(&format!("{}.", prev_phase_id))
                        || n.id == prev_phase_id
                })
                .collect();

            let incomplete: Vec<String> = prev_children
                .iter()
                .filter(|n| {
                    n.status != Some(TaskStatus::Completed)
                        && n.status != Some(TaskStatus::Failed)
                })
                .map(|n| format!("'{}' ({:?})", n.id, n.status.unwrap_or(TaskStatus::Pending)))
                .collect();

            if !incomplete.is_empty() {
                return Err(GrishaExecutionError::new(
                    GrishaErrorCode::PhaseTransitionIncomplete,
                    format!(
                        "🛡️ Grisha Phase Transition Gate: Cannot start Phase {} while Phase {} still has {} incomplete task(s): {}",
                        phase_num, prev_phase_id, incomplete.len(), incomplete.join(", ")
                    ),
                    format!(
                        "Complete or mark as failed ALL remaining tasks in Phase {} before transitioning to Phase {}. Use TaskGraph(operation: 'update_status') to resolve the pending tasks first.",
                        prev_phase_id, phase_num
                    ),
                ).format_error());
            }
        }

        Ok(())
    }
}
