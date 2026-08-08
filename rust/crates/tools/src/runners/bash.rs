use std::path::{Path, PathBuf};
use std::process::Command;
use serde_json::json;
use runtime::{
    BashCommandInput, BashCommandOutput, PermissionMode, ContextBudget,
    execute_bash, check_freshness, BranchFreshness, LaneEvent, LaneEventName,
    LaneEventStatus, LaneFailureClass, GrepSearchInput,
};
use crate::util::{to_pretty_json, iso8601_now};
use crate::tool_types::*;
use crate::shell::execute_powershell;

/// Classify bash command permission based on command type and path.
/// ROADMAP #50: Read-only commands targeting CWD paths get `WorkspaceWrite`,
/// all others remain `DangerFullAccess`.
pub(crate) fn classify_bash_permission(command: &str) -> PermissionMode {
    // Read-only commands that are safe when targeting workspace paths
    const READ_ONLY_COMMANDS: &[&str] = &[
        "cat", "head", "tail", "less", "more", "ls", "ll", "dir", "find", "test", "[", "[[",
        "grep", "rg", "awk", "sed", "file", "stat", "readlink", "wc", "sort", "uniq", "cut", "tr",
        "pwd", "echo", "printf",
    ];

    // Get the base command (first word before any args or pipes)
    let base_cmd = command.split_whitespace().next().unwrap_or("");
    let base_cmd = base_cmd.split('|').next().unwrap_or("").trim();
    let base_cmd = base_cmd.split(';').next().unwrap_or("").trim();
    let base_cmd = base_cmd.split('>').next().unwrap_or("").trim();
    let base_cmd = base_cmd.split('<').next().unwrap_or("").trim();

    // Check if it's a read-only command
    let cmd_name = base_cmd.split('/').next_back().unwrap_or(base_cmd);
    let is_read_only = READ_ONLY_COMMANDS.contains(&cmd_name);

    if !is_read_only {
        return PermissionMode::DangerFullAccess;
    }

    // Check if any path argument is outside workspace
    // Simple heuristic: check for absolute paths not starting with CWD
    if has_dangerous_paths(command) {
        return PermissionMode::DangerFullAccess;
    }

    PermissionMode::WorkspaceWrite
}

