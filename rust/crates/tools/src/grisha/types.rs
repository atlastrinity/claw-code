use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrishaErrorCode {
    /// Synthetic echo command simulating tool/diagnostic output.
    SimFauxExecution,
    /// Unverified task completion without actual tool inspection.
    SimUnverifiedCompletion,
    /// Fake metric or security state declaration via echo.
    SimFakeDiagnostic,
    /// Plain text imitating a tool call instead of sending real API tool_use.
    SimTextualToolCall,
    /// Target is remote client but command runs locally without transport.
    SimRemoteMismatch,
    /// Plan lacks sufficient recursive leaf breakdown.
    PlanMissingLeafNodes,
    /// Plan lacks verification steps.
    PlanMissingVerification,
    /// Plan contains recursive duplicates or cycles.
    PlanRecursiveDuplicate,
    /// Plan exceeds max allowed hierarchy depth.
    PlanMaxDepthExceeded,
    /// Phase transition attempted with incomplete prerequisites.
    PhaseTransitionIncomplete,
}

impl GrishaErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SimFauxExecution => "GRISHA_SIM_001",
            Self::SimUnverifiedCompletion => "GRISHA_SIM_002",
            Self::SimFakeDiagnostic => "GRISHA_SIM_003",
            Self::SimTextualToolCall => "GRISHA_SIM_004",
            Self::SimRemoteMismatch => "GRISHA_SIM_005",
            Self::PlanMissingLeafNodes => "GRISHA_PLAN_001",
            Self::PlanMissingVerification => "GRISHA_PLAN_002",
            Self::PlanRecursiveDuplicate => "GRISHA_PLAN_003",
            Self::PlanMaxDepthExceeded => "GRISHA_PLAN_004",
            Self::PhaseTransitionIncomplete => "GRISHA_PHASE_001",
        }
    }
}

impl std::fmt::Display for GrishaErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrishaExecutionError {
    pub code: GrishaErrorCode,
    pub description: String,
    pub remedy: String,
}

impl GrishaExecutionError {
    #[must_use]
    pub fn new(code: GrishaErrorCode, description: impl Into<String>, remedy: impl Into<String>) -> Self {
        Self {
            code,
            description: description.into(),
            remedy: remedy.into(),
        }
    }

    #[must_use]
    pub fn format_error(&self) -> String {
        format!(
            "ExecutionError [{}]: {}\n\nHOW TO PROCEED (GRISHA SUPERVISOR DIRECTIVE):\n{}",
            self.code, self.description, self.remedy
        )
    }
}

impl std::fmt::Display for GrishaExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format_error())
    }
}

impl std::error::Error for GrishaExecutionError {}
