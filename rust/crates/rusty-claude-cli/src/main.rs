#![recursion_limit = "256"]
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    clippy::doc_markdown,
    clippy::len_zero,
    clippy::manual_string_new,
    clippy::match_same_arms,
    clippy::result_large_err,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unneeded_struct_pattern,
    clippy::unnecessary_wraps,
    clippy::unused_self
)]
mod init;
mod input;
pub mod render;
pub use render::*;
mod setup_wizard;

use std::collections::BTreeSet;
use std::env as std_env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::net::TcpListener;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};

use log::debug;

use api::{
    detect_provider_kind, model_family_identity_for, resolve_startup_auth_source, AnthropicClient,
    AuthSource, ContentBlockDelta, InputContentBlock, InputMessage, MessageRequest,
    MessageResponse, OutputContentBlock, PromptCache, ProviderClient as ApiProviderClient,
    ProviderKind, StreamEvent as ApiStreamEvent, ToolChoice, ToolDefinition,
    ToolResultContentBlock,
};

use commands::{
    classify_skills_slash_command, handle_agents_slash_command, handle_agents_slash_command_json,
    handle_mcp_slash_command, handle_mcp_slash_command_json, handle_plugins_slash_command,
    handle_skills_slash_command, handle_skills_slash_command_json, render_slash_command_help,
    render_slash_command_help_filtered, resolve_skill_invocation, resume_supported_slash_commands,
    slash_command_specs, validate_slash_command_input, PluginsCommandResult, SkillSlashDispatch,
    SlashCommand,
};
use init::initialize_repo;
use plugins::{PluginHooks, PluginManager, PluginManagerConfig, PluginRegistry};

use runtime::{
    check_base_commit, format_stale_base_warning, format_usd, load_oauth_credentials,
    load_system_prompt, load_system_prompt_with_context, pricing_for_model, resolve_expected_base,
    resolve_sandbox_status, ApiClient, ApiRequest, AssistantEvent, BaseCommitState,
    CompactionConfig, ConfigFileReport, ConfigLoader, ConfigSource, ContentBlock, ContextFile,
    ConversationMessage, ConversationRuntime, McpConfigCollection, McpInvalidServerConfig,
    McpServer, McpServerManager, McpServerSpec, McpTool, MessageRole, ModelPricing, PermissionMode,
    PermissionPolicy, ProjectContext, PromptCacheEvent, ResolvedPermissionMode, RuntimeError,
    RuntimeInvalidHookConfig, Session, TokenUsage, ToolError, ToolExecutor, UsageTracker,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tools::{
    canonical_allowed_tool_name, execute_tool, mvp_tool_specs, GlobalToolRegistry,
    RuntimeToolDefinition, ToolSearchOutput,
};

pub mod env;
pub use crate::env::*;
pub mod cli;
pub use cli::*;
pub mod config;
pub use config::*;
pub mod error;
pub use error::*;
pub mod git;
pub use git::*;
pub mod session;
pub use session::*;
pub mod status;
pub use status::*;
pub mod validation;
pub use validation::*;
pub mod help;
pub use help::*;
pub mod mcp;
pub use mcp::*;
pub mod setup;
pub use setup::*;
pub mod export;
pub use export::*;
pub mod doctor;
pub use doctor::*;
pub mod session_orchestrator;
pub mod tool_executor;
pub use session_orchestrator::*;
pub use tool_executor::*;
pub mod repl;
pub use repl::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const OFFICIAL_REPO_URL: &str = "https://github.com/ultraworkers/claw-code";
pub const OFFICIAL_REPO_SLUG: &str = "ultraworkers/claw-code";
pub const DEPRECATED_INSTALL_COMMAND: &str = "cargo install claw-code";
pub const GIT_SHA: Option<&str> = option_env!("GIT_SHA");
pub const GIT_SHA_SHORT: Option<&str> = option_env!("GIT_SHA_SHORT");
pub const GIT_DIRTY: Option<&str> = option_env!("GIT_DIRTY");
pub const GIT_BRANCH: Option<&str> = option_env!("GIT_BRANCH");
pub const GIT_COMMIT_DATE: Option<&str> = option_env!("GIT_COMMIT_DATE");
pub const GIT_COMMIT_TIMESTAMP: Option<&str> = option_env!("GIT_COMMIT_TIMESTAMP");
pub const RUSTC_VERSION: Option<&str> = option_env!("RUSTC_VERSION");
pub const BUILD_TARGET: Option<&str> = option_env!("TARGET");
pub const DEFAULT_MODEL: &str = "anthropic/claude-opus-4-7";
pub const DEFAULT_DATE: &str = match option_env!("BUILD_DATE") {
    Some(d) => d,
    None => "unknown",
};
pub const CLI_OPTION_SUGGESTIONS: &[&str] = &[
    "--help",
    "-h",
    "--version",
    "-V",
    "--model",
    "--output-format",
    "--permission-mode",
    "--cwd",
    "--directory",
    "-C",
    "--skip-permissions",
    "--dangerously-skip-permissions",
    "--allowedTools",
    "--allowed-tools",
    "--resume",
    "--acp",
    "-acp",
    "--print",
    "--compact",
    "--base-commit",
    "-p",
    "--preset",
    "--accept-danger-non-interactive",
];

pub mod utils;
pub use utils::*;

use runtime::session_control::LATEST_SESSION_REFERENCE;

fn main() {
    let _logger_guard = claw_logger::init_logger("claw");
    let _ = dotenvy::dotenv();
    tracing::info!(
        version = VERSION,
        git_sha = GIT_SHA.unwrap_or("unknown"),
        "claw starting"
    );
    if let Err(error) = run() {
        let message = error.to_string();
        tracing::error!(error = %error, "claw exiting with error");
        // When --output-format json is active, emit errors as JSON so downstream
        // tools can parse failures the same way they parse successes (ROADMAP #42).
        let argv: Vec<String> = std::env::args().collect();
        let json_output = raw_args_request_json_output(&argv[1..]);
        if json_output {
            // #77/#696: classify error by prefix so downstream claws can route
            // without regex-scraping prose. Keep the legacy `type`/`kind`
            // fields and add the stable status/error_kind/action contract used
            // by non-interactive command guards.
            let kind = classify_error_kind(&message);
            let (short_reason, inline_hint) = split_error_hint(&message);
            // #781: fall back to a kind-derived hint when the message has no \n-delimited hint
            let hint = inline_hint.or_else(|| fallback_hint_for_error_kind(kind).map(String::from));
            let mut error_json = serde_json::json!({
                "type": "error",
                "kind": kind,
                "status": "error",
                "error_kind": kind,
                "error": short_reason,
                "message": short_reason,
                "action": "abort",
                "hint": hint,
                "exit_code": 1,
            });
            if kind == "invalid_cwd" {
                if let Some(error) = error.downcast_ref::<InvalidCwdError>() {
                    if let Some(object) = error_json.as_object_mut() {
                        object.insert("path".to_string(), serde_json::json!(&error.path));
                        object.insert(
                            "reason".to_string(),
                            serde_json::json!(error.reason.as_str()),
                        );
                    }
                }
            } else if kind == "invalid_output_path" {
                if let Some(error) = error.downcast_ref::<InvalidOutputPathError>() {
                    if let Some(object) = error_json.as_object_mut() {
                        object.insert("path".to_string(), serde_json::json!(&error.path));
                        object.insert(
                            "reason".to_string(),
                            serde_json::json!(error.reason.as_str()),
                        );
                    }
                }
            } else if kind == "invalid_output_format" {
                if let Some(object) = error_json.as_object_mut() {
                    object.insert(
                        "value".to_string(),
                        serde_json::json!(invalid_output_format_value(&message)),
                    );
                    object.insert("expected".to_string(), serde_json::json!(["text", "json"]));
                }
            } else if kind == "invalid_tool_name" {
                let (tool_name, available, aliases) = invalid_tool_name_details(&message);
                if let Some(object) = error_json.as_object_mut() {
                    if let Some(tool_name) = tool_name {
                        object.insert("tool_name".to_string(), serde_json::json!(tool_name));
                    }
                    object.insert("available".to_string(), serde_json::json!(available));
                    object.insert("tool_aliases".to_string(), aliases);
                }
            } else if kind == "missing_argument" {
                if let Some(object) = error_json.as_object_mut() {
                    if message.contains("--tools") {
                        object.insert("argument".to_string(), serde_json::json!("--tools"));
                    } else if message.contains("prompt or subcommand") {
                        object.insert(
                            "argument".to_string(),
                            serde_json::json!("prompt or subcommand"),
                        );
                    }
                }
            }
            // #819/#820/#823: JSON mode error envelopes must go to stdout so machine
            // consumers can parse failures from stdout byte 0 (parity with all
            // non-interactive command guards that already use println! / to_stdout).
            println!("{}", error_json);
        } else {
            // #156: Add machine-readable error kind to text output so stderr observers
            // don't need to regex-scrape the prose.
            let kind = classify_error_kind(&message);
            if message.contains("`claw --help`") {
                eprintln!(
                    "[error-kind: {kind}]
error: {message}"
                );
            } else {
                eprintln!(
                    "[error-kind: {kind}]
error: {message}

Run `claw --help` for usage."
                );
            }
        }
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvalidCwdReason {
    Empty,
    NotFound,
    NotADirectory,
}

impl InvalidCwdReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::NotFound => "not_found",
            Self::NotADirectory => "not_a_directory",
        }
    }
}

