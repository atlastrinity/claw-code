#![allow(clippy::must_use_candidate)]
//! Parallel-aware tool call dispatch.
//!
//! Groups model-issued tool calls into batches: read-only calls that can
//! safely execute in parallel, and mutating calls that must run sequentially.
//! This module does NOT change the [`ToolExecutor`] trait — parallelism is
//! achieved by routing known side-effect-free tools through a separate
//! stateless code path while everything else goes through the normal executor.

use std::collections::BTreeSet;

/// A single tool call request as emitted by the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRequest {
    pub tool_use_id: String,
    pub tool_name: String,
    pub input: String,
}

/// Result of executing one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub output: String,
    pub is_error: bool,
}

/// A batch of tool calls that share the same dispatch strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolBatch {
    /// All calls in this batch are side-effect-free and can execute concurrently.
    Parallel(Vec<ToolCallRequest>),
    /// This call has side-effects and must execute alone, in order.
    Sequential(ToolCallRequest),
}

/// Well-known read-only tools that are safe for parallel execution.
///
/// These tools perform only filesystem reads or in-memory computation.
/// They never create, modify, or delete files or external resources.
const PARALLEL_SAFE_TOOLS: &[&str] = &[
    "read_file",
    "Read",
    "grep_search",
    "glob_search",
    "list_dir",
    "ListDir",
    "ToolSearch",
    "ListMcpResourcesTool",
    "ReadMcpResourceTool",
];

/// Returns `true` if the tool is known to be side-effect-free and safe
/// for concurrent execution.
pub fn is_parallelizable(tool_name: &str) -> bool {
    PARALLEL_SAFE_TOOLS.contains(&tool_name)
}

/// Groups a sequence of tool calls into batches for optimal dispatch.
///
/// **Strategy**: consecutive parallelizable calls form a `Parallel` batch.
/// A non-parallelizable call breaks the batch and becomes a `Sequential` item.
///
/// ```text
/// [read_file, grep_search, write_file, read_file]
/// → Parallel([read_file, grep_search]), Sequential(write_file), Sequential(read_file)
/// ```
///
/// Note: the trailing `read_file` after `write_file` is Sequential because it
/// might depend on the write result. We only batch *consecutive* reads.
pub fn batch_tool_calls(calls: Vec<ToolCallRequest>) -> Vec<ToolBatch> {
    if calls.is_empty() {
        return Vec::new();
    }

    let mut batches: Vec<ToolBatch> = Vec::new();
    let mut parallel_buffer: Vec<ToolCallRequest> = Vec::new();

    for call in calls {
        if is_parallelizable(&call.tool_name) {
            parallel_buffer.push(call);
        } else {
            // Flush any accumulated parallel calls before the sequential one.
            if !parallel_buffer.is_empty() {
                flush_parallel_buffer(&mut parallel_buffer, &mut batches);
            }
            batches.push(ToolBatch::Sequential(call));
        }
    }

    // Flush any remaining parallel calls at the end.
    if !parallel_buffer.is_empty() {
        flush_parallel_buffer(&mut parallel_buffer, &mut batches);
    }

    batches
}

fn flush_parallel_buffer(buffer: &mut Vec<ToolCallRequest>, batches: &mut Vec<ToolBatch>) {
    if buffer.len() == 1 {
        // Single-item "parallel" batch is just sequential.
        batches.push(ToolBatch::Sequential(buffer.pop().unwrap()));
    } else {
        batches.push(ToolBatch::Parallel(std::mem::take(buffer)));
    }
}

