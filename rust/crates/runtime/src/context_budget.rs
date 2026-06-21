use serde::{Deserialize, Serialize};

/// Represents dynamic limits and budgets applied globally across tools,
/// scaled according to the selected model's context window size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudget {
    pub max_read_file_lines: usize,
    pub max_glob_files: usize,
    pub max_bash_output_bytes: usize,
}

impl ContextBudget {
    /// Calculate operational limits based on the available context window tokens.
    ///
    /// - `max_read_file_lines`: Number of lines allowed when reading files without a strict limit.
    /// - `max_glob_files`: Maximum number of files to return from a wildcard search.
    /// - `max_bash_output_bytes`: Maximum allowed bash stdout/stderr size.
    pub fn from_context_window(window_tokens: u32) -> Self {
        Self {
            max_read_file_lines: ((window_tokens as usize) / 100).clamp(300, 3000),
            max_glob_files: ((window_tokens as usize) / 500).clamp(100, 1000),
            max_bash_output_bytes: ((window_tokens as usize) * 2).clamp(10_000, 200_000),
        }
    }

    /// Provides a default budget when the model or its context window is not explicitly known.
    /// Assumes a standard context window of 64k tokens.
    pub fn default_budget() -> Self {
        Self::from_context_window(64_000)
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self::default_budget()
    }
}