#[derive(Debug)]
struct InvalidCwdError {
    path: String,
    reason: InvalidCwdReason,
}

impl InvalidCwdError {
    fn new(path: impl Into<String>, reason: InvalidCwdReason) -> Self {
        Self {
            path: path.into(),
            reason,
        }
    }
}

impl std::fmt::Display for InvalidCwdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid_cwd: {}: `{}`\nUsage: --cwd <path>, -C <path>, or --directory <path>",
            self.reason.as_str(),
            self.path
        )
    }
}

impl std::error::Error for InvalidCwdError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidOutputPathReason {
    Empty,
    ParentNotFound,
    ParentNotADirectory,
    PathIsDirectory,
}

impl InvalidOutputPathReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::ParentNotFound => "parent_not_found",
            Self::ParentNotADirectory => "parent_not_a_directory",
            Self::PathIsDirectory => "path_is_directory",
        }
    }
}

#[derive(Debug)]
pub struct InvalidOutputPathError {
    path: String,
    reason: InvalidOutputPathReason,
}

impl InvalidOutputPathError {
    fn new(path: impl Into<String>, reason: InvalidOutputPathReason) -> Self {
        Self {
            path: path.into(),
            reason,
        }
    }
}

impl std::fmt::Display for InvalidOutputPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid_output_path: {}: `{}`\nUsage: claw export [PATH] [--session SESSION] [--output PATH]",
            self.reason.as_str(),
            self.path
        )
    }
}

impl std::error::Error for InvalidOutputPathError {}

