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
}
