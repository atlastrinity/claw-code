use runtime::{
    edit_file_in_workspace, glob_search_in_workspace, grep_search_in_workspace,
    read_file_in_workspace, write_file_in_workspace, GrepSearchInput, ContextBudget,
};
use crate::tool_types::*;
use crate::util::{to_pretty_json, io_to_string};
use crate::web::{execute_web_fetch, execute_web_search};

pub(crate) fn run_read_file(input: ReadFileInput, budget: ContextBudget) -> Result<String, String> {
    let workspace = runtime::workspace::workspace_root();
    match read_file_in_workspace(&input.path, input.offset, input.limit, &workspace, budget.max_read_file_lines) {
        Ok(output) => to_pretty_json(output),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                Err(format!(
                    "Error: File '{}' not found. The path might be incorrect or you are in the wrong directory. Please use the `list_dir` or `glob_search` tool to find the correct file path before trying to read again.",
                    input.path
                ))
            } else {
                Err(io_to_string(e))
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_write_file(input: WriteFileInput) -> Result<String, String> {
    if input.path.ends_with("task.md") {
        return Err("Error: Direct modification of task.md is forbidden. You MUST use the TaskGraph tool to maintain your task tree.".to_string());
    }
    let workspace = runtime::workspace::workspace_root();
    to_pretty_json(
        write_file_in_workspace(&input.path, &input.content, &workspace).map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_edit_file(input: EditFileInput) -> Result<String, String> {
    if input.path.ends_with("task.md") {
        return Err("Error: Direct modification of task.md is forbidden. You MUST use the TaskGraph tool to maintain your task tree.".to_string());
    }
    let workspace = runtime::workspace::workspace_root();
    to_pretty_json(
        edit_file_in_workspace(
            &input.path,
            &input.old_string,
            &input.new_string,
            input.replace_all.unwrap_or(false),
            &workspace,
        )
        .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_glob_search(input: GlobSearchInputValue, budget: ContextBudget) -> Result<String, String> {
    let workspace = runtime::workspace::workspace_root();
    to_pretty_json(
        glob_search_in_workspace(&input.pattern, input.path.as_deref(), &workspace, budget.max_glob_files)
            .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_grep_search(input: GrepSearchInput, budget: ContextBudget) -> Result<String, String> {
    let workspace = runtime::workspace::workspace_root();
    to_pretty_json(grep_search_in_workspace(&input, &workspace, budget.max_read_file_lines).map_err(io_to_string)?)
}

#[allow(clippy::needless_pass_by_value)]
#[allow(dead_code)]
pub(crate) fn run_web_fetch(input: WebFetchInput) -> Result<String, String> {
    to_pretty_json(execute_web_fetch(&input)?)
}

#[allow(clippy::needless_pass_by_value)]
#[allow(dead_code)]
pub(crate) fn run_web_search(input: WebSearchInput) -> Result<String, String> {
    to_pretty_json(execute_web_search(&input)?)
}