fn split_global_cwd_args(
    args: &[String],
) -> Result<(Vec<String>, Option<PathBuf>), Box<dyn std::error::Error>> {
    let mut filtered = Vec::with_capacity(args.len());
    let mut cwd = None;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--cwd" | "-C" | "--directory" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "missing_flag_value: missing value for --cwd.\nUsage: --cwd <path>, -C <path>, or --directory <path>",
                    )
                })?;
                cwd = Some(validate_global_cwd(value)?);
                index += 2;
            }
            flag if flag.starts_with("--cwd=") => {
                let value = &flag[6..];
                cwd = Some(validate_global_cwd(value)?);
                index += 1;
            }
            flag if flag.starts_with("--directory=") => {
                let value = &flag[12..];
                cwd = Some(validate_global_cwd(value)?);
                index += 1;
            }
            flag if global_flag_takes_value(flag) => {
                filtered.push(arg.clone());
                if let Some(value) = args.get(index + 1) {
                    filtered.push(value.clone());
                    index += 2;
                } else {
                    index += 1;
                }
            }
            flag if global_flag_is_value_inline(flag) => {
                filtered.push(arg.clone());
                index += 1;
            }
            flag if global_flag_without_value(flag) => {
                filtered.push(arg.clone());
                index += 1;
            }
            "--" => {
                filtered.extend(args[index..].iter().cloned());
                break;
            }
            other if other.starts_with('-') => {
                filtered.push(arg.clone());
                index += 1;
            }
            _ => {
                filtered.extend(args[index..].iter().cloned());
                break;
            }
        }
    }

    Ok((filtered, cwd))
}

fn global_flag_takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "--model"
            | "--preset"
            | "--output-format"
            | "--permission-mode"
            | "--base-commit"
            | "--reasoning-effort"
            | "--tools"
    )
}

fn global_flag_is_value_inline(flag: &str) -> bool {
    flag.starts_with("--model=")
        || flag.starts_with("--preset=")
        || flag.starts_with("--output-format=")
        || flag.starts_with("--permission-mode=")
        || flag.starts_with("--base-commit=")
        || flag.starts_with("--reasoning-effort=")
        || flag.starts_with("--tools=")
}

fn global_flag_without_value(flag: &str) -> bool {
    matches!(
        flag,
        "--help"
            | "-h"
            | "--version"
            | "-V"
            | "--dangerously-skip-permissions"
            | "--skip-permissions"
            | "--accept-danger-non-interactive"
            | "--compact"
            | "--allow-broad-cwd"
            | "--print"
            | "--acp"
            | "-acp"
    )
}

fn validate_global_cwd(value: &str) -> Result<PathBuf, InvalidCwdError> {
    if value.trim().is_empty() {
        return Err(InvalidCwdError::new(value, InvalidCwdReason::Empty));
    }
    let path = PathBuf::from(value);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_dir() => Ok(path),
        Ok(_) => Err(InvalidCwdError::new(value, InvalidCwdReason::NotADirectory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(InvalidCwdError::new(value, InvalidCwdReason::NotFound))
        }
        Err(_) => Err(InvalidCwdError::new(value, InvalidCwdReason::NotFound)),
    }
}

fn apply_global_cwd(cwd: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(cwd) = cwd {
        std::env::set_current_dir(cwd)?;
    }
    Ok(())
}

/// Read piped stdin content when stdin is not a terminal.
///
/// Returns `None` when stdin is attached to a terminal (interactive REPL use),
/// when reading fails, or when the piped content is empty after trimming.
/// Returns `Some(raw_content)` when a pipe delivered non-empty content.
fn read_piped_stdin() -> Option<String> {
    if io::stdin().is_terminal() {
        return None;
    }
    let mut buffer = String::new();
    if io::stdin().read_to_string(&mut buffer).is_err() {
        return None;
    }
    if buffer.trim().is_empty() {
        return None;
    }
    Some(buffer)
}

/// Merge a piped stdin payload into a prompt argument.
///
/// When `stdin_content` is `None` or empty after trimming, the prompt is
/// returned unchanged. Otherwise the trimmed stdin content is appended to the
/// prompt separated by a blank line so the model sees the prompt first and the
/// piped context immediately after it.
fn merge_prompt_with_stdin(prompt: &str, stdin_content: Option<&str>) -> String {
    let Some(raw) = stdin_content else {
        return prompt.to_string();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return prompt.to_string();
    }
    if prompt.is_empty() {
        return trimmed.to_string();
    }
    format!("{prompt}\n\n{trimmed}")
}

fn plugin_command_json(
    action: &str,
    target: Option<&str>,
    result: &commands::PluginsCommandResult,
    report: &plugins::PluginRegistryReport,
) -> Value {
    let failures = report.failures();
    json!({
        "kind": "plugin",
        "action": action,
        "target": target,
        "status": if failures.is_empty() { "ok" } else { "degraded" },
        "message": result.message,
        "reload_runtime": result.reload_runtime,
        "plugins": report.summaries().iter().map(plugin_summary_json).collect::<Vec<_>>(),
        "load_failures": failures.iter().map(plugin_load_failure_json).collect::<Vec<_>>(),
    })
}

pub fn plugin_summary_json(plugin: &plugins::PluginSummary) -> Value {
    json!({
        "id": &plugin.metadata.id,
        "name": &plugin.metadata.name,
        "version": &plugin.metadata.version,
        "description": &plugin.metadata.description,
        "kind": plugin.metadata.kind.to_string(),
        "source": &plugin.metadata.source,
        // #730: path parity with agents (#728) and skills (#729)
        "path": plugin.metadata.root.as_ref().map(|p| p.display().to_string()),
        "enabled": plugin.enabled,
        "lifecycle_state": plugin.lifecycle_state(),
        "lifecycle": {
            "configured": !plugin.lifecycle.is_empty(),
            "init": {
                "configured": !plugin.lifecycle.init.is_empty(),
                "command_count": plugin.lifecycle.init.len(),
            },
            "shutdown": {
                "configured": !plugin.lifecycle.shutdown.is_empty(),
                "command_count": plugin.lifecycle.shutdown.len(),
            },
        },
    })
}

