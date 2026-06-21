use std::env;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::process::Command as TokioCommand;
use tokio::runtime::Builder;
use tokio::time::timeout;

use crate::lane_events::{LaneEvent, ShipMergeMethod, ShipProvenance};
use crate::sandbox::{
    build_linux_sandbox_command, resolve_sandbox_status_for_request, FilesystemIsolationMode,
    SandboxConfig, SandboxStatus,
};
use crate::ConfigLoader;

/// Input schema for the built-in bash execution tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BashCommandInput {
    pub command: String,
    pub timeout: Option<u64>,
    pub description: Option<String>,
    #[serde(rename = "run_in_background")]
    pub run_in_background: Option<bool>,
    #[serde(rename = "dangerouslyDisableSandbox")]
    pub dangerously_disable_sandbox: Option<bool>,
    #[serde(rename = "namespaceRestrictions")]
    pub namespace_restrictions: Option<bool>,
    #[serde(rename = "isolateNetwork")]
    pub isolate_network: Option<bool>,
    #[serde(rename = "filesystemMode")]
    pub filesystem_mode: Option<FilesystemIsolationMode>,
    #[serde(rename = "allowedMounts")]
    pub allowed_mounts: Option<Vec<String>>,
}

/// Output returned from a bash tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BashCommandOutput {
    pub stdout: String,
    pub stderr: String,
    #[serde(rename = "rawOutputPath")]
    pub raw_output_path: Option<String>,
    pub interrupted: bool,
    #[serde(rename = "isImage")]
    pub is_image: Option<bool>,
    #[serde(rename = "backgroundTaskId")]
    pub background_task_id: Option<String>,
    #[serde(rename = "backgroundedByUser")]
    pub backgrounded_by_user: Option<bool>,
    #[serde(rename = "assistantAutoBackgrounded")]
    pub assistant_auto_backgrounded: Option<bool>,
    #[serde(rename = "dangerouslyDisableSandbox")]
    pub dangerously_disable_sandbox: Option<bool>,
    #[serde(rename = "returnCodeInterpretation")]
    pub return_code_interpretation: Option<String>,
    #[serde(rename = "noOutputExpected")]
    pub no_output_expected: Option<bool>,
    #[serde(rename = "structuredContent")]
    pub structured_content: Option<Vec<serde_json::Value>>,
    #[serde(rename = "persistedOutputPath")]
    pub persisted_output_path: Option<String>,
    #[serde(rename = "persistedOutputSize")]
    pub persisted_output_size: Option<u64>,
    #[serde(rename = "sandboxStatus")]
    pub sandbox_status: Option<SandboxStatus>,
}

/// Executes a shell command with the requested sandbox settings.
pub fn execute_bash(input: BashCommandInput, max_output_bytes: usize) -> io::Result<BashCommandOutput> {
    let cwd = env::current_dir()?;
    let sandbox_status = sandbox_status_for_input(&input, &cwd);

    if input.run_in_background.unwrap_or(false) {
        let mut child = prepare_command(&input.command, &cwd, &sandbox_status, false);
        let child = child
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        return Ok(BashCommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            raw_output_path: None,
            interrupted: false,
            is_image: None,
            background_task_id: Some(child.id().to_string()),
            backgrounded_by_user: Some(false),
            assistant_auto_backgrounded: Some(false),
            dangerously_disable_sandbox: input.dangerously_disable_sandbox,
            return_code_interpretation: None,
            no_output_expected: Some(true),
            structured_content: None,
            persisted_output_path: None,
            persisted_output_size: None,
            sandbox_status: Some(sandbox_status),
        });
    }

    let runtime = Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(execute_bash_async(input, sandbox_status, cwd, max_output_bytes))
}

/// Detect git push to main and emit ship provenance event
fn detect_and_emit_ship_prepared(command: &str) {
    let trimmed = command.trim();
    // Simple detection: git push with main/master
    if trimmed.contains("git push") && (trimmed.contains("main") || trimmed.contains("master")) {
        // Emit ship.prepared event
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let provenance = ShipProvenance {
            source_branch: get_current_branch().unwrap_or_else(|| "unknown".to_string()),
            base_commit: get_head_commit().unwrap_or_default(),
            commit_count: 0, // Would need to calculate from range
            commit_range: "unknown..HEAD".to_string(),
            merge_method: ShipMergeMethod::DirectPush,
            actor: get_git_actor().unwrap_or_else(|| "unknown".to_string()),
            pr_number: None,
        };
        let _event = LaneEvent::ship_prepared(format!("{now}"), &provenance);
        // Log to stderr as interim routing before event stream integration
        eprintln!(
            "[ship.prepared] branch={} -> main, commits={}, actor={}",
            provenance.source_branch, provenance.commit_count, provenance.actor
        );
    }
}