/// Execute a batch of `Parallel` tool calls concurrently using `std::thread::scope`.
///
/// Each call is dispatched to its own scoped thread. Results are collected
/// and returned in the original call order.
///
/// The `execute_fn` closure is called once per tool call. It receives the
/// tool name and input, and should return `(output, is_error)`.
pub fn execute_parallel_batch<F>(
    calls: &[ToolCallRequest],
    execute_fn: F,
) -> Vec<ToolCallResult>
where
    F: Fn(&str, &str) -> (String, bool) + Sync,
{
    if calls.len() <= 1 {
        // Shortcut: no parallelism needed.
        return calls
            .iter()
            .map(|call| {
                let (output, is_error) = execute_fn(&call.tool_name, &call.input);
                ToolCallResult {
                    tool_use_id: call.tool_use_id.clone(),
                    tool_name: call.tool_name.clone(),
                    output,
                    is_error,
                }
            })
            .collect();
    }

    // Use scoped threads so we can borrow `execute_fn` without Arc/Mutex.
    std::thread::scope(|scope| {
        let handles: Vec<_> = calls
            .iter()
            .map(|call| {
                let tool_name = &call.tool_name;
                let input = &call.input;
                let execute = &execute_fn;
                scope.spawn(move || {
                    let (output, is_error) = execute(tool_name, input);
                    ToolCallResult {
                        tool_use_id: call.tool_use_id.clone(),
                        tool_name: call.tool_name.clone(),
                        output,
                        is_error,
                    }
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|handle| handle.join().expect("tool dispatch thread panicked"))
            .collect()
    })
}

/// Summary of how a tool batch was dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchReport {
    pub total_calls: usize,
    pub parallel_batches: usize,
    pub parallel_calls: usize,
    pub sequential_calls: usize,
}

/// Compute a dispatch report from a set of batches.
pub fn dispatch_report(batches: &[ToolBatch]) -> DispatchReport {
    let mut report = DispatchReport {
        total_calls: 0,
        parallel_batches: 0,
        parallel_calls: 0,
        sequential_calls: 0,
    };

    for batch in batches {
        match batch {
            ToolBatch::Parallel(calls) => {
                report.parallel_batches += 1;
                report.parallel_calls += calls.len();
                report.total_calls += calls.len();
            }
            ToolBatch::Sequential(_) => {
                report.sequential_calls += 1;
                report.total_calls += 1;
            }
        }
    }

    report
}

/// Set of tool names that the user has marked as parallel-safe,
/// extending the built-in list.
#[derive(Debug, Clone, Default)]
pub struct ParallelToolConfig {
    additional_parallel_tools: BTreeSet<String>,
    excluded_parallel_tools: BTreeSet<String>,
}

impl ParallelToolConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark an additional tool as safe for parallel execution.
    pub fn allow_parallel(&mut self, tool_name: impl Into<String>) {
        self.additional_parallel_tools.insert(tool_name.into());
    }

    /// Exclude a built-in tool from parallel execution.
    pub fn exclude_parallel(&mut self, tool_name: impl Into<String>) {
        self.excluded_parallel_tools.insert(tool_name.into());
    }

    /// Check whether a tool is parallelizable given this configuration.
    pub fn is_parallelizable(&self, tool_name: &str) -> bool {
        if self.excluded_parallel_tools.contains(tool_name) {
            return false;
        }
        is_parallelizable(tool_name) || self.additional_parallel_tools.contains(tool_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_call(id: &str, name: &str) -> ToolCallRequest {
        ToolCallRequest {
            tool_use_id: id.to_string(),
            tool_name: name.to_string(),
            input: format!(r#"{{"path":"src/{id}.rs"}}"#),
        }
    }

    // ── is_parallelizable ──────────────────────────────

    #[test]
    fn read_only_tools_are_parallelizable() {
        assert!(is_parallelizable("read_file"));
        assert!(is_parallelizable("Read"));
        assert!(is_parallelizable("grep_search"));
        assert!(is_parallelizable("glob_search"));
        assert!(is_parallelizable("list_dir"));
        assert!(is_parallelizable("ListDir"));
        assert!(is_parallelizable("ToolSearch"));
    }

    #[test]
    fn write_tools_are_not_parallelizable() {
        assert!(!is_parallelizable("write_file"));
        assert!(!is_parallelizable("edit_file"));
        assert!(!is_parallelizable("bash"));
        assert!(!is_parallelizable("Write"));
        assert!(!is_parallelizable("Edit"));
    }

    #[test]
    fn unknown_tools_default_to_sequential() {
        assert!(!is_parallelizable("custom_tool"));
        assert!(!is_parallelizable("MCPTool"));
    }

    // ── batch_tool_calls ───────────────────────────────

    #[test]
    fn empty_input_produces_no_batches() {
        let batches = batch_tool_calls(vec![]);
        assert!(batches.is_empty());
    }

    #[test]
    fn single_read_produces_sequential_batch() {
        let calls = vec![make_call("1", "read_file")];
        let batches = batch_tool_calls(calls);
        assert_eq!(batches.len(), 1);
        assert!(matches!(&batches[0], ToolBatch::Sequential(c) if c.tool_use_id == "1"));
    }

    #[test]
    fn consecutive_reads_batch_into_parallel() {
        let calls = vec![
            make_call("1", "read_file"),
            make_call("2", "grep_search"),
            make_call("3", "glob_search"),
        ];
        let batches = batch_tool_calls(calls);
        assert_eq!(batches.len(), 1);
        match &batches[0] {
            ToolBatch::Parallel(calls) => assert_eq!(calls.len(), 3),
            other => panic!("expected Parallel, got {other:?}"),
        }
    }

    #[test]
    fn write_breaks_parallel_batch() {
        let calls = vec![
            make_call("1", "read_file"),
            make_call("2", "read_file"),
            make_call("3", "write_file"),
            make_call("4", "read_file"),
        ];
        let batches = batch_tool_calls(calls);
        assert_eq!(batches.len(), 3);
        assert!(matches!(&batches[0], ToolBatch::Parallel(v) if v.len() == 2));
        assert!(matches!(&batches[1], ToolBatch::Sequential(c) if c.tool_name == "write_file"));
        assert!(matches!(&batches[2], ToolBatch::Sequential(c) if c.tool_name == "read_file"));
    }

    #[test]
    fn all_sequential_stays_sequential() {
        let calls = vec![
            make_call("1", "bash"),
            make_call("2", "write_file"),
            make_call("3", "edit_file"),
        ];
        let batches = batch_tool_calls(calls);
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| matches!(b, ToolBatch::Sequential(_))));
    }

    #[test]
    fn mixed_sequence_batches_correctly() {
        // read, read, bash, read, read, read, edit, read
        let calls = vec![
            make_call("1", "read_file"),
            make_call("2", "grep_search"),
            make_call("3", "bash"),
            make_call("4", "read_file"),
            make_call("5", "read_file"),
            make_call("6", "glob_search"),
            make_call("7", "edit_file"),
            make_call("8", "read_file"),
        ];
        let batches = batch_tool_calls(calls);
        // Expected: Parallel([1,2]), Sequential(3), Parallel([4,5,6]), Sequential(7), Sequential(8)
        assert_eq!(batches.len(), 5);
        assert!(matches!(&batches[0], ToolBatch::Parallel(v) if v.len() == 2));
        assert!(matches!(&batches[1], ToolBatch::Sequential(c) if c.tool_name == "bash"));
        assert!(matches!(&batches[2], ToolBatch::Parallel(v) if v.len() == 3));
        assert!(matches!(&batches[3], ToolBatch::Sequential(c) if c.tool_name == "edit_file"));
        assert!(matches!(&batches[4], ToolBatch::Sequential(c) if c.tool_name == "read_file"));
    }

    // ── execute_parallel_batch ─────────────────────────

    #[test]
    fn parallel_execution_returns_all_results() {
        let calls = vec![
            make_call("1", "read_file"),
            make_call("2", "read_file"),
            make_call("3", "grep_search"),
        ];

        let results = execute_parallel_batch(&calls, |name, _input| {
            (format!("result_for_{name}"), false)
        });

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].output, "result_for_read_file");
        assert_eq!(results[1].output, "result_for_read_file");
        assert_eq!(results[2].output, "result_for_grep_search");
        assert!(results.iter().all(|r| !r.is_error));
    }

    #[test]
    fn parallel_execution_preserves_errors() {
        let calls = vec![make_call("1", "read_file"), make_call("2", "read_file")];

        let results = execute_parallel_batch(&calls, |_name, _input| {
            ("file not found".to_string(), true)
        });

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_error));
    }

    #[test]
    fn parallel_execution_actually_runs_concurrently() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        let peak_concurrent = Arc::new(AtomicUsize::new(0));
        let current_concurrent = Arc::new(AtomicUsize::new(0));

        let calls: Vec<_> = (0..4)
            .map(|i| make_call(&format!("{i}"), "read_file"))
            .collect();

        let peak = Arc::clone(&peak_concurrent);
        let current = Arc::clone(&current_concurrent);

        let results = execute_parallel_batch(&calls, move |_name, _input| {
            let running = current.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(running, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            current.fetch_sub(1, Ordering::SeqCst);
            ("ok".to_string(), false)
        });

        assert_eq!(results.len(), 4);
        // With 4 calls and 50ms sleep, we should see >1 concurrent if truly parallel.
        let observed_peak = peak_concurrent.load(Ordering::SeqCst);
        assert!(
            observed_peak > 1,
            "expected concurrent execution, peak was {observed_peak}"
        );
    }

    // ── dispatch_report ────────────────────────────────

    #[test]
    fn dispatch_report_counts_correctly() {
        let batches = vec![
            ToolBatch::Parallel(vec![make_call("1", "read_file"), make_call("2", "read_file")]),
            ToolBatch::Sequential(make_call("3", "write_file")),
            ToolBatch::Parallel(vec![
                make_call("4", "grep_search"),
                make_call("5", "grep_search"),
                make_call("6", "grep_search"),
            ]),
        ];

        let report = dispatch_report(&batches);
        assert_eq!(report.total_calls, 6);
        assert_eq!(report.parallel_batches, 2);
        assert_eq!(report.parallel_calls, 5);
        assert_eq!(report.sequential_calls, 1);
    }

    // ── ParallelToolConfig ─────────────────────────────

    #[test]
    fn config_extends_parallel_tools() {
        let mut config = ParallelToolConfig::new();
        assert!(!config.is_parallelizable("my_custom_reader"));

        config.allow_parallel("my_custom_reader");
        assert!(config.is_parallelizable("my_custom_reader"));
        assert!(config.is_parallelizable("read_file")); // built-in still works
    }

    #[test]
    fn config_excludes_builtin_parallel_tools() {
        let mut config = ParallelToolConfig::new();
        assert!(config.is_parallelizable("read_file"));

        config.exclude_parallel("read_file");
        assert!(!config.is_parallelizable("read_file"));
        assert!(config.is_parallelizable("grep_search")); // others unaffected
    }
}