pub fn plugin_load_failure_json(failure: &plugins::PluginLoadFailure) -> Value {
    json!({
        "plugin_root": failure.plugin_root.display().to_string(),
        "kind": failure.kind.to_string(),
        "source": &failure.source,
        "lifecycle_state": "load_failed",
        "error": failure.error().to_string(),
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    tracing::debug!(args = ?args, "parsing CLI arguments");
    // #824: suppress config deprecation prose warnings to stderr when JSON
    // output mode is active.  Scan the raw argv before parse_args so the
    // suppression is in place before any settings file is loaded.
    let json_mode = raw_args_request_json_output(&args);
    if json_mode {
        runtime::suppress_config_warnings_for_json_mode();
    }
    let (args, cwd) = split_global_cwd_args(&args)?;
    apply_global_cwd(cwd)?;
    let cwd_display = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    tracing::info!(cwd = %cwd_display, "working directory set");
    match parse_args(&args)? {
        CliAction::DumpManifests {
            output_format,
            manifests_dir,
        } => dump_manifests(manifests_dir.as_deref(), output_format)?,
        CliAction::BootstrapPlan { output_format } => print_bootstrap_plan(output_format)?,
        CliAction::Agents {
            args,
            output_format,
        } => LiveCli::print_agents(args.as_deref(), output_format)?,
        CliAction::Mcp {
            args,
            output_format,
        } => LiveCli::print_mcp(args.as_deref(), output_format)?,
        CliAction::Skills {
            args,
            output_format,
        } => LiveCli::print_skills(args.as_deref(), output_format)?,
        CliAction::Plugins {
            action,
            target,
            output_format,
        } => LiveCli::print_plugins(action.as_deref(), target.as_deref(), output_format)?,
        CliAction::PrintSystemPrompt {
            cwd,
            date,
            model,
            output_format,
        } => print_system_prompt(cwd, date, &model, output_format)?,
        CliAction::Version { output_format } => print_version(output_format)?,
        CliAction::ResumeSession {
            session_path,
            commands,
            output_format,
            allow_broad_cwd,
            preset,
        } => {
            enforce_broad_cwd_policy(allow_broad_cwd, output_format)?;
            resume_session(&session_path, &commands, output_format)
        }
        CliAction::Status {
            model,
            model_flag_raw,
            permission_mode,
            output_format,
            allowed_tools: tools,
        } => print_status_snapshot(
            &model,
            model_flag_raw.as_deref(),
            permission_mode,
            output_format,
            tools.as_ref(),
        )?,
        CliAction::Sandbox { output_format } => print_sandbox_status_snapshot(output_format)?,
        CliAction::Prompt {
            prompt,
            model,
            output_format,
            allowed_tools: tools,
            permission_mode,
            compact,
            base_commit,
            reasoning_effort,
            allow_broad_cwd,
            preset,
            attach_skill,
        } => {
            cleanup_orphaned_processes();
            tracing::info!(model = %model, permission_mode = %permission_mode.as_str(), "running prompt mode");
            enforce_broad_cwd_policy(allow_broad_cwd, output_format)?;
            run_stale_base_preflight(base_commit.as_deref());
            // Only consume piped stdin as prompt context when the permission
            // mode is fully unattended. In modes where the permission
            // prompter may invoke CliPermissionPrompter::decide(), stdin
            // must remain available for interactive approval; otherwise the
            // prompter's read_line() would hit EOF and deny every request.
            let stdin_context = if matches!(permission_mode, PermissionMode::DangerFullAccess) {
                read_piped_stdin()
            } else {
                None
            };
            let effective_prompt = merge_prompt_with_stdin(&prompt, stdin_context.as_deref());
            let resolved_model = resolve_repl_model(model)?;
            
            let mut extra_sections = Vec::new();
            if let Some(skill_name) = attach_skill {
                let cwd = std::env::current_dir()?;
                let skill_path = commands::resolve_skill_path(&cwd, &skill_name)
                    .unwrap_or_else(|_| std::path::PathBuf::from(&skill_name));
                let content = std::fs::read_to_string(&skill_path)?;
                extra_sections.push(content);
            }
            
            let mut cli = LiveCli::new(resolved_model, true, tools, permission_mode, extra_sections)?;
            cli.set_reasoning_effort(reasoning_effort);
            cli.run_turn_with_output(&effective_prompt, output_format, compact)?;
        }
        CliAction::Doctor {
            output_format,
            permission_mode,
        } => run_doctor(output_format, permission_mode)?,
        CliAction::Acp { output_format } => {
            print_acp_status(output_format)?;
            std::process::exit(2);
        }
        CliAction::SessionList { output_format } => run_session_list(output_format)?,
        CliAction::State { output_format } => run_worker_state(output_format)?,
        CliAction::Init { output_format } => run_init(output_format)?,
        CliAction::Setup { output_format: _ } => run_setup()?,
        // #146: dispatch pure-local introspection. Text mode uses existing
        // render_config_report/render_diff_report; JSON mode uses the
        // corresponding _json helpers already exposed for resume sessions.
        CliAction::Config {
            section,
            output_format,
        } => match output_format {
            CliOutputFormat::Text => {
                println!("{}", render_config_report(section.as_deref())?);
            }
            CliOutputFormat::Json | crate::cli::CliOutputFormat::Ndjson => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&render_config_json(section.as_deref())?)?
                );
            }
        },
        CliAction::Models {
            action,
            output_format,
        } => print_models(action.as_deref(), output_format)?,
        CliAction::Diff { output_format } => match output_format {
            CliOutputFormat::Text => {
                println!("{}", render_diff_report()?);
            }
            CliOutputFormat::Json | crate::cli::CliOutputFormat::Ndjson => {
                let cwd = friendly_cwd(std::env::current_dir()?);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&render_diff_json_for(&cwd)?)?
                );
            }
        },
        CliAction::Export {
            session_reference,
            output_path,
            output_format,
        } => run_export(&session_reference, output_path.as_deref(), output_format)?,
        CliAction::Repl {
            model,
            allowed_tools: tools,
            permission_mode,
            base_commit,
            reasoning_effort,
            allow_broad_cwd,
            preset,
            attach_skill,
        } => {
            cleanup_orphaned_processes();
            tracing::info!(model = %model, permission_mode = %permission_mode.as_str(), "entering REPL mode");
            let mut extra_sections = Vec::new();
            if let Some(skill_name) = attach_skill {
                let cwd = std::env::current_dir()?;
                let skill_path = commands::resolve_skill_path(&cwd, &skill_name)
                    .unwrap_or_else(|_| std::path::PathBuf::from(&skill_name));
                let content = std::fs::read_to_string(&skill_path)?;
                extra_sections.push(content);
            }

            run_repl(
                model,
                tools,
                permission_mode,
                base_commit,
                reasoning_effort,
                allow_broad_cwd,
                extra_sections,
            )?;
        }
        CliAction::HelpTopic {
            topic,
            output_format,
        } => print_help_topic(topic, output_format)?,
        CliAction::Help { output_format } => print_help(output_format)?,
    }
    Ok(())
}