fn get_current_branch() -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn get_head_commit() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn get_git_actor() -> Option<String> {
    let name = Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    Some(name)
}

async fn execute_bash_async(
    input: BashCommandInput,
    sandbox_status: SandboxStatus,
    cwd: std::path::PathBuf,
    max_output_bytes: usize,
) -> io::Result<BashCommandOutput> {
    // Detect and emit ship provenance for git push operations
    detect_and_emit_ship_prepared(&input.command);

    let mut command = prepare_tokio_command(&input.command, &cwd, &sandbox_status, true);

    let output_result = if let Some(timeout_ms) = input.timeout {
        if let Ok(result) = timeout(Duration::from_millis(timeout_ms), command.output()).await {
            (result?, false)
        } else {
            return Ok(timeout_output(&input, timeout_ms, sandbox_status));
        }
    } else {
        (command.output().await?, false)
    };

    let (output, interrupted) = output_result;
    let stdout = truncate_output(&String::from_utf8_lossy(&output.stdout), max_output_bytes);
    let stderr = truncate_output(&String::from_utf8_lossy(&output.stderr), max_output_bytes);
    let no_output_expected = Some(stdout.trim().is_empty() && stderr.trim().is_empty());
    let return_code_interpretation = output.status.code().and_then(|code| {
        if code == 0 {
            None
        } else {
            Some(format!("exit_code:{code}"))
        }
    });

    Ok(BashCommandOutput {
        stdout,
        stderr,
        raw_output_path: None,
        interrupted,
        is_image: None,
        background_task_id: None,
        backgrounded_by_user: None,
        assistant_auto_backgrounded: None,
        dangerously_disable_sandbox: input.dangerously_disable_sandbox,
        return_code_interpretation,
        no_output_expected,
        structured_content: None,
        persisted_output_path: None,
        persisted_output_size: None,
        sandbox_status: Some(sandbox_status),
    })
}

fn timeout_output(
    input: &BashCommandInput,
    timeout_ms: u64,
    sandbox_status: SandboxStatus,
) -> BashCommandOutput {
    let is_test = is_test_command(&input.command);
    let return_code_interpretation = if is_test { "test.hung" } else { "timeout" };
    BashCommandOutput {
        stdout: String::new(),
        stderr: format!("Command exceeded timeout of {timeout_ms} ms"),
        raw_output_path: None,
        interrupted: true,
        is_image: None,
        background_task_id: None,
        backgrounded_by_user: None,
        assistant_auto_backgrounded: None,
        dangerously_disable_sandbox: input.dangerously_disable_sandbox,
        return_code_interpretation: Some(String::from(return_code_interpretation)),
        no_output_expected: Some(true),
        structured_content: Some(vec![test_timeout_provenance(
            &input.command,
            timeout_ms,
            is_test,
        )]),
        persisted_output_path: None,
        persisted_output_size: None,
        sandbox_status: Some(sandbox_status),
    }
}

fn is_test_command(command: &str) -> bool {
    let normalized = command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    normalized.contains("cargo test")
        || normalized.contains("cargo nextest")
        || normalized.contains("npm test")
        || normalized.contains("pnpm test")
        || normalized.contains("yarn test")
        || normalized.contains("pytest")
}

fn test_timeout_provenance(
    command: &str,
    timeout_ms: u64,
    classified_as_test_hang: bool,
) -> serde_json::Value {
    json!({
        "event": if classified_as_test_hang { "test.hung" } else { "command.timeout" },
        "failureClass": if classified_as_test_hang { "test_hang" } else { "timeout" },
        "data": {
            "command": command,
            "timeoutMs": timeout_ms,
            "provenance": "bash.timeout",
            "classification": if classified_as_test_hang { "test.hung" } else { "timeout" }
        }
    })
}

