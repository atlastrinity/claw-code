use serde_json::Value;
use std::time::Instant;
use runtime::{
    BashCommandInput, GrepSearchInput, ContextBudget,
    PermissionMode,
    security::permission_enforcer::{EnforcementResult, PermissionEnforcer},
    TaskPacket,
};

use crate::util::from_value;
use crate::tool_types::*;
use crate::runners::*;
use crate::web::{run_web_fetch, run_web_search};
use crate::skills::run_skill;
use crate::agent::run_agent;
use crate::tool_search::run_tool_search;
use crate::config::{run_config, run_enter_plan_mode, run_exit_plan_mode};
use crate::runners::run_powershell;
use crate::task_graph::{TaskGraphInput, run_task_graph, validate_active_task_for_tool};

/// Check permission before executing a tool. Returns Err with denial reason if blocked.
pub fn enforce_permission_check(
    enforcer: &PermissionEnforcer,
    tool_name: &str,
    input: &Value,
) -> Result<(), String> {
    let input_str = serde_json::to_string(input).unwrap_or_default();
    let result = enforcer.check(tool_name, &input_str);

    match result {
        EnforcementResult::Allowed => Ok(()),
        EnforcementResult::Denied { reason, .. } => Err(reason),
    }
}



pub fn execute_tool(name: &str, input: &Value) -> Result<String, String> {
    execute_tool_with_enforcer(None, name, input, ContextBudget::default_budget())
}

/// One log line per tool call: which tool, how long it took, and whether it
/// worked. This replaces an aspect-generated `[ENTRY]`/`[EXIT]` pair that named
/// neither the tool nor the duration and printed its result as
/// `"dyn core::any::Any"`, i.e. two lines per call carrying no information.
pub(crate) fn execute_tool_with_enforcer(
    enforcer: Option<&PermissionEnforcer>,
    name: &str,
    input: &Value,
    budget: ContextBudget,
) -> Result<String, String> {
    let started = Instant::now();
    let result = dispatch_tool(enforcer, name, input, budget);
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(output) => tracing::info!(
            tool = %name,
            ms = elapsed_ms,
            output_chars = output.len(),
            "tool call"
        ),
        Err(error) => tracing::warn!(
            tool = %name,
            ms = elapsed_ms,
            error = %summarize_error(error),
            "tool call failed"
        ),
    }
    result
}

/// Tool errors routinely embed the failing command's whole output; a log line
/// that repeats it makes the log unreadable and hides the tool name.
fn summarize_error(error: &str) -> String {
    const MAX_CHARS: usize = 300;
    let flat: String = error
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_CHARS {
        flat
    } else {
        let cut: String = flat.chars().take(MAX_CHARS).collect();
        format!("{cut}… (+{} chars)", flat.chars().count() - MAX_CHARS)
    }
}