static OUTPUT_FORMAT_SELECTION: OnceLock<Mutex<OutputFormatSelection>> = OnceLock::new();
// #468: duplicate global flag occurrences for provenance reporting
static DUPLICATE_FLAGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn cleanup_orphaned_processes() {
    tracing::info!("Cleaning up orphaned zombie processes and microservices before startup...");
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let current_pid = std::process::id();
    
    let targets = ["claw", "xcodebuildmcp", "mcp-server-macos-use", "claw-analog"];
    
    for (pid, process) in sys.processes() {
        if pid.as_u32() == current_pid {
            continue;
        }
        let name_str = process.name().to_string_lossy().to_lowercase();
        let is_target = targets.iter().any(|&t| {
            name_str == t || name_str.ends_with(&format!("/{}", t))
        });
        
        if is_target {
            tracing::info!("Killing zombie process: {} (PID: {})", name_str, pid);
            process.kill();
        }
    }
}

fn is_help_flag(value: &str) -> bool {
    matches!(value, "--help" | "-h")
}

fn parse_single_word_command_alias(
    rest: &[String],
    model: &str,
    // #148: raw --model flag input for status provenance. None = no flag.
    model_flag_raw: Option<&str>,
    permission_mode_override: Option<PermissionMode>,
    output_format: CliOutputFormat,
    tools: Option<AllowedToolSet>,
) -> Option<Result<CliAction, String>> {
    if rest.is_empty() {
        return None;
    }

    // Diagnostic verbs (help, version, status, sandbox, doctor, state) accept only the verb itself
    // or --help / -h as a suffix. Any other suffix args are unrecognized.
    let verb = &rest[0];
    let is_diagnostic = matches!(
        verb.as_str(),
        "help" | "version" | "status" | "sandbox" | "doctor" | "setup" | "state"
    );

    if is_diagnostic && rest.len() > 1 {
        // Diagnostic verb with trailing args: reject unrecognized suffix
        let all_extra_are_help = rest[1..].iter().all(|a| is_help_flag(a));
        if all_extra_are_help {
            // "doctor --help -h" is valid, routed to parse_local_help_action() instead
            return None;
        }
        // #720: `claw help <topic>` — when the verb is "help" and exactly one
        // non-flag argument follows, try to route to the topic's handler.
        if verb == "help" && rest.len() == 2 {
            let topic_name = rest[1].as_str();
            let topic = match topic_name {
                "status" => Some(LocalHelpTopic::Status),
                "sandbox" => Some(LocalHelpTopic::Sandbox),
                "doctor" => Some(LocalHelpTopic::Doctor),
                "acp" => Some(LocalHelpTopic::Acp),
                "init" => Some(LocalHelpTopic::Init),
                "setup" => Some(LocalHelpTopic::Setup),
                "state" => Some(LocalHelpTopic::State),
                "export" => Some(LocalHelpTopic::Export),
                "version" => Some(LocalHelpTopic::Version),
                "system-prompt" => Some(LocalHelpTopic::SystemPrompt),
                "dump-manifests" => Some(LocalHelpTopic::DumpManifests),
                "bootstrap-plan" => Some(LocalHelpTopic::BootstrapPlan),
                "resume" => Some(LocalHelpTopic::Resume),
                "session" => Some(LocalHelpTopic::Session),
                "compact" => Some(LocalHelpTopic::Compact),
                "agents" | "agent" => Some(LocalHelpTopic::Agents),
                "skills" | "skill" => Some(LocalHelpTopic::Skills),
                "plugins" | "plugin" | "marketplace" => Some(LocalHelpTopic::Plugins),
                "mcp" => Some(LocalHelpTopic::Mcp),
                "config" => Some(LocalHelpTopic::Config),
                "model" | "models" => Some(LocalHelpTopic::Model),
                "settings" => Some(LocalHelpTopic::Settings),
                "diff" => Some(LocalHelpTopic::Diff),
                _ => None,
            };
            if let Some(t) = topic {
                return Some(Ok(CliAction::HelpTopic {
                    topic: t,
                    output_format,
                }));
            }
            // Unknown topic: fall through to generic help.
            return Some(Ok(CliAction::Help { output_format }));
        }
        // Unrecognized suffix like "--json"
        let mut msg = format!(
            "unrecognized argument `{}` for subcommand `{}`",
            rest[1], verb
        );
        // #152: common mistake — users type `--json` expecting JSON output.
        // Hint at the correct flag so they don't have to re-read --help.
        if rest[1] == "--json" {
            msg.push_str("\nDid you mean `--output-format json`?");
        } else {
            // #752: generic fallback hint so cli_parse errors always have non-null hint
            msg.push_str(&format!("\nRun `claw {} --help` for usage.", verb));
        }
        return Some(Err(msg));
    }

    // #720: `claw help <topic>` — when `help` is the verb and a topic follows,
    // try to route to the topic's help handler instead of erroring.
    if rest.len() == 2 && rest[0] == "help" {
        let topic_name = rest[1].as_str();
        let topic = match topic_name {
            "status" => Some(LocalHelpTopic::Status),
            "sandbox" => Some(LocalHelpTopic::Sandbox),
            "doctor" => Some(LocalHelpTopic::Doctor),
            "acp" => Some(LocalHelpTopic::Acp),
            "init" => Some(LocalHelpTopic::Init),
            "setup" => Some(LocalHelpTopic::Setup),
            "state" => Some(LocalHelpTopic::State),
            "export" => Some(LocalHelpTopic::Export),
            "version" => Some(LocalHelpTopic::Version),
            "system-prompt" => Some(LocalHelpTopic::SystemPrompt),
            "dump-manifests" => Some(LocalHelpTopic::DumpManifests),
            "bootstrap-plan" => Some(LocalHelpTopic::BootstrapPlan),
            "resume" => Some(LocalHelpTopic::Resume),
            "session" => Some(LocalHelpTopic::Session),
            "compact" => Some(LocalHelpTopic::Compact),
            "agents" | "agent" => Some(LocalHelpTopic::Agents),
            "skills" | "skill" => Some(LocalHelpTopic::Skills),
            "plugins" | "plugin" | "marketplace" => Some(LocalHelpTopic::Plugins),
            "mcp" => Some(LocalHelpTopic::Mcp),
            "config" => Some(LocalHelpTopic::Config),
            "model" | "models" => Some(LocalHelpTopic::Model),
            "settings" => Some(LocalHelpTopic::Settings),
            "diff" => Some(LocalHelpTopic::Diff),
            _ => None,
        };
        if let Some(t) = topic {
            return Some(Ok(CliAction::HelpTopic {
                topic: t,
                output_format,
            }));
        }
        // Unknown topic falls through to the generic help action.
        return Some(Ok(CliAction::Help { output_format }));
    }

    // #453: fire guard for multi-word CLI subcommands too (claw cost list, claw model list, etc.)
    // For slash commands that are commonly used as prompts (explain, cost, tokens, etc.),
    // only fire the guard when there's exactly one token.
    if rest.is_empty() {
        return None;
    }
    // Known CLI subcommands that don't accept additional arguments
    const CLI_SUBCOMMANDS: &[&str] = &[
        "help", "version", "status", "sandbox", "doctor", "state", "config", "diff",
    ];
    if rest.len() > 1 && !CLI_SUBCOMMANDS.contains(&rest[0].as_str()) {
        return None;
    }

    match rest[0].as_str() {
        "help" => Some(Ok(CliAction::Help { output_format })),
        "version" => Some(Ok(CliAction::Version { output_format })),
        "status" => Some(Ok(CliAction::Status {
            model: model.to_string(),
            model_flag_raw: model_flag_raw.map(str::to_string), // #148
            permission_mode: permission_mode_override
                .map(PermissionModeProvenance::from_flag)
                .unwrap_or_else(permission_mode_provenance_for_current_dir),
            output_format,
            allowed_tools: tools,
        })),
        "sandbox" => Some(Ok(CliAction::Sandbox { output_format })),
        "doctor" => Some(Ok(CliAction::Doctor {
            output_format,
            permission_mode: permission_mode_override
                .map(PermissionModeProvenance::from_flag)
                .unwrap_or_else(permission_mode_provenance_for_current_dir),
        })),
        "setup" => Some(Ok(CliAction::Setup { output_format })),
        "state" => Some(Ok(CliAction::State { output_format })),
        // #146: let `config` and `diff` fall through to parse_subcommand
        // where they are wired as pure-local introspection, instead of
        // producing the "is a slash command" guidance. Zero-arg cases
        // reach parse_subcommand too via this None.
        "config" | "diff" => None,
        other => bare_slash_command_guidance(other).map(Err),
    }
}