fn sandbox_status_for_input(input: &BashCommandInput, cwd: &std::path::Path) -> SandboxStatus {
    let config = ConfigLoader::default_for(cwd).load().map_or_else(
        |_| SandboxConfig::default(),
        |runtime_config| runtime_config.sandbox().clone(),
    );
    let request = config.resolve_request(
        input.dangerously_disable_sandbox.map(|disabled| !disabled),
        input.namespace_restrictions,
        input.isolate_network,
        input.filesystem_mode,
        input.allowed_mounts.clone(),
    );
    resolve_sandbox_status_for_request(&request, cwd)
}

// ---------------------------------------------------------------------------
// Sudo password auto-supply from SUDO_PASSWORD env / .env
// ---------------------------------------------------------------------------

/// Resolve `SUDO_PASSWORD` from the process environment first, then fall back
/// to parsing the `.env` file in the given directory. Returns `None` when the
/// variable is absent or empty.
fn resolve_sudo_password(cwd: &Path, env_var: Option<String>) -> Option<String> {
    // 1. Process environment
    if let Some(value) = env_var {
        if !value.is_empty() {
            return Some(value);
        }
    }

    // 2. .env file in cwd (minimal parser, mirrors api crate's parse_dotenv)
    let env_path = cwd.join(".env");
    let content = std::fs::read_to_string(env_path).ok()?;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key
            .trim()
            .strip_prefix("export ")
            .map_or_else(|| raw_key.trim(), str::trim);
        if key != "SUDO_PASSWORD" {
            continue;
        }
        let trimmed = raw_value.trim();
        let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"')
            || trimmed.starts_with('\'') && trimmed.ends_with('\''))
            && trimmed.len() >= 2
        {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        };
        if !unquoted.is_empty() {
            return Some(unquoted.to_string());
        }
    }
    None
}

/// If `command` begins with `sudo` (possibly after env-var assignments) and
/// `SUDO_PASSWORD` is available, rewrite the command so the password is piped
/// through stdin:
///
/// ```text
/// sudo apt install foo
///   →  printf '%s\n' '<password>' | sudo -S apt install foo
/// ```
///
/// Returns `None` when no rewrite is necessary (no sudo, or no password).
fn rewrite_sudo_command(command: &str, cwd: &Path) -> Option<String> {
    rewrite_sudo_command_with_env(command, cwd, env::var("SUDO_PASSWORD").ok())
}

fn rewrite_sudo_command_with_env(
    command: &str,
    cwd: &Path,
    env_var: Option<String>,
) -> Option<String> {
    let trimmed = command.trim();

    // Fast path: does the command even mention sudo?
    if !trimmed.contains("sudo") {
        return None;
    }

    // Find the first token that is "sudo" (skip leading env-var assignments)
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let sudo_idx = tokens.iter().position(|&t| t == "sudo")?;

    let password = resolve_sudo_password(cwd, env_var)?;

    // Collect everything after "sudo"
    let after_sudo: Vec<&str> = tokens[sudo_idx + 1..].to_vec();

    // Check if -S is already present among sudo flags
    let has_stdin_flag = after_sudo.iter().any(|t| *t == "-S" || *t == "--stdin");

    // Reconstruct: prefix env-vars (if any) + printf pipe + sudo -S + rest
    let env_prefix = if sudo_idx > 0 {
        tokens[..sudo_idx].join(" ") + " "
    } else {
        String::new()
    };

    // Escape single quotes in the password for safe shell embedding:
    // replace ' with '\'' (end quote, literal quote, start quote)
    let escaped_password = password.replace('\'', "'\\''");

    let stdin_flag = if has_stdin_flag { "" } else { "-S " };
    let rest = after_sudo.join(" ");

    Some(format!(
        "{env_prefix}printf '%s\\n' '{escaped_password}' | sudo {stdin_flag}{rest}"
    ))
}

fn prepare_command(
    command: &str,
    cwd: &std::path::Path,
    sandbox_status: &SandboxStatus,
    create_dirs: bool,
) -> Command {
    if create_dirs {
        prepare_sandbox_dirs(cwd);
    }

    if let Some(launcher) = build_linux_sandbox_command(command, cwd, sandbox_status) {
        let mut prepared = Command::new(launcher.program);
        prepared.args(launcher.args);
        prepared.current_dir(cwd);
        prepared.envs(launcher.env);
        return prepared;
    }

    let effective_command =
        rewrite_sudo_command(command, cwd).unwrap_or_else(|| command.to_string());
    let mut prepared = Command::new("sh");
    prepared.arg("-lc").arg(&effective_command).current_dir(cwd);
    if sandbox_status.filesystem_active {
        prepared.env("HOME", cwd.join(".sandbox-home"));
        prepared.env("TMPDIR", cwd.join(".sandbox-tmp"));
    }
    prepared
}