#[allow(clippy::too_many_lines)]
fn dispatch_tool(
    enforcer: Option<&PermissionEnforcer>,
    name: &str,
    input: &Value,
    budget: ContextBudget,
) -> Result<String, String> {
    validate_active_task_for_tool(name, input)?;
    let mut res = match name {
        "bash" => {
            // Parse input to get the command for permission classification
            let bash_input: BashCommandInput = from_value(input)?;
            let classified_mode = classify_bash_permission(&bash_input.command);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, classified_mode)?;
            run_bash(bash_input, budget)
        }
        "read_file" => {
            let file_input: ReadFileInput = from_value(input)?;
            let required_mode = classify_read_path_permission(&file_input.path, false);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, required_mode)?;
            run_read_file(file_input, budget)
        }
        "write_file" => {
            let file_input: WriteFileInput = from_value(input)?;
            let required_mode = classify_file_path_permission(&file_input.path, true);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, required_mode)?;
            run_write_file(file_input)
        }
        "edit_file" => {
            let file_input: EditFileInput = from_value(input)?;
            let required_mode = classify_file_path_permission(&file_input.path, false);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, required_mode)?;
            run_edit_file(file_input)
        }
        "glob_search" => {
            let glob_input: GlobSearchInputValue = from_value(input)?;
            let required_mode = classify_glob_permission(&glob_input);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, required_mode)?;
            run_glob_search(glob_input, budget)
        }
        "grep_search" => {
            let grep_input: GrepSearchInput = from_value(input)?;
            let required_mode = classify_grep_permission(&grep_input);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, required_mode)?;
            run_grep_search(grep_input, budget)
        }
        "WebFetch" => {
            let web_input = from_value::<WebFetchInput>(input)?;
            maybe_enforce_permission_check_with_mode(
                enforcer,
                name,
                input,
                PermissionMode::DangerFullAccess,
            )?;
            run_web_fetch(web_input)
        }
        "WebSearch" => {
            let web_input = from_value::<WebSearchInput>(input)?;
            maybe_enforce_permission_check_with_mode(
                enforcer,
                name,
                input,
                PermissionMode::DangerFullAccess,
            )?;
            run_web_search(web_input)
        }
        "TaskGraph" => from_value::<TaskGraphInput>(input).and_then(run_task_graph),
        "Skill" => from_value::<SkillInput>(input).and_then(run_skill),
        "Agent" => from_value::<AgentInput>(input).and_then(run_agent),
        "ToolSearch" => from_value::<ToolSearchInput>(input).and_then(run_tool_search),
        "McpSearch" => from_value::<McpSearchInput>(input).and_then(run_mcp_search),
        "NotebookEdit" => from_value::<NotebookEditInput>(input).and_then(run_notebook_edit),
        "Sleep" => from_value::<SleepInput>(input).and_then(run_sleep),
        "SendUserMessage" | "Brief" => from_value::<BriefInput>(input).and_then(run_brief),
        "Config" => from_value::<ConfigInput>(input).and_then(run_config),
        "EnterPlanMode" => from_value::<EnterPlanModeInput>(input).and_then(run_enter_plan_mode),
        "ExitPlanMode" => from_value::<ExitPlanModeInput>(input).and_then(run_exit_plan_mode),
        "StructuredOutput" => {
            from_value::<StructuredOutputInput>(input).and_then(run_structured_output)
        }
        "REPL" => from_value::<ReplInput>(input).and_then(run_repl),
        "PowerShell" => {
            // Parse input to get the command for permission classification
            let ps_input: PowerShellInput = from_value(input)?;
            let classified_mode = classify_powershell_permission(&ps_input.command);
            maybe_enforce_permission_check_with_mode(enforcer, name, input, classified_mode)?;
            run_powershell(ps_input)
        }
        "AskUserQuestion" => {
            from_value::<AskUserQuestionInput>(input).and_then(run_ask_user_question)
        }
        "TaskCreate" => from_value::<TaskCreateInput>(input).and_then(run_task_create),
        "RunTaskPacket" => from_value::<TaskPacket>(input).and_then(run_task_packet),
        "TaskGet" => from_value::<TaskIdInput>(input).and_then(run_task_get),
        "TaskList" => run_task_list(input.clone()),
        "TaskStop" => from_value::<TaskIdInput>(input).and_then(run_task_stop),
        "TaskUpdate" => from_value::<TaskUpdateInput>(input).and_then(run_task_update),
        "TaskOutput" => from_value::<TaskIdInput>(input).and_then(run_task_output),
        "WorkerCreate" => from_value::<WorkerCreateInput>(input).and_then(run_worker_create),
        "WorkerGet" => from_value::<WorkerIdInput>(input).and_then(run_worker_get),
        "WorkerObserve" => from_value::<WorkerObserveInput>(input).and_then(run_worker_observe),
        "WorkerResolveTrust" => {
            from_value::<WorkerIdInput>(input).and_then(run_worker_resolve_trust)
        }
        "WorkerAwaitReady" => from_value::<WorkerIdInput>(input).and_then(run_worker_await_ready),
        "WorkerSendPrompt" => {
            from_value::<WorkerSendPromptInput>(input).and_then(run_worker_send_prompt)
        }
        "WorkerRestart" => from_value::<WorkerIdInput>(input).and_then(run_worker_restart),
        "WorkerTerminate" => from_value::<WorkerIdInput>(input).and_then(run_worker_terminate),
        "WorkerObserveCompletion" => from_value::<WorkerObserveCompletionInput>(input)
            .and_then(run_worker_observe_completion),
        "TeamCreate" => from_value::<TeamCreateInput>(input).and_then(run_team_create),
        "TeamDelete" => from_value::<TeamDeleteInput>(input).and_then(run_team_delete),
        "CronCreate" => from_value::<CronCreateInput>(input).and_then(run_cron_create),
        "CronDelete" => from_value::<CronDeleteInput>(input).and_then(run_cron_delete),
        "CronList" => run_cron_list(input.clone()),
        "LSP" => from_value::<LspInput>(input).and_then(run_lsp),
        "ListMcpResources" => {
            from_value::<McpResourceInput>(input).and_then(run_list_mcp_resources)
        }
        "ReadMcpResource" => from_value::<McpResourceInput>(input).and_then(run_read_mcp_resource),
        "McpAuth" => from_value::<McpAuthInput>(input).and_then(run_mcp_auth),
        "RemoteTrigger" => from_value::<RemoteTriggerInput>(input).and_then(run_remote_trigger),
        "MCP" => from_value::<McpToolInput>(input).and_then(run_mcp_tool),
        "TestingPermission" => {
            from_value::<TestingPermissionInput>(input).and_then(run_testing_permission)
        }
        "GitStatus" => from_value::<GitStatusInput>(input).and_then(run_git_status),
        "GitDiff" => from_value::<GitDiffInput>(input).and_then(run_git_diff),
        "GitLog" => from_value::<GitLogInput>(input).and_then(run_git_log),
        "GitShow" => from_value::<GitShowInput>(input).and_then(run_git_show),
        "GitBlame" => from_value::<GitBlameInput>(input).and_then(run_git_blame),
        "retrieve_context" => {
            maybe_enforce_permission_check_with_mode(
                enforcer,
                name,
                input,
                PermissionMode::ReadOnly,
            )?;
            from_value::<RetrieveContextInput>(input).and_then(run_retrieve_context)
        }
        "ingest_context" => {
            maybe_enforce_permission_check_with_mode(
                enforcer,
                name,
                input,
                PermissionMode::WorkspaceWrite,
            )?;
            from_value::<IngestContextInput>(input).and_then(run_ingest_context)
        }
        _ => Err(format!("unsupported tool: {name}")),
    };

    if let Ok(ref mut output) = res {
        const MAX_TOOL_OUTPUT_CHARS: usize = 16_000;
        if output.len() > MAX_TOOL_OUTPUT_CHARS {
            let total_len = output.len();
            let mut truncate_len = MAX_TOOL_OUTPUT_CHARS;
            while truncate_len > 0 && !output.is_char_boundary(truncate_len) {
                truncate_len -= 1;
            }
            output.truncate(truncate_len);
            output.push_str(&format!(
                "\n\n[PROGRESSIVE CHUNK NOTICE: Tool output chunked at {} characters to prevent context window overflow. Total output size was {} characters. Use specific line/file limits or targeted search queries to request further chunks if needed].",
                truncate_len, total_len
            ));
        }
    }

    let input_summary = serde_json::to_string(input).unwrap_or_default();
    match &mut res {
        Ok(out) => {
            if out.contains("BUILD FAILED") || out.contains("Command failed with exit code") || out.contains("Supported platforms for the buildables in the current scheme is empty") {
                if let Some(blocker) = crate::priority_manager::record_and_evaluate_tool_failure(name, &input_summary) {
                    out.push_str("\n\n");
                    out.push_str(&blocker);
                }
            } else {
                crate::priority_manager::clear_tool_failure(name, &input_summary);
            }
        }
        Err(err_msg) => {
            if let Some(blocker) = crate::priority_manager::record_and_evaluate_tool_failure(name, &input_summary) {
                err_msg.push_str("\n\n");
                err_msg.push_str(&blocker);
            }
        }
    }

    res
}