fn bare_slash_command_guidance(command_name: &str) -> Option<String> {
    if matches!(
        command_name,
        "dump-manifests"
            | "bootstrap-plan"
            | "agents"
            | "mcp"
            | "plugin"
            | "plugins"
            | "marketplace"
            | "skills"
            | "system-prompt"
            | "init"
            | "prompt"
            | "export"
    ) {
        return None;
    }
    let slash_command = slash_command_specs()
        .iter()
        // #772: check both spec.name and spec.aliases for command-line invocations
        .find(|spec| spec.name == command_name || spec.aliases.contains(&command_name))?;
    let canonical_name = slash_command.name;
    // #745: newline before remediation text so split_error_hint populates hint field
    let guidance = if slash_command.resume_supported {
        format!(
            "`claw {command_name}` is a slash command.\nUse `claw --resume SESSION.jsonl /{canonical_name}` or start `claw` and run `/{canonical_name}`."
        )
    } else {
        format!(
            "`claw {command_name}` is a slash command.\nStart `claw` and run `/{canonical_name}` inside the REPL."
        )
    };
    // #772: help text still mentions the alias, but the remediation shows canonical form
    Some(guidance)
}

fn compact_interactive_only_error() -> String {
    // #749: newline before remediation so split_error_hint populates hint field
    "interactive_only: `claw compact` is an interactive/session command.\nStart `claw` and run `/compact`, or use `claw --resume SESSION.jsonl /compact` to compact an existing session."
        .to_string()
}

fn removed_auth_surface_error(command_name: &str) -> String {
    // #765: two-line format so split_error_hint() extracts hint into JSON envelope
    format!(
        "`claw {command_name}` has been removed.\nSet ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN instead."
    )
}

fn unexpected_diff_args_error(extra: &[String]) -> String {
    format!(
        "unexpected extra arguments after `claw diff`: {}\nUsage: claw diff",
        extra.join(" ")
    )
}