fn prepare_tokio_command(
    command: &str,
    cwd: &std::path::Path,
    sandbox_status: &SandboxStatus,
    create_dirs: bool,
) -> TokioCommand {
    if create_dirs {
        prepare_sandbox_dirs(cwd);
    }

    let mut prepared =
        if let Some(launcher) = build_linux_sandbox_command(command, cwd, sandbox_status) {
            let mut cmd = TokioCommand::new(launcher.program);
            cmd.args(launcher.args);
            cmd.envs(launcher.env);
            cmd
        } else {
            let effective_command =
                rewrite_sudo_command(command, cwd).unwrap_or_else(|| command.to_string());
            let mut cmd = TokioCommand::new("sh");
            cmd.arg("-lc").arg(&effective_command);
            if sandbox_status.filesystem_active {
                cmd.env("HOME", cwd.join(".sandbox-home"));
                cmd.env("TMPDIR", cwd.join(".sandbox-tmp"));
            }
            cmd
        };

    prepared.current_dir(cwd);
    prepared.stdin(Stdio::null());
    prepared
}

fn prepare_sandbox_dirs(cwd: &Path) {
    let _ = std::fs::create_dir_all(cwd.join(".sandbox-home"));
    let _ = std::fs::create_dir_all(cwd.join(".sandbox-tmp"));
}

#[cfg(test)]
mod tests {
    use super::{execute_bash, rewrite_sudo_command_with_env, BashCommandInput};
    use crate::sandbox::FilesystemIsolationMode;

    #[test]
    fn executes_simple_command() {
        let output = execute_bash(BashCommandInput {
            command: String::from("printf 'hello'"),
            timeout: Some(1_000),
            description: None,
            run_in_background: Some(false),
            dangerously_disable_sandbox: Some(false),
            namespace_restrictions: Some(false),
            isolate_network: Some(false),
            filesystem_mode: Some(FilesystemIsolationMode::WorkspaceOnly),
            allowed_mounts: None,
        }, 16384)
        .expect("bash command should execute");

        assert_eq!(output.stdout, "hello");
        assert!(!output.interrupted);
        assert!(output.sandbox_status.is_some());
    }

    #[test]
    fn disables_sandbox_when_requested() {
        let output = execute_bash(BashCommandInput {
            command: String::from("printf 'hello'"),
            timeout: Some(1_000),
            description: None,
            run_in_background: Some(false),
            dangerously_disable_sandbox: Some(true),
            namespace_restrictions: None,
            isolate_network: None,
            filesystem_mode: None,
            allowed_mounts: None,
        }, 16384)
        .expect("bash command should execute");

        assert!(!output.sandbox_status.expect("sandbox status").enabled);
    }

    #[test]
    fn timed_out_test_command_is_classified_as_hung_test_with_provenance() {
        let output = execute_bash(BashCommandInput {
            command: String::from("sleep 1 # cargo test slow_case"),
            timeout: Some(1),
            description: None,
            run_in_background: Some(false),
            dangerously_disable_sandbox: Some(false),
            namespace_restrictions: Some(false),
            isolate_network: Some(false),
            filesystem_mode: Some(FilesystemIsolationMode::WorkspaceOnly),
            allowed_mounts: None,
        }, 16384)
        .expect("bash command should return structured timeout");

        assert!(output.interrupted);
        assert_eq!(
            output.return_code_interpretation.as_deref(),
            Some("test.hung")
        );
        let structured = output.structured_content.expect("structured content");
        assert_eq!(structured[0]["event"], "test.hung");
        assert_eq!(structured[0]["data"]["provenance"], "bash.timeout");
    }

    #[test]
    fn prevents_stdin_hangs_by_redirecting_to_null() {
        let output = execute_bash(BashCommandInput {
            command: String::from("cat"),
            timeout: Some(2_000),
            description: None,
            run_in_background: Some(false),
            dangerously_disable_sandbox: Some(true),
            namespace_restrictions: None,
            isolate_network: None,
            filesystem_mode: None,
            allowed_mounts: None,
        }, 16384)
        .expect("bash command should execute cleanly");

        assert!(
            !output.interrupted,
            "Command hung and was cut off by the timeout!"
        );
    }

