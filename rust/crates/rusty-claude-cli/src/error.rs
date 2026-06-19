use serde_json::{Map, Value};


/// #77: Classify a stringified error message into a machine-readable kind.
///
/// Returns a `snake_case` token that downstream consumers can switch on instead
/// of regex-scraping the prose. The classification is best-effort prefix/keyword
/// matching against the error messages produced throughout the CLI surface.
pub fn classify_error_kind(message: &str) -> &'static str {
    // Check specific patterns first (more specific before generic)
    if message.starts_with("unknown_slash_command:") {
        "unknown_slash_command"
    } else if message.starts_with("command_not_found:") {
        "command_not_found"
    } else if message.contains("missing Anthropic credentials") {
        "missing_credentials"
    } else if message.contains("Manifest source files are missing")
        || message.starts_with("missing_manifests:")
    {
        "missing_manifests"
    } else if message.contains("no worker state file found") {
        "missing_worker_state"
    } else if message.contains("session not found") {
        "session_not_found"
    } else if message.contains("no managed sessions found") {
        "no_managed_sessions"
    } else if message.contains("legacy session is missing workspace binding") {
        // #780: must precede the generic "failed to restore session" arm — the full
        // error message is "failed to restore session: legacy session is missing workspace
        // binding: ...", so the specific arm must be checked first.
        "legacy_session_no_workspace_binding"
    } else if message.contains("Is a directory") || message.contains("os error 21") {
        // #787: --resume given a directory path instead of a .jsonl file
        "session_path_is_directory"
    } else if message.contains("failed to restore session") {
        "session_load_failed"
    } else if message.contains("unsupported ACP invocation") {
        "unsupported_acp_invocation"
    } else if message.starts_with("missing_argument:") {
        "missing_argument"
    } else if message.contains("unsupported skills action") {
        "unsupported_skills_action"
    } else if message.starts_with("invalid_install_source:") {
        "invalid_install_source"
    } else if message.starts_with("invalid_cwd:") {
        "invalid_cwd"
    } else if message.starts_with("invalid_output_path:") {
        "invalid_output_path"
    } else if message.starts_with("invalid_output_format:") {
        "invalid_output_format"
    } else if message.starts_with("invalid_tool_name:") {
        "invalid_tool_name"
    } else if message.contains("unrecognized argument") || message.contains("unknown option") {
        "cli_parse"
    } else if message.starts_with("missing_flag_value:") {
        "missing_flag_value"
    } else if message.starts_with("invalid_permission_mode:") {
        "invalid_permission_mode"
    } else if message.starts_with("invalid_flag_value:") {
        "invalid_flag_value"
    } else if message.starts_with("invalid_model:") {
        "invalid_model"
    } else if message.contains("invalid model syntax") {
        "invalid_model_syntax"
    } else if message.contains("is not yet implemented") {
        "unsupported_command"
    } else if message.contains("unsupported resumed command") {
        "unsupported_resumed_command"
    } else if message.contains("confirmation required") {
        "confirmation_required"
    } else if (message.contains("api failed") || message.contains("api returned"))
        && (message.contains("401")
            || message.contains("Unauthorized")
            || message.contains("authentication_error"))
    {
        // #781: sub-classify auth failures so wrappers can distinguish from rate-limit / server errors
        "api_auth_error"
    } else if (message.contains("api failed") || message.contains("api returned"))
        && (message.contains("429")
            || message.contains("rate_limit")
            || message.contains("rate limit"))
    {
        // #781: sub-classify rate-limit failures
        "api_rate_limit_error"
    } else if message.contains("api failed") || message.contains("api returned") {
        "api_http_error"
    } else if message.contains("mcpServers") {
        "malformed_mcp_config"
    } else if message.contains(".claw/settings.json") || message.contains(".claw.json") {
        // #763: config file JSON parse / validation errors (e.g. unterminated string, type mismatch)
        "config_parse_error"
    } else if message.starts_with("empty prompt") {
        "empty_prompt"
    } else if message.starts_with("interactive_only:") || message.contains("stdin is not a TTY") {
        "interactive_only"
    } else if message.starts_with("unknown agents subcommand:") {
        "unknown_agents_subcommand"
    } else if message.starts_with("agent not found:") {
        "agent_not_found"
    } else if message.contains("is not installed") || message.starts_with("plugin_not_found:") {
        "plugin_not_found"
    } else if message.contains("plugin source") && message.contains("was not found") {
        // #794: `plugins install /nonexistent/path` → "plugin source ... was not found"
        "plugin_source_not_found"
    } else if (message.contains("skill source") && message.contains("not found"))
        || message.starts_with("skill '")
    {
        "skill_not_found"
    } else if message.contains("Unsupported config section") {
        "unsupported_config_section"
    } else if message.contains("unknown_plugins_action") {
        "unknown_plugins_action"
    } else if message.starts_with("invalid_history_count:") || message.contains("invalid count") {
        "invalid_history_count"
    } else if message.starts_with("missing_prompt:") {
        "missing_prompt"
    } else if message.contains("has been removed.") {
        // #765: removed subcommands (login, logout) — hint contains migration guidance
        "removed_subcommand"
    } else if message.starts_with("unknown subcommand:") {
        // #785/#825: typo/unknown top-level subcommand (e.g. `claw dump` → did you mean dump-manifests?)
        // Unified under command_not_found in #825.
        "command_not_found"
    } else if message.starts_with("unexpected extra arguments")
        || message.starts_with("unexpected_extra_args:")
    {
        // #766: extra positionals after commands that take no arguments (e.g. claw diff)
        // #784: export extra-positional errors use the typed prefix form
        "unexpected_extra_args"
    } else if message.starts_with("invalid_resume_argument:") {
        // #768: --resume trailing arg is not a slash command
        "invalid_resume_argument"
    } else if message.starts_with("unknown_option:") {
        "unknown_option"
    } else if message.contains("is a slash command")
        || message.starts_with("interactive_only:")
        // #735: "slash command /X is interactive-only" emitted by interactive-only guard
        || (message.starts_with("slash command") && message.contains("interactive-only"))
    {
        "interactive_only"
    } else {
        "unknown"
    }
}