fn parse_acp_args(args: &[String], output_format: CliOutputFormat) -> Result<CliAction, String> {
    match args {
        [] => Ok(CliAction::Acp { output_format }),
        [subcommand] if subcommand == "serve" => Ok(CliAction::Acp { output_format }),
        _ => Err(String::from(
            "unsupported_acp_invocation: unsupported ACP invocation. Use `claw acp` or `claw acp serve`.\nACP/Zed editor integration is not implemented yet; `claw acp serve` reports status only.",
        )),
    }
}

pub fn try_resolve_bare_skill_prompt(cwd: &Path, trimmed: &str) -> Option<String> {
    let bare_first_token = trimmed.split_whitespace().next().unwrap_or_default();
    let looks_like_skill_name = !bare_first_token.is_empty()
        && !bare_first_token.starts_with('/')
        && bare_first_token
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if !looks_like_skill_name {
        return None;
    }
    match resolve_skill_invocation(cwd, Some(trimmed)) {
        Ok(SkillSlashDispatch::Invoke(prompt)) => Some(prompt),
        _ => None,
    }
}

fn join_optional_args(args: &[String]) -> Option<String> {
    let joined = args.join(" ");
    let trimmed = joined.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn parse_direct_slash_cli_action(
    rest: &[String],
    model: String,
    output_format: CliOutputFormat,
    tools: Option<AllowedToolSet>,
    permission_mode: PermissionModeProvenance,
    compact: bool,
    base_commit: Option<String>,
    reasoning_effort: Option<String>,
    allow_broad_cwd: bool,
) -> Result<CliAction, String> {
    let raw = rest.join(" ");
    match SlashCommand::parse(&raw) {
        Ok(Some(SlashCommand::Help)) => Ok(CliAction::Help { output_format }),
        Ok(Some(SlashCommand::Status)) => Ok(CliAction::Status {
            model,
            model_flag_raw: None,
            permission_mode,
            output_format,
            allowed_tools: tools,
        }),
        Ok(Some(SlashCommand::Sandbox)) => Ok(CliAction::Sandbox { output_format }),
        Ok(Some(SlashCommand::Diff)) => Ok(CliAction::Diff { output_format }),
        Ok(Some(SlashCommand::Version)) => Ok(CliAction::Version { output_format }),
        Ok(Some(SlashCommand::Doctor)) => Ok(CliAction::Doctor {
            output_format,
            permission_mode,
        }),
        Ok(Some(SlashCommand::Agents { args })) => Ok(CliAction::Agents {
            args,
            output_format,
        }),
        Ok(Some(SlashCommand::Mcp { action, target })) => Ok(CliAction::Mcp {
            args: match (action, target) {
                (None, None) => None,
                (Some(action), None) => Some(action),
                (Some(action), Some(target)) => Some(format!("{action} {target}")),
                (None, Some(target)) => Some(target),
            },
            output_format,
        }),
        Ok(Some(SlashCommand::Skills { args })) => {
            match classify_skills_slash_command(args.as_deref()) {
                SkillSlashDispatch::Invoke(prompt) => Ok(CliAction::Prompt {
                    prompt,
                    model,
                    output_format,
                    allowed_tools: tools,
                    permission_mode: permission_mode.mode,
                    compact,
                    base_commit,
                    reasoning_effort: reasoning_effort.clone(),
                    allow_broad_cwd,
                    preset: None, attach_skill: None,
                }),
                SkillSlashDispatch::Local => Ok(CliAction::Skills {
                    args,
                    output_format,
                }),
            }
        }
        Ok(Some(SlashCommand::Unknown(name))) => {
            // #828: /approve and /deny are valid REPL-only slash commands that
            // are not SlashCommand enum variants (they require an active tool
            // call in the REPL to be meaningful). Emit interactive_only so
            // machine consumers see the correct error_kind instead of
            // unknown_slash_command.
            if matches!(name.as_str(), "approve" | "yes" | "y" | "deny" | "no" | "n") {
                Err(format!(
                    "interactive_only: /{name} requires an active tool call in the REPL.\nStart `claw` and use /{name} to approve or deny a pending tool execution."
                ))
            } else {
                Err(format_unknown_direct_slash_command(&name))
            }
        }
        Ok(Some(command)) => Err({
            let _ = command;
            let command_name = &rest[0];
            // #829: only suggest --resume when the command is actually
            // resume-safe. Non-resume-safe commands (e.g. /commit, /pr)
            // previously suggested --resume, which just re-triggered
            // interactive_only on a second invocation.
            let bare_name = command_name.trim_start_matches('/');
            let is_resume_safe = commands::resume_supported_slash_commands()
                .iter()
                .any(|spec| spec.name == bare_name);
            if is_resume_safe {
                format!(
                    // #738: newline before remediation so split_error_hint populates hint field
                    "interactive_only: slash command {command_name} requires a live session.\nStart `claw` and run it there, or use `claw --resume SESSION.jsonl {command_name}` / `claw --resume {latest} {command_name}`.",
                    latest = LATEST_SESSION_REFERENCE,
                )
            } else {
                format!(
                    "interactive_only: slash command {command_name} requires a live REPL session.\nStart `claw` and run it there."
                )
            }
        }),
        Ok(None) => Err(format!("unknown subcommand: {}", rest[0])),
        Err(error) => Err(error.to_string()),
    }
}

fn omc_compatibility_note_for_unknown_slash_command(name: &str) -> Option<&'static str> {
    name.starts_with("oh-my-claudecode:")
        .then_some(
            "Compatibility note: `/oh-my-claudecode:*` is a Claude Code/OMC plugin command. `claw` does not yet load plugin slash commands, Claude statusline stdin, or OMC session hooks.",
        )
}