/// Check if command has dangerous paths (outside workspace).
pub(crate) fn has_dangerous_paths(command: &str) -> bool {
    // Look for absolute paths
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let cwd = {
        let root = runtime::workspace::workspace_root();
        Some(root.canonicalize().unwrap_or(root))
    };

    for token in tokens {
        let token = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | '`' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}'
            )
        });
        // Skip flags/options
        if token.starts_with('-') {
            continue;
        }

        if token.contains('$') {
            return true;
        }

        if looks_like_windows_absolute_path(token) {
            return true;
        }

        // Check for absolute paths
        if token.starts_with('/') || token.starts_with("~/") {
            // Check if it's within CWD
            let path =
                PathBuf::from(token.replace('~', &std::env::var("HOME").unwrap_or_default()));
            if let Some(cwd) = cwd.as_ref() {
                let resolved = path.canonicalize().unwrap_or(path);
                if !resolved.starts_with(cwd) {
                    return true; // Path outside workspace
                }
            }
        }

        // Check for parent directory traversal that escapes workspace
        if token.contains("../..") || token.starts_with("../") && !token.starts_with("./") {
            return true;
        }

        if let Some(cwd) = cwd.as_ref() {
            if token.starts_with('.') || token.contains('/') || Path::new(token).exists() {
                let candidate = if Path::new(token).is_absolute() {
                    PathBuf::from(token)
                } else {
                    cwd.join(token)
                };
                if let Ok(canonical) = candidate.canonicalize() {
                    if !canonical.starts_with(cwd) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

pub(crate) fn looks_like_windows_absolute_path(token: &str) -> bool {
    let bytes = token.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || token.starts_with(r"\\")
}

pub(crate) fn analyze_command_failure_hint(stdout: &str, stderr: &str) -> Option<String> {
    let combined = format!("{}\n{}", stdout, stderr);
    if combined.contains("Supported platforms for the buildables in the current scheme is empty") {
        return Some("💡 DIAGNOSTIC HINT: xcodebuild failed because the Xcode scheme is missing supported platforms. Update .xcodegen.yml to set SDKROOT: iphoneos and SUPPORTED_PLATFORMS: \"iphonesimulator iphoneos\", then run 'xcodegen generate -s .xcodegen.yml'.".to_string());
    }
    if combined.contains("annotated with '@main' and must provide a main static function") || combined.contains("duplicate attribute '@main'") {
        return Some("💡 DIAGNOSTIC HINT: Swift compilation failed due to multiple '@main' entry points. Inspect your Swift files (e.g. SceneDelegate.swift vs App.swift) and remove redundant '@main' annotations.".to_string());
    }
    if combined.contains("Unable to find a device matching the provided destination specifier") {
        return Some("💡 DIAGNOSTIC HINT: xcodebuild/simctl could not find target device by name. Use 'xcrun simctl list devices booted' to get the booted device ID (e.g. -destination 'id=<UUID>').".to_string());
    }
    if combined.contains("No project spec found at") && combined.contains("project.yml") {
        return Some("💡 DIAGNOSTIC HINT: xcodegen default spec is project.yml. If your spec file is .xcodegen.yml, specify the spec flag: 'xcodegen generate -s .xcodegen.yml'.".to_string());
    }
    None
}

pub(crate) fn run_bash(input: BashCommandInput, budget: ContextBudget) -> Result<String, String> {
    if input.command.contains("task.md") && (input.command.contains('>') || input.command.contains("sed ") || input.command.contains("awk ") || input.command.contains("ed ") || input.command.contains("vim ") || input.command.contains("nano ") || input.command.contains("echo ")) {
        return Err("Error: Direct modification of task.md via bash is forbidden. You MUST use the TaskGraph tool to maintain your task tree.".to_string());
    }
    if let Some(output) = workspace_test_branch_preflight(&input.command) {
        return serde_json::to_string_pretty(&output).map_err(|error| error.to_string());
    }
    execute_bash(input, budget.max_bash_output_bytes)
        .map(|mut output| {
            if let Some(hint) = analyze_command_failure_hint(&output.stdout, &output.stderr) {
                if !output.stderr.is_empty() {
                    output.stderr.push_str("\n\n");
                }
                output.stderr.push_str(&hint);
            }
            to_pretty_json(output).unwrap_or_else(|_| "{}".to_string())
        })
        .map_err(|error| format!("failed to execute bash command: {error}"))
}

pub(crate) fn workspace_test_branch_preflight(command: &str) -> Option<BashCommandOutput> {
    if !is_workspace_test_command(command) {
        return None;
    }

    let branch = git_stdout(&["branch", "--show-current"])?;
    let main_ref = resolve_main_ref(&branch)?;
    let freshness = check_freshness(&branch, &main_ref);
    match freshness {
        BranchFreshness::Fresh => None,
        BranchFreshness::Stale {
            commits_behind,
            missing_fixes,
        } => Some(branch_divergence_output(
            command,
            &branch,
            &main_ref,
            commits_behind,
            None,
            &missing_fixes,
        )),
        BranchFreshness::Diverged {
            ahead,
            behind,
            missing_fixes,
        } => Some(branch_divergence_output(
            command,
            &branch,
            &main_ref,
            behind,
            Some(ahead),
            &missing_fixes,
        )),
    }
}

pub(crate) fn is_workspace_test_command(command: &str) -> bool {
    let normalized = normalize_shell_command(command);
    [
        "cargo test --workspace",
        "cargo test --all",
        "cargo nextest run --workspace",
        "cargo nextest run --all",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

pub(crate) fn normalize_shell_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

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






pub(crate) fn classify_file_path_permission(path: &str, allow_missing: bool) -> PermissionMode {
    if path_within_current_workspace(path, allow_missing) {
        PermissionMode::WorkspaceWrite
    } else {
        PermissionMode::DangerFullAccess
    }
}

pub(crate) fn classify_read_path_permission(path: &str, allow_missing: bool) -> PermissionMode {
    if path_within_current_workspace(path, allow_missing) {
        PermissionMode::ReadOnly
    } else {
        PermissionMode::DangerFullAccess
    }
}

pub(crate) fn classify_glob_permission(input: &GlobSearchInputValue) -> PermissionMode {
    let base_allowed = input
        .path
        .as_deref()
        .is_none_or(|path| path_within_current_workspace(path, false));
    let pattern_allowed = path_within_current_workspace(&input.pattern, true);
    if base_allowed && pattern_allowed {
        PermissionMode::ReadOnly
    } else {
        PermissionMode::DangerFullAccess
    }
}

pub(crate) fn classify_grep_permission(input: &GrepSearchInput) -> PermissionMode {
    if input
        .path
        .as_deref()
        .is_none_or(|path| path_within_current_workspace(path, false))
    {
        PermissionMode::ReadOnly
    } else {
        PermissionMode::DangerFullAccess
    }
}

pub(crate) fn path_within_current_workspace(path: &str, allow_missing: bool) -> bool {
    let trimmed = path.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}'
        )
    });
    if looks_like_windows_absolute_path(trimmed) {
        return false;
    }

    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let candidate = PathBuf::from(trimmed);
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    };

    let resolved = if allow_missing {
        absolute
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .map(|parent| parent.join(absolute.file_name().unwrap_or_default()))
            .unwrap_or(absolute)
    } else {
        match absolute.canonicalize() {
            Ok(path) => path,
            Err(_) => absolute,
        }
    };

    resolved.starts_with(cwd)
}