    // --- sudo rewrite ---

    #[test]
    fn rewrites_sudo_when_password_available() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".env"), "SUDO_PASSWORD=\"testpass123\"")
            .expect("write .env");

        let result = rewrite_sudo_command_with_env("sudo apt install curl", dir.path(), None);
        assert!(result.is_some(), "should rewrite sudo command");
        let rewritten = result.unwrap();
        assert!(
            rewritten.contains("printf '%s\\n' 'testpass123'"),
            "should contain password in printf: {rewritten}"
        );
        assert!(
            rewritten.contains("sudo -S apt install curl"),
            "should contain -S flag and original args: {rewritten}"
        );
    }

    #[test]
    fn no_rewrite_without_sudo() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".env"), "SUDO_PASSWORD=\"testpass123\"")
            .expect("write .env");

        assert!(rewrite_sudo_command_with_env("ls -la", dir.path(), None).is_none());
        assert!(rewrite_sudo_command_with_env("echo hello", dir.path(), None).is_none());
    }

    #[test]
    fn no_rewrite_without_password() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No .env file at all
        assert!(rewrite_sudo_command_with_env("sudo ls", dir.path(), None).is_none());

        // .env exists but no SUDO_PASSWORD
        std::fs::write(dir.path().join(".env"), "OTHER_VAR=foo").expect("write .env");
        assert!(rewrite_sudo_command_with_env("sudo ls", dir.path(), None).is_none());
    }

    #[test]
    fn no_double_stdin_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".env"), "SUDO_PASSWORD=pass").expect("write .env");

        let result = rewrite_sudo_command_with_env("sudo -S apt update", dir.path(), None)
            .expect("should rewrite");
        // Should NOT have double -S
        assert!(
            !result.contains("-S -S"),
            "should not duplicate -S flag: {result}"
        );
        assert!(
            result.contains("sudo -S apt update"),
            "should preserve existing -S: {result}"
        );
    }

    #[test]
    fn handles_password_with_special_chars() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".env"), "SUDO_PASSWORD=\"p@ss'w0rd$\"")
            .expect("write .env");

        let result = rewrite_sudo_command_with_env("sudo rm -rf /tmp/test", dir.path(), None)
            .expect("should rewrite");
        // Single quotes in password should be escaped
        assert!(
            result.contains("p@ss'\\''w0rd$"),
            "should escape single quotes in password: {result}"
        );
    }

    #[test]
    fn preserves_env_var_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".env"), "SUDO_PASSWORD=mypass").expect("write .env");

        let result = rewrite_sudo_command_with_env(
            "DEBIAN_FRONTEND=noninteractive sudo apt install -y curl",
            dir.path(),
            None,
        )
        .expect("should rewrite");
        assert!(
            result.starts_with("DEBIAN_FRONTEND=noninteractive"),
            "should preserve env-var prefix: {result}"
        );
        assert!(
            result.contains("sudo -S"),
            "should contain sudo -S: {result}"
        );
    }
}

/// Truncate output to `max_bytes`, appending a marker when trimmed.
fn truncate_output(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Find the last valid UTF-8 boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = s[..end].to_string();
    truncated.push_str(&format!("\n\n[output truncated — exceeded {} bytes]", max_bytes));
    truncated
}

#[cfg(test)]
mod truncation_tests {
    use super::*;

    #[test]
    fn short_output_unchanged() {
        let s = "hello world";
        assert_eq!(truncate_output(s, 16384), s);
    }

    #[test]
    fn long_output_truncated() {
        let s = "x".repeat(20_000);
        let result = truncate_output(&s, 16384);
        assert!(result.len() < 20_000);
        assert!(result.ends_with("[output truncated — exceeded 16384 bytes]"));
    }

    #[test]
    fn exact_boundary_unchanged() {
        let s = "a".repeat(16384);
        assert_eq!(truncate_output(&s, 16384), s);
    }

    #[test]
    fn one_over_boundary_truncated() {
        let s = "a".repeat(16384 + 1);
        let result = truncate_output(&s, 16384);
        assert!(result.contains("[output truncated"));
    }
}