pub fn suggest_slash_commands(input: &str) -> Vec<String> {
    let mut candidates = slash_command_specs()
        .iter()
        .flat_map(|spec| {
            std::iter::once(spec.name)
                .chain(spec.aliases.iter().copied())
                .map(|name| format!("/{name}"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.dedup();
    let candidate_refs = candidates.iter().map(String::as_str).collect::<Vec<_>>();
    ranked_suggestions(input.trim_start_matches('/'), &candidate_refs)
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn suggest_closest_term<'a>(input: &str, candidates: &'a [&'a str]) -> Option<&'a str> {
    ranked_suggestions(input, candidates).into_iter().next()
}

pub fn suggest_similar_subcommand(input: &str) -> Option<Vec<String>> {
    const KNOWN_SUBCOMMANDS: &[&str] = &[
        "help",
        "version",
        "status",
        "sandbox",
        "doctor",
        "setup",
        "state",
        "dump-manifests",
        "bootstrap-plan",
        "agents",
        "mcp",
        "skills",
        "system-prompt",
        "acp",
        "init",
        "export",
        "prompt",
        "list",
    ];

    let normalized_input = input.to_ascii_lowercase();
    let mut ranked = KNOWN_SUBCOMMANDS
        .iter()
        .filter_map(|candidate| {
            let normalized_candidate = candidate.to_ascii_lowercase();
            let distance = levenshtein_distance(&normalized_input, &normalized_candidate);
            let prefix_match = common_prefix_len(&normalized_input, &normalized_candidate) >= 4;
            let substring_match = normalized_candidate.contains(&normalized_input)
                || normalized_input.contains(&normalized_candidate);
            ((distance <= 2) || prefix_match || substring_match).then_some((distance, *candidate))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.cmp(right).then_with(|| left.1.cmp(right.1)));
    ranked.dedup_by(|left, right| left.1 == right.1);
    let suggestions = ranked
        .into_iter()
        .map(|(_, candidate)| candidate.to_string())
        .take(3)
        .collect::<Vec<_>>();
    (!suggestions.is_empty()).then_some(suggestions)
}

pub fn is_known_top_level_subcommand(value: &str) -> bool {
    matches!(
        value,
        "help"
            | "version"
            | "status"
            | "sandbox"
            | "doctor"
            | "state"
            | "dump-manifests"
            | "bootstrap-plan"
            | "agents"
            | "agent"
            | "mcp"
            | "skills"
            | "skill"
            | "plugins"
            | "plugin"
            | "marketplace"
            | "system-prompt"
            | "acp"
            | "init"
            | "export"
            | "prompt"
            | "resume"
            | "session"
            | "compact"
            | "config"
            | "model"
            | "models"
            | "settings"
            | "diff"
    )
}

fn common_prefix_len(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(l, r)| l == r)
        .count()
}

pub fn looks_like_subcommand_typo(input: &str) -> bool {
    !input.is_empty()
        && input
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
}

fn ranked_suggestions<'a>(input: &str, candidates: &'a [&'a str]) -> Vec<&'a str> {
    let normalized_input = input.trim_start_matches('/').to_ascii_lowercase();
    let mut ranked = candidates
        .iter()
        .filter_map(|candidate| {
            let normalized_candidate = candidate.trim_start_matches('/').to_ascii_lowercase();
            let distance = levenshtein_distance(&normalized_input, &normalized_candidate);
            let prefix_bonus = usize::from(
                !(normalized_candidate.starts_with(&normalized_input)
                    || normalized_input.starts_with(&normalized_candidate)),
            );
            let score = distance + prefix_bonus;
            (score <= 4).then_some((score, *candidate))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.cmp(right).then_with(|| left.1.cmp(right.1)));
    ranked
        .into_iter()
        .map(|(_, candidate)| candidate)
        .take(3)
        .collect()
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    if left.is_empty() {
        return right.chars().count();
    }
    if right.is_empty() {
        return left.chars().count();
    }

    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != *right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + substitution_cost);
        }
        previous.clone_from(&current);
    }

    previous[right_chars.len()]
}

fn normalize_tools(values: &[String]) -> Result<Option<AllowedToolSet>, String> {
    if values.is_empty() {
        return Ok(None);
    }
    current_tool_registry()?.normalize_tool_list(values, "--tools")
}

fn tools_missing_error() -> String {
    "missing_argument: --tools requires a tool list before subcommands or flags.\nUsage: --tools <tool-name>[,<tool-name>...]  e.g. --tools read,glob".to_string()
}

fn compact_missing_argument_error() -> String {
    "missing_argument: --compact requires prompt text, piped stdin, or a subcommand. argument: prompt or subcommand\nUsage: claw --compact <prompt>  or  echo '<prompt>' | claw --compact"
        .to_string()
}

pub fn should_reject_unknown_option_like(value: &str) -> bool {
    is_registered_cli_flag_token(value)
        || (value.starts_with("--")
            && suggest_closest_term(value, CLI_OPTION_SUGGESTIONS).is_some())
}

pub fn format_unknown_option(option: &str) -> String {
    if option == "--" {
        return "end_of_flags: `--` terminates flag parsing. Pass literal prompt text after it, for example `claw -- \"-literal prompt\"`.\nRun `claw --help` for usage.".to_string();
    }
    let mut message = format!("unknown option: {option}");
    if let Some(suggestion) = suggest_closest_term(option, CLI_OPTION_SUGGESTIONS) {
        message.push_str("\nDid you mean ");
        message.push_str(suggestion);
        message.push('?');
    }
    message.push_str("\nRun `claw --help` for usage.");
    message
}

pub fn is_registered_cli_flag_token(value: &str) -> bool {
    let flag = value.split_once('=').map_or(value, |(flag, _)| flag);
    CLI_OPTION_SUGGESTIONS.contains(&flag)
}