/// Classify `PowerShell` command permission based on command type and path.
/// ROADMAP #50: Read-only commands targeting CWD paths get `WorkspaceWrite`,
/// all others remain `DangerFullAccess`.
pub(crate) fn classify_powershell_permission(command: &str) -> PermissionMode {
    // Read-only commands that are safe when targeting workspace paths
    const READ_ONLY_COMMANDS: &[&str] = &[
        "Get-Content",
        "Get-ChildItem",
        "Test-Path",
        "Get-Item",
        "Get-ItemProperty",
        "Get-FileHash",
        "Select-String",
    ];

    // Check if command starts with a read-only cmdlet
    let cmd_lower = command.trim().to_lowercase();
    let is_read_only_cmd = READ_ONLY_COMMANDS
        .iter()
        .any(|cmd| cmd_lower.starts_with(&cmd.to_lowercase()));

    if !is_read_only_cmd {
        return PermissionMode::DangerFullAccess;
    }

    // Check if the path is within workspace (CWD or subdirectory)
    // Extract path from command - look for -Path or positional parameter
    let path = extract_powershell_path(command);
    match path {
        Some(p) if is_within_workspace(&p) => PermissionMode::WorkspaceWrite,
        _ => PermissionMode::DangerFullAccess,
    }
}

/// Extract the path argument from a `PowerShell` command.
pub(crate) fn extract_powershell_path(command: &str) -> Option<String> {
    // Look for -Path parameter
    if let Some(idx) = command.to_lowercase().find("-path") {
        let after_path = &command[idx + 5..];
        let path = after_path.split_whitespace().next()?;
        return Some(path.trim_matches('"').trim_matches('\'').to_string());
    }

    // Look for positional path parameter (after command name)
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.len() >= 2 {
        // Skip the cmdlet name and take the first argument
        let first_arg = parts[1];
        // Check if it looks like a path (contains \, /, or .)
        if first_arg.contains(['\\', '/', '.']) {
            return Some(first_arg.trim_matches('"').trim_matches('\'').to_string());
        }
    }

    None
}

/// Check if a path is within the current workspace.
pub(crate) fn is_within_workspace(path: &str) -> bool {
    let trimmed = path.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}'
        )
    });
    if looks_like_windows_absolute_path(trimmed) {
        return false;
    }

    let path = PathBuf::from(trimmed);

    // Reject any parent-directory traversal. Callers never need `..` to refer
    // to files inside the workspace, and `..` defeats both checks below: the
    // relative branch only inspects the leading component, and the absolute
    // branch's `canonicalize()` silently falls back to the literal `..` path
    // when the target does not exist yet (e.g. a file about to be created).
    // Returning false here is the safe direction: it classifies the command as
    // requiring full-access permission rather than workspace-write.
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }

    // If path is absolute, check if it starts with CWD
    if path.is_absolute() {
        if let Ok(cwd) = std::env::current_dir() {
            let cwd = cwd.canonicalize().unwrap_or(cwd);
            let resolved = path.canonicalize().unwrap_or(path);
            return resolved.starts_with(&cwd);
        }
    }

    // Relative paths are assumed to be within workspace
    !path.starts_with("/") && !path.starts_with("\\") && !path.starts_with("..")
}

pub(crate) fn run_powershell(input: PowerShellInput) -> Result<String, String> {
    if input.command.contains("task.md") && (input.command.contains('>') || input.command.contains("Set-Content") || input.command.contains("Add-Content") || input.command.contains("Out-File")) {
        return Err("Error: Direct modification of task.md via powershell is forbidden. You MUST use the TaskGraph tool to maintain your task tree.".to_string());
    }
    to_pretty_json(execute_powershell(input).map_err(|error| error.to_string())?)
}


