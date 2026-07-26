use std::process::Command;
use serde_json::json;
use runtime::{BashCommandOutput, LaneEvent, LaneEventName, LaneEventStatus, LaneFailureClass};
use crate::tool_types::*;
use crate::util::{to_pretty_json, iso8601_now};

pub(crate) fn run_git_status(input: GitStatusInput) -> Result<String, String> {
    let mut args: Vec<&str> = vec!["status"];
    if input.short.unwrap_or(true) {
        args.push("--short");
        args.push("--branch");
    }
    match git_stdout(&args) {
        Some(output) => to_pretty_json(json!({
            "output": output
        })),
        None => Err(
            "git status failed. Ensure the current directory is inside a git repository."
                .to_string(),
        ),
    }
}

#[allow(clippy::needless_pass_by_value)]
/// Execute `git diff` with optional --cached, commit, and path filters.
/// Returns the diff output wrapped in a JSON object.

pub(crate) fn run_git_diff(input: GitDiffInput) -> Result<String, String> {
    let mut args: Vec<String> = vec!["diff".to_string()];
    if input.staged.unwrap_or(false) {
        args.push("--cached".to_string());
    }
    if let Some(ref commit) = input.commit {
        if let Some(ref commit2) = input.commit2 {
            args.push(format!("{commit}...{commit2}"));
        } else {
            args.push(commit.clone());
        }
    }
    if let Some(ref path) = input.path {
        args.push("--".to_string());
        args.push(path.clone());
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match git_stdout(&arg_refs) {
        Some(output) => to_pretty_json(json!({
            "output": output
        })),
        None => Err(
            "git diff failed. Ensure the current directory is inside a git repository.".to_string(),
        ),
    }
}

#[allow(clippy::needless_pass_by_value)]
/// Execute `git log` with count, author, date, and path filters.
/// Defaults to the last 20 commits.
pub(crate) fn run_git_log(input: GitLogInput) -> Result<String, String> {
    let mut args: Vec<String> = vec!["log".to_string()];
    let count = input.count.unwrap_or(20);
    args.push(format!("-n{count}"));
    if input.oneline.unwrap_or(false) {
        args.push("--oneline".to_string());
    }
    if let Some(ref author) = input.author {
        args.push(format!("--author={author}"));
    }
    if let Some(ref since) = input.since {
        args.push(format!("--since={since}"));
    }
    if let Some(ref until) = input.until {
        args.push(format!("--until={until}"));
    }
    if let Some(ref path) = input.path {
        args.push("--".to_string());
        args.push(path.clone());
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match git_stdout(&arg_refs) {
        Some(output) => to_pretty_json(json!({
            "output": output
        })),
        None => Err(
            "git log failed. Ensure the current directory is inside a git repository.".to_string(),
        ),
    }
}

/// Execute `git show` for a given commit, optionally with --stat or a file path.
/// Uses the `commit:path` syntax when a path is specified.

pub(crate) fn run_git_show(input: GitShowInput) -> Result<String, String> {
    let mut args: Vec<String> = vec!["show".to_string()];

    match input.format.as_deref() {
        Some("metadata") if input.path.is_some() => {
            return Err(
                "GitShow format \"metadata\" cannot be combined with path; metadata describes a commit, not a blob. Use format \"patch\" or \"stat\" with path, or omit path."
                    .to_string(),
            );
        }
        Some("metadata") => {
            args.push("--format=medium".to_string());
            args.push("--no-patch".to_string());
        }
        Some("stat") => {
            args.push("--stat".to_string());
        }
        Some("patch") | None => {
            if input.format.is_none() && input.stat.unwrap_or(false) {
                args.push("--stat".to_string());
            }
        }
        Some(other) => {
            return Err(format!(
                "unknown GitShow format: \"{other}\". Supported values: \"patch\" (default), \"stat\", \"metadata\"."
            ));
        }
    }

    if let Some(ref path) = input.path {
        args.push(format!("{}:{}", input.commit, path));
    } else {
        args.push(input.commit.clone());
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match git_stdout(&arg_refs) {
        Some(output) => to_pretty_json(json!({
            "output": output
        })),
        None => Err(format!(
            "git show {} failed. Ensure the commit exists.",
            input.commit
        )),
    }
}

#[allow(clippy::needless_pass_by_value)]
/// Execute `git blame` on a file, optionally restricted to a line range.

pub(crate) fn run_git_blame(input: GitBlameInput) -> Result<String, String> {
    let mut args: Vec<String> = vec!["blame".to_string()];
    if let (Some(start), Some(end)) = (input.start_line, input.end_line) {
        args.push(format!("-L{start},{end}"));
    }
    args.push(input.path.clone());
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match git_stdout(&arg_refs) {
        Some(output) => to_pretty_json(json!({
            "output": output
        })),
        None => Err(format!("git blame {} failed. Ensure the file exists and the directory is inside a git repository.", input.path)),
    }
}

#[allow(dead_code)]
pub(crate) fn resolve_main_ref(branch: &str) -> Option<String> {
    let has_local_main = git_ref_exists("main");
    let has_remote_main = git_ref_exists("origin/main");

    if branch == "main" && has_remote_main {
        Some("origin/main".to_string())
    } else if has_local_main {
        Some("main".to_string())
    } else if has_remote_main {
        Some("origin/main".to_string())
    } else {
        None
    }
}

#[allow(dead_code)]
pub(crate) fn git_ref_exists(reference: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(crate) fn git_stdout(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

#[allow(dead_code)]
pub(crate) fn branch_divergence_output(
    command: &str,
    branch: &str,
    main_ref: &str,
    commits_behind: usize,
    commits_ahead: Option<usize>,
    missing_fixes: &[String],
) -> BashCommandOutput {
    let relation = commits_ahead.map_or_else(
        || format!("is {commits_behind} commit(s) behind"),
        |ahead| format!("has diverged ({ahead} ahead, {commits_behind} behind)"),
    );
    let missing_summary = if missing_fixes.is_empty() {
        "(none surfaced)".to_string()
    } else {
        missing_fixes.join("; ")
    };
    let stderr = format!(
        "branch divergence detected before workspace tests: `{branch}` {relation} `{main_ref}`. Missing commits: {missing_summary}. Merge or rebase `{main_ref}` before re-running `{command}`."
    );

    BashCommandOutput {
        stdout: String::new(),
        stderr: stderr.clone(),
        raw_output_path: None,
        interrupted: false,
        is_image: None,
        background_task_id: None,
        backgrounded_by_user: None,
        assistant_auto_backgrounded: None,
        dangerously_disable_sandbox: None,
        return_code_interpretation: Some("preflight_blocked:branch_divergence".to_string()),
        no_output_expected: Some(false),
        structured_content: Some(vec![serde_json::to_value(
            LaneEvent::new(
                LaneEventName::BranchStaleAgainstMain,
                LaneEventStatus::Blocked,
                iso8601_now(),
            )
            .with_failure_class(LaneFailureClass::BranchDivergence)
            .with_detail(stderr.clone())
            .with_data(json!({
                "branch": branch,
                "mainRef": main_ref,
                "commitsBehind": commits_behind,
                "commitsAhead": commits_ahead,
                "missingCommits": missing_fixes,
                "blockedCommand": command,
                "recommendedAction": format!("merge or rebase {main_ref} before workspace tests")
            })),
        )
        .expect("lane event should serialize")]),
        persisted_output_path: None,
        persisted_output_size: None,
        sandbox_status: None,
    }
}