/// #77: Split a multi-line error message into (`short_reason`, `optional_hint`).
///
/// The `short_reason` is the first line (up to the first newline), and the hint
/// is the remaining text or `None` if there's no newline. This prevents the
/// runbook prose from being stuffed into the `error` field that downstream
/// parsers expect to be the short reason alone.
pub fn split_error_hint(message: &str) -> (String, Option<String>) {
    match message.split_once('\n') {
        Some((short, hint)) => (short.to_string(), Some(hint.trim().to_string())),
        None => (message.to_string(), None),
    }
}

pub fn invalid_tool_name_details(message: &str) -> (Option<String>, Vec<String>, Value) {
    let tool_name = message
        .strip_prefix("invalid_tool_name: unsupported tool in --allowedTools:")
        .and_then(|rest| rest.lines().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let available = message
        .lines()
        .find_map(|line| line.strip_prefix("Available:"))
        .map(|line| {
            line.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let aliases = message
        .lines()
        .find_map(|line| line.strip_prefix("Aliases:"))
        .map(|line| {
            line.split(',')
                .filter_map(|entry| entry.trim().split_once('='))
                .map(|(alias, canonical)| {
                    (
                        alias.trim().to_string(),
                        Value::String(canonical.trim().to_string()),
                    )
                })
                .collect::<Map<_, _>>()
        })
        .unwrap_or_default();
    (tool_name, available, Value::Object(aliases))
}

pub fn invalid_output_format_value(message: &str) -> Option<String> {
    message
        .strip_prefix("invalid_output_format: unsupported value for --output-format:")
        .and_then(|rest| rest.lines().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// #781: derive a stable fallback hint from a classified error kind when the error
/// message itself has no `\n`-delimited hint. Returns `None` for kinds where the
/// message is self-explanatory or no canonical remediation exists.
pub fn fallback_hint_for_error_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "api_auth_error" => {
            Some("Check that ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN is set and valid.")
        }
        "api_rate_limit_error" => {
            Some("You have hit the API rate limit. Wait and retry, or reduce request frequency.")
        }
        "missing_credentials" => {
            Some("Set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN before running claw.")
        }
        "config_parse_error" => Some(
            "Fix the JSON syntax or schema in the referenced .claw/settings.json or .claw.json file, then rerun the command.",
        ),
        // #787: session load failures have no \n-delimited hint from the OS error path
        "session_load_failed" => Some(
            "Pass a path to a .jsonl session file, not a directory. Managed sessions live in .claw/sessions/.",
        ),
        "session_path_is_directory" => Some(
            "--resume expects a .jsonl session file path, not a directory. Run `claw --output-format json /session list` to list managed sessions.",
        ),
        // #793: plugins uninstall/enable/disable of non-existing plugin propagates through
        // the ? operator with no \n delimiter, so split_error_hint returns None.
        "plugin_not_found" => Some("Run `claw plugins list` to see installed plugins."),
        // #794: plugins install with a path that doesn't exist
        "plugin_source_not_found" => Some(
            "Check that the path or URL is correct. Use a local directory or a valid registry id.",
        ),
        // #795: skills install/show of a non-existing skill path or name
        "skill_not_found" => Some(
            "Run `claw skills list` to see available skills, or `claw skills install <path>` to install a new one.",
        ),
        // #795/#431: unsupported/invalid skills lifecycle input should include actionable local guidance.
        "unsupported_skills_action" => Some(
            "Supported: list, show <name>, install <path>, uninstall <name>, help. Run `claw skills help` for details.",
        ),
        "invalid_install_source" => Some(
            "Pass a local skill directory containing SKILL.md or a standalone markdown file.",
        ),
        "invalid_tool_name" => Some(
            "Use canonical snake_case tool names from `available` or documented aliases from `tool_aliases`.",
        ),
        "invalid_output_format" => Some("Use --output-format text or --output-format json."),
        _ => None,
    }
}