/// Enforce permission check with a dynamically classified permission mode./// Used for tools like bash and `PowerShell` where the required permission
/// depends on the actual command being executed.
pub(crate) fn maybe_enforce_permission_check_with_mode(
    enforcer: Option<&PermissionEnforcer>,
    tool_name: &str,
    input: &Value,
    required_mode: PermissionMode,
) -> Result<(), String> {
    if let Some(enforcer) = enforcer {
        let input_str = serde_json::to_string(input).unwrap_or_default();
        let result = enforcer.check_with_required_mode(tool_name, &input_str, required_mode);

        match result {
            EnforcementResult::Allowed => Ok(()),
            EnforcementResult::Denied { reason, .. } => Err(reason),
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_error;

    #[test]
    fn short_errors_pass_through_unchanged() {
        assert_eq!(
            summarize_error("No such file or directory: edge-tts"),
            "No such file or directory: edge-tts"
        );
    }

    #[test]
    fn long_errors_are_truncated_with_the_overflow_counted() {
        let summary = summarize_error(&"x".repeat(5_000));
        assert!(summary.chars().count() < 400, "summary was {} chars", summary.chars().count());
        assert!(summary.ends_with("… (+4700 chars)"), "summary was: {summary}");
    }

    #[test]
    fn embedded_command_output_does_not_break_the_log_line() {
        let summary = summarize_error("exit 1\nstdout:\nline1\n\tline2\r\n");
        assert!(!summary.contains(['\n', '\r', '\t']), "summary was: {summary:?}");
    }
}
