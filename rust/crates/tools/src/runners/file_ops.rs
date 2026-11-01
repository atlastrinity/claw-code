use crate::tool_types::*;
use crate::util::{io_to_string, to_pretty_json};
use crate::web::{execute_web_fetch, execute_web_search};
use runtime::{
    edit_file_in_workspace, glob_search_in_workspace, grep_search_in_workspace,
    read_file_in_workspace, write_file_in_workspace, ContextBudget, GrepSearchInput,
};

pub(crate) fn run_read_file(input: ReadFileInput, budget: ContextBudget) -> Result<String, String> {
    let workspace = runtime::workspace::workspace_root();
    match read_file_in_workspace(
        &input.path,
        input.offset,
        input.limit,
        &workspace,
        budget.max_read_file_lines,
    ) {
        Ok(output) => to_pretty_json(output),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                let target_path = std::path::Path::new(&input.path);
                let stem = target_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let suggestions = if !stem.is_empty() {
                    let parent = target_path.parent().unwrap_or(std::path::Path::new(""));
                    let candidates = vec![
                        workspace.join(parent),
                        std::env::current_dir().unwrap_or_default().join(parent),
                        std::path::PathBuf::from(parent),
                    ];
                    let mut matches = Vec::new();
                    let mut matched_dir = String::new();
                    for dir in candidates {
                        if let Ok(entries) = std::fs::read_dir(&dir) {
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().into_owned();
                                if name.to_lowercase().contains(&stem.to_lowercase())
                                    && !matches.contains(&name)
                                {
                                    matches.push(name);
                                }
                            }
                        }
                        if !matches.is_empty() {
                            matched_dir = dir.display().to_string();
                            break;
                        }
                    }
                    if matches.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "\n\nDid you mean one of these files in '{}'?\n  - {}",
                            matched_dir,
                            matches.join("\n  - ")
                        )
                    }
                } else {
                    String::new()
                };

                Err(format!(
                    "Error: File '{}' not found. The path might be incorrect or you are in the wrong directory. Please use the `list_dir` or `glob_search` tool to find the correct file path before trying to read again.{}",
                    input.path, suggestions
                ))
            } else if e.to_string().contains("binary") {
                let path_obj = std::path::Path::new(&input.path);
                let file_size = std::fs::metadata(path_obj).map(|m| m.len()).unwrap_or(0);
                Err(format!(
                    "Notice: File '{}' is a binary file (size: {} bytes). Full raw text reading is omitted to prevent terminal corruption. Use specific inspection tools (e.g. `file`, `stat`, or binary CLI parsers via `bash`) if necessary.",
                    input.path, file_size
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
    let output =
        write_file_in_workspace(&input.path, &input.content, &workspace).map_err(io_to_string)?;
    // Transactional working memory (M4) + zero-turn syntax backpressure (M3).
    runtime::turn_state::record_file_mutation(
        &output.file_path,
        output.original_file.as_deref(),
        &output.content,
    );
    let mut payload = serde_json::to_value(&output).map_err(|e| e.to_string())?;
    summarize_written_payload(&mut payload, &input.content);
    if let Some(defect) =
        runtime::syntax_guard::check_file_syntax(&output.file_path, &output.content)
    {
        // Carried as a JSON field: the payload stays parseable for the CLI
        // renderers while the marker still reaches the model verbatim.
        payload["syntax_backpressure"] = serde_json::Value::String(
            runtime::syntax_guard::backpressure_block(&defect)
                .trim_start()
                .to_string(),
        );
    }
    to_pretty_json(&payload)
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_edit_file(input: EditFileInput) -> Result<String, String> {
    if input.path.ends_with("task.md") {
        return Err("Error: Direct modification of task.md is forbidden. You MUST use the TaskGraph tool to maintain your task tree.".to_string());
    }
    let workspace = runtime::workspace::workspace_root();
    let output = edit_file_in_workspace(
        &input.path,
        &input.old_string,
        &input.new_string,
        input.replace_all.unwrap_or(false),
        &workspace,
    )
    .map_err(io_to_string)?;
    // `new_string` is only the replaced fragment; the post-edit content lives on
    // disk and is what the guard and the working memory must see.
    let current =
        std::fs::read_to_string(&output.file_path).unwrap_or_else(|_| output.new_string.clone());
    runtime::turn_state::record_file_mutation(
        &output.file_path,
        Some(&output.original_file),
        &current,
    );
    let mut payload = serde_json::to_value(&output).map_err(|e| e.to_string())?;
    summarize_written_payload(&mut payload, &current);
    if let Some(defect) = runtime::syntax_guard::check_file_syntax(&output.file_path, &current) {
        payload["syntax_backpressure"] = serde_json::Value::String(
            runtime::syntax_guard::backpressure_block(&defect)
                .trim_start()
                .to_string(),
        );
    }
    to_pretty_json(&payload)
}

/// Report the size of what landed, not the text that landed.
///
/// The assistant message that made this call already carries the file, and the
/// patch now carries the lines that really changed. `content` and `originalFile`
/// were echoes of text the model had in front of it either way: one write put
/// the same file in the context three times over, and a live run spent 21 KB of
/// tool results describing 6 KB of source. The scale stays — renderers and the
/// model both want "how much is there now" — the copy goes.
fn summarize_written_payload(payload: &mut serde_json::Value, file_text: &str) {
    if let Some(object) = payload.as_object_mut() {
        object.remove("content");
        object.remove("originalFile");
    }
    payload["fileBytes"] = serde_json::Value::from(file_text.len());
    payload["fileLines"] = serde_json::Value::from(file_text.lines().count());
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_glob_search(
    input: GlobSearchInputValue,
    budget: ContextBudget,
) -> Result<String, String> {
    let workspace = runtime::workspace::workspace_root();
    to_pretty_json(
        glob_search_in_workspace(
            &input.pattern,
            input.path.as_deref(),
            &workspace,
            budget.max_glob_files,
        )
        .map_err(io_to_string)?,
    )
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn run_grep_search(
    input: GrepSearchInput,
    budget: ContextBudget,
) -> Result<String, String> {
    let workspace = runtime::workspace::workspace_root();
    to_pretty_json(
        grep_search_in_workspace(&input, &workspace, budget.max_read_file_lines)
            .map_err(io_to_string)?,
    )
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
