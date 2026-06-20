use crate::CliOutputFormat;
use crate::ConfigLoader;
use crate::{
    binary_provenance_for, format_stale_base_warning, invalid_hooks_json, load_oauth_credentials,
    memory_files_json, mvp_tool_specs, render_doctor_report, stale_base_json_value,
    ConfigWarningMode, DiagnosticCheck, DiagnosticLevel, HookValidationSummary,
    PermissionModeProvenance, StatusContext, BUILD_TARGET, DEPRECATED_INSTALL_COMMAND, GIT_SHA,
    OFFICIAL_REPO_SLUG, OFFICIAL_REPO_URL, VERSION,
};
use runtime::{PermissionMode, SandboxStatus};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub fn run_doctor(
    output_format: CliOutputFormat,
    permission_mode: PermissionModeProvenance,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = render_doctor_report(
        match output_format {
            CliOutputFormat::Json | CliOutputFormat::Ndjson => ConfigWarningMode::SuppressStderr,
            CliOutputFormat::Text => ConfigWarningMode::EmitStderr,
        },
        permission_mode,
    )?;
    let message = report.render();
    match output_format {
        CliOutputFormat::Text => println!("{message}"),
        CliOutputFormat::Json | CliOutputFormat::Ndjson => {
            println!("{}", serde_json::to_string_pretty(&report.json_value())?);
        }
    }
    if report.has_failures() {
        return Err("doctor found failing checks".into());
    }
    Ok(())
}

/// Run the interactive setup wizard to configure provider, API key, and model.
#[allow(clippy::too_many_lines)]
pub fn check_auth_health() -> DiagnosticCheck {
    let api_key_present = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let auth_token_present = std::env::var("ANTHROPIC_AUTH_TOKEN")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let openai_key_present = std::env::var("OPENAI_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let any_auth_present = api_key_present || auth_token_present || openai_key_present;
    let prompt_ready = any_auth_present;
    let env_details = format!(
        "Environment       api_key={} auth_token={} openai_key={}",
        if api_key_present { "present" } else { "absent" },
        if auth_token_present {
            "present"
        } else {
            "absent"
        },
        if openai_key_present {
            "present"
        } else {
            "absent"
        }
    );

    match load_oauth_credentials() {
        Ok(Some(token_set)) => DiagnosticCheck::new(
            "Auth",
            if any_auth_present {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warn
            },
            if any_auth_present {
                "supported auth env vars are configured; legacy saved OAuth is ignored"
            } else {
                "legacy saved OAuth credentials are present but unsupported"
            },
        )
        .with_details(vec![
            env_details,
            format!(
                "Legacy OAuth      expires_at={} refresh_token={} scopes={}",
                token_set
                    .expires_at
                    .map_or_else(|| "<none>".to_string(), |value| value.to_string()),
                if token_set.refresh_token.is_some() {
                    "present"
                } else {
                    "absent"
                },
                if token_set.scopes.is_empty() {
                    "<none>".to_string()
                } else {
                    token_set.scopes.join(",")
                }
            ),
            "Suggested action  set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN; `claw login` is removed"
                .to_string(),
        ])
        .with_hint("Set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN env var. The saved OAuth token is no longer accepted.")
        .with_data(Map::from_iter([
            ("api_key_present".to_string(), json!(api_key_present)),
            ("auth_token_present".to_string(), json!(auth_token_present)),
            ("openai_key_present".to_string(), json!(openai_key_present)),
            ("prompt_ready".to_string(), json!(prompt_ready)),
            ("prompt_blocked_reason".to_string(), if prompt_ready { Value::Null } else { json!("auth_missing") }),

            ("legacy_saved_oauth_present".to_string(), json!(true)),
            (
                "legacy_saved_oauth_expires_at".to_string(),
                json!(token_set.expires_at),
            ),
            (
                "legacy_refresh_token_present".to_string(),
                json!(token_set.refresh_token.is_some()),
            ),
            ("legacy_scopes".to_string(), json!(token_set.scopes)),
        ])),
        Ok(None) => DiagnosticCheck::new(
            "Auth",
            if any_auth_present {
                DiagnosticLevel::Ok
            } else {
                DiagnosticLevel::Warn
            },
            if any_auth_present {
                "supported auth env vars are configured"
            } else {
                "no supported auth env vars were found"
            },
        )
        .with_details(vec![env_details])
        .with_hint(if !any_auth_present { "Set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN to authenticate." } else { "" })
        .with_data(Map::from_iter([
            ("api_key_present".to_string(), json!(api_key_present)),
            ("auth_token_present".to_string(), json!(auth_token_present)),
            ("openai_key_present".to_string(), json!(openai_key_present)),
            ("prompt_ready".to_string(), json!(prompt_ready)),
            ("prompt_blocked_reason".to_string(), if prompt_ready { Value::Null } else { json!("auth_missing") }),
            ("legacy_saved_oauth_present".to_string(), json!(false)),
            ("legacy_saved_oauth_expires_at".to_string(), Value::Null),
            ("legacy_refresh_token_present".to_string(), json!(false)),
            ("legacy_scopes".to_string(), json!(Vec::<String>::new())),
        ])),
        Err(error) => DiagnosticCheck::new(
            "Auth",
            DiagnosticLevel::Fail,
            format!("failed to inspect legacy saved credentials: {error}"),
        )
        .with_hint("Set ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN env var to authenticate.")
        .with_data(Map::from_iter([
            ("api_key_present".to_string(), json!(api_key_present)),
            ("auth_token_present".to_string(), json!(auth_token_present)),
            ("openai_key_present".to_string(), json!(openai_key_present)),
            ("prompt_ready".to_string(), json!(prompt_ready)),
            ("prompt_blocked_reason".to_string(), if prompt_ready { Value::Null } else { json!("auth_missing") }),
            ("legacy_saved_oauth_present".to_string(), Value::Null),
            ("legacy_saved_oauth_expires_at".to_string(), Value::Null),
            ("legacy_refresh_token_present".to_string(), Value::Null),
            ("legacy_scopes".to_string(), Value::Null),
            ("legacy_saved_oauth_error".to_string(), json!(error.to_string())),
        ])),
    }
}

pub fn check_github_health() -> DiagnosticCheck {
    let github_token_present = std::env::var("GITHUB_TOKEN")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    let gh_token_present = std::env::var("GH_TOKEN")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());

    let token_present = github_token_present || gh_token_present;

    let env_details = format!(
        "Environment       GITHUB_TOKEN={} GH_TOKEN={}",
        if github_token_present {
            "present"
        } else {
            "absent"
        },
        if gh_token_present {
            "present"
        } else {
            "absent"
        }
    );

    DiagnosticCheck::new(
        "GitHub Integration",
        if token_present {
            DiagnosticLevel::Ok
        } else {
            DiagnosticLevel::Warn
        },
        if token_present {
            "GitHub token is configured for CI/CD access"
        } else {
            "no GitHub token found in environment"
        },
    )
    .with_details(vec![env_details])
    .with_hint(if !token_present {
        "Set GITHUB_TOKEN or GH_TOKEN to allow the agent to interact with GitHub/CI systems."
    } else {
        ""
    })
    .with_data(Map::from_iter([
        (
            "github_token_present".to_string(),
            json!(github_token_present),
        ),
        ("gh_token_present".to_string(), json!(gh_token_present)),
    ]))
}

/// #466: validate provider BASE_URL env vars
pub fn check_base_url_health() -> DiagnosticCheck {
    let base_url_vars = [
        ("ANTHROPIC_BASE_URL", "https://api.anthropic.com"),
        ("OPENAI_BASE_URL", "https://api.openai.com"),
        ("XAI_BASE_URL", "https://api.x.ai"),
        ("DASHSCOPE_BASE_URL", "https://dashscope.aliyuncs.com"),
    ];
    let mut issues: Vec<String> = Vec::new();
    let mut details: Vec<String> = Vec::new();
    for (var_name, default_url) in &base_url_vars {
        if let Ok(value) = std::env::var(var_name) {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                issues.push(format!("{var_name} is empty"));
                details.push(format!(
                    "{var_name}  empty (will use default: {default_url})"
                ));
            } else if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
                issues.push(format!("{var_name}={trimmed} is not a valid HTTP(S) URL"));
                details.push(format!("{var_name}  invalid ({trimmed})"));
            } else {
                details.push(format!("{var_name}  {trimmed}"));
            }
        }
    }
    if issues.is_empty() {
        DiagnosticCheck::new(
            "Base URLs",
            DiagnosticLevel::Ok,
            "provider base URL env vars are valid or unset",
        )
        .with_details(details)
    } else {
        DiagnosticCheck::new(
            "Base URLs",
            DiagnosticLevel::Warn,
            format!("{} base URL issue(s) found", issues.len()),
        )
        .with_details(details)
        .with_hint("Fix the reported BASE_URL env vars or unset them to use provider defaults.")
    }
}

pub fn check_config_health(
    config_loader: &ConfigLoader,
    config: Result<&runtime::RuntimeConfig, &runtime::ConfigError>,
) -> DiagnosticCheck {
    let discovered = config_loader.discover();
    let discovered_count = discovered.len();
    // Separate candidate paths that actually exist from those that don't.
    // Showing non-existent paths as "Discovered file" implies they loaded
    // but something went wrong, which is confusing. We only surface paths
    // that exist on disk as discovered; non-existent ones are silently
    // omitted from the display (they are just the standard search locations).
    let present_paths: Vec<String> = discovered
        .iter()
        .filter(|e| e.path.exists())
        .map(|e| e.path.display().to_string())
        .collect();
    let discovered_paths = discovered
        .iter()
        .map(|entry| entry.path.display().to_string())
        .collect::<Vec<_>>();
    match config {
        Ok(runtime_config) => {
            let loaded_entries = runtime_config.loaded_entries();
            let loaded_count = loaded_entries.len();
            let present_count = present_paths.len();
            let mut details = vec![format!(
                "Config files      loaded {}/{}",
                loaded_count, present_count
            )];
            if let Some(model) = runtime_config.model() {
                details.push(format!("Resolved model    {model}"));
            }
            details.push(format!(
                "MCP servers       {}",
                runtime_config.mcp().valid_count()
            ));
            if runtime_config.mcp().invalid_count() > 0 {
                details.push(format!(
                    "MCP invalid       {}",
                    runtime_config.mcp().invalid_count()
                ));
            }
            if present_paths.is_empty() {
                details.push("Discovered files  <none> (defaults active)".to_string());
            } else {
                details.extend(
                    present_paths
                        .iter()
                        .map(|path| format!("Discovered file   {path}")),
                );
            }
            DiagnosticCheck::new(
                "Config",
                DiagnosticLevel::Ok,
                if present_count == 0 {
                    "no config files present; defaults are active"
                } else {
                    "runtime config loaded successfully"
                },
            )
            .with_details(details)
            .with_data(Map::from_iter([
                ("discovered_files".to_string(), json!(present_paths)),
                ("discovered_files_count".to_string(), json!(present_count)),
                ("loaded_config_files".to_string(), json!(loaded_count)),
                ("resolved_model".to_string(), json!(runtime_config.model())),
                (
                    "mcp_servers".to_string(),
                    json!(runtime_config.mcp().valid_count()),
                ),
                (
                    "mcp_invalid_servers".to_string(),
                    json!(runtime_config.mcp().invalid_count()),
                ),
                (
                    "hook_invalid_entries".to_string(),
                    json!(runtime_config.hooks().invalid_count()),
                ),
            ]))
        }
        Err(error) => DiagnosticCheck::new(
            "Config",
            DiagnosticLevel::Fail,
            format!("runtime config failed to load: {error}"),
        )
        .with_details(if discovered_paths.is_empty() {
            vec!["Discovered files  <none>".to_string()]
        } else {
            discovered_paths
                .iter()
                .map(|path| format!("Discovered file   {path}"))
                .collect()
        })
        .with_hint("Fix the JSON syntax error in the listed config file, then rerun `claw doctor`.")
        .with_data(Map::from_iter([
            ("discovered_files".to_string(), json!(discovered_paths)),
            (
                "discovered_files_count".to_string(),
                json!(discovered_count),
            ),
            ("loaded_config_files".to_string(), json!(0)),
            ("resolved_model".to_string(), Value::Null),
            ("mcp_servers".to_string(), Value::Null),
            ("load_error".to_string(), json!(error.to_string())),
        ])),
    }
}

pub fn check_hook_validation_health(summary: &HookValidationSummary) -> DiagnosticCheck {
    let mut details = vec![
        format!("Valid entries     {}", summary.valid_count),
        format!("Invalid entries   {}", summary.invalid_count()),
    ];
    details.extend(
        summary
            .invalid_hooks
            .iter()
            .map(|hook| format!("Invalid hook     {} ({})", hook.event, hook.reason)),
    );

    DiagnosticCheck::new(
        "Hook validation",
        if summary.has_invalid_hooks() {
            DiagnosticLevel::Warn
        } else {
            DiagnosticLevel::Ok
        },
        if summary.has_invalid_hooks() {
            format!(
                "{} hook entries are invalid; {} valid entries remain loaded",
                summary.invalid_count(),
                summary.valid_count
            )
        } else {
            format!("{} hook entries validated", summary.valid_count)
        },
    )
    .with_hint(if summary.has_invalid_hooks() {
        "Inspect `claw status --output-format json` hook_validation.invalid_hooks and fix each rejected hooks entry."
    } else {
        ""
    })
    .with_details(details)
    .with_data(Map::from_iter([
        ("valid_count".to_string(), json!(summary.valid_count)),
        ("invalid_count".to_string(), json!(summary.invalid_count())),
        (
            "invalid_hooks".to_string(),
            Value::Array(invalid_hooks_json(&summary.invalid_hooks)),
        ),
    ]))
}

pub fn check_permission_health(permission_mode: PermissionModeProvenance) -> DiagnosticCheck {
    let mode = permission_mode.mode.as_str();
    let source = permission_mode.source.as_str();
    let explicit = permission_mode.source.is_explicit();
    let warning = matches!(permission_mode.mode, PermissionMode::DangerFullAccess) && !explicit;
    let message = if warning {
        "running with full access without explicit opt-in"
    } else if matches!(permission_mode.mode, PermissionMode::DangerFullAccess) {
        "danger-full-access was explicitly selected"
    } else if matches!(permission_mode.mode, PermissionMode::WorkspaceWrite) && !explicit {
        "default permission mode is workspace-write"
    } else {
        "permission mode is explicitly bounded below danger-full-access"
    };
    let source_detail = permission_mode.env_var.map_or_else(
        || source.to_string(),
        |env_var| format!("{source}:{env_var}"),
    );
    let specs = mvp_tool_specs();
    let tools_satisfied = specs
        .iter()
        .filter(|spec| permission_mode.mode >= spec.required_permission)
        .map(|spec| spec.name)
        .collect::<Vec<_>>();
    let tools_gated = specs
        .iter()
        .filter(|spec| permission_mode.mode < spec.required_permission)
        .map(|spec| spec.name)
        .collect::<Vec<_>>();

    DiagnosticCheck::new(
        "Permissions",
        if warning {
            DiagnosticLevel::Warn
        } else {
            DiagnosticLevel::Ok
        },
        message,
    )
    .with_details(vec![
        format!("Mode             {mode}"),
        format!("Source           {source_detail}"),
        format!("Explicit opt-in  {explicit}"),
        format!("Tools allowed    {}", tools_satisfied.join(", ")),
        format!("Tools gated      {}", tools_gated.join(", ")),
    ])
    .with_hint(if warning {
        "Use the workspace-write default, or pass --permission-mode danger-full-access / --dangerously-skip-permissions only when full filesystem, network, and command access is intentional."
    } else {
        "Use --permission-mode read-only|workspace-write|danger-full-access to make the runtime permission boundary explicit."
    })
    .with_data(Map::from_iter([
        ("mode".to_string(), json!(mode)),
        ("source".to_string(), json!(source)),
        ("source_explicit".to_string(), json!(explicit)),
        ("env_var".to_string(), json!(permission_mode.env_var)),
        ("message".to_string(), json!(message)),
        ("tools_satisfied".to_string(), json!(tools_satisfied)),
        ("tools_gated".to_string(), json!(tools_gated)),
    ]))
}

pub fn check_install_source_health() -> DiagnosticCheck {
    DiagnosticCheck::new(
        "Install source",
        DiagnosticLevel::Ok,
        format!(
            "official source of truth is {OFFICIAL_REPO_SLUG}; avoid `{DEPRECATED_INSTALL_COMMAND}`"
        ),
    )
    .with_details(vec![
        format!("Official repo     {OFFICIAL_REPO_URL}"),
        "Recommended path  build from this repo or use the upstream binary documented in README.md"
            .to_string(),
        format!(
            "Deprecated crate  `{DEPRECATED_INSTALL_COMMAND}` installs a deprecated stub and does not provide the `claw` binary"
        )
            .to_string(),
    ])
    .with_data(Map::from_iter([
        ("official_repo".to_string(), json!(OFFICIAL_REPO_URL)),
        (
            "deprecated_install".to_string(),
            json!(DEPRECATED_INSTALL_COMMAND),
        ),
        (
            "recommended_install".to_string(),
            json!("build from source or follow the upstream binary instructions in README.md"),
        ),
    ]))
}

pub fn check_workspace_health(context: &StatusContext) -> DiagnosticCheck {
    let in_repo = context.project_root.is_some();
    let stale_base_warning = format_stale_base_warning(&context.stale_base_state);
    DiagnosticCheck::new(
        "Workspace",
        if in_repo && stale_base_warning.is_none() {
            DiagnosticLevel::Ok
        } else {
            DiagnosticLevel::Warn
        },
        if in_repo {
            format!(
                "project root detected on branch {}",
                context.git_branch.as_deref().unwrap_or("unknown")
            )
        } else {
            "current directory is not inside a git project".to_string()
        },
    )
    .with_hint(if !in_repo {
        "Run `git init` to initialise a repository, or `cd` into a git project."
    } else if stale_base_warning.is_some() {
        "Rebase or merge to bring the branch up to date with its base."
    } else {
        ""
    })
    .with_details(vec![
        format!("Cwd              {}", context.cwd.display()),
        format!(
            "Project root     {}",
            context
                .project_root
                .as_ref()
                .map_or_else(|| "<none>".to_string(), |path| path.display().to_string())
        ),
        format!(
            "Git branch       {}",
            context.git_branch.as_deref().unwrap_or("unknown")
        ),
        format!(
            "Git state        {}",
            if context.project_root.is_some() {
                context.git_summary.headline()
            } else {
                "no git repo".to_string()
            }
        ),
        format!("Changed files    {}", context.git_summary.changed_files),
        format!(
            "Memory files     {} · config files loaded {}/{}",
            context.memory_file_count, context.loaded_config_files, context.discovered_config_files
        ),
        format!(
            "Loaded memory    {}",
            if context.memory_files.is_empty() {
                "<none>".to_string()
            } else {
                context
                    .memory_files
                    .iter()
                    .map(|file| format!("{}:{}", file.source, file.path))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ),
        format!(
            "Stale base      {}",
            stale_base_warning.as_deref().unwrap_or("ok")
        ),
    ])
    .with_data(Map::from_iter([
        ("cwd".to_string(), json!(context.cwd.display().to_string())),
        (
            "project_root".to_string(),
            json!(context
                .project_root
                .as_ref()
                .map(|path| path.display().to_string())),
        ),
        ("in_git_repo".to_string(), json!(in_repo)),
        ("git_branch".to_string(), json!(context.git_branch)),
        (
            "git_state".to_string(),
            json!(if context.project_root.is_some() {
                context.git_summary.headline()
            } else {
                "no_git_repo".to_string()
            }),
        ),
        (
            "changed_files".to_string(),
            json!(context.git_summary.changed_files),
        ),
        (
            "memory_file_count".to_string(),
            json!(context.memory_file_count),
        ),
        (
            "memory_files".to_string(),
            Value::Array(memory_files_json(&context.memory_files)),
        ),
        (
            "unloaded_memory_files".to_string(),
            json!(context.unloaded_memory_files),
        ),
        (
            "loaded_config_files".to_string(),
            json!(context.loaded_config_files),
        ),
        (
            "discovered_config_files".to_string(),
            json!(context.discovered_config_files),
        ),
        (
            "stale_base".to_string(),
            stale_base_json_value(&context.stale_base_state),
        ),
    ]))
}

pub fn check_memory_health(context: &StatusContext) -> DiagnosticCheck {
    let has_unloaded = !context.unloaded_memory_files.is_empty();
    let has_outside_project = context.memory_files.iter().any(|file| file.outside_project);
    let mut details = vec![format!("Loaded files     {}", context.memory_file_count)];
    details.extend(context.memory_files.iter().map(|file| {
        format!(
            "Loaded          {} ({}, chars={})",
            file.path, file.source, file.chars
        )
    }));
    details.extend(
        context
            .unloaded_memory_files
            .iter()
            .map(|path| format!("Unloaded        {path}")),
    );

    DiagnosticCheck::new(
        "Memory",
        if has_unloaded || has_outside_project {
            DiagnosticLevel::Warn
        } else {
            DiagnosticLevel::Ok
        },
        if has_outside_project {
            "memory files outside the current git project are loaded".to_string()
        } else if has_unloaded {
            "some workspace memory files exist but were not loaded".to_string()
        } else {
            format!("{} workspace memory files loaded", context.memory_file_count)
        },
    )
    .with_hint(if has_outside_project {
        "Inspect workspace.memory_files in `claw status --output-format json`; move unintended ancestor instructions inside the git project or run from the intended workspace root."
    } else if has_unloaded {
        "Move instructions into CLAUDE.md, CLAW.md, or AGENTS.md within the current workspace ancestry, or inspect workspace.memory_files in `claw status --output-format json`."
    } else {
        ""
    })
    .with_details(details)
    .with_data(Map::from_iter([
        (
            "memory_file_count".to_string(),
            json!(context.memory_file_count),
        ),
        (
            "memory_files".to_string(),
            Value::Array(memory_files_json(&context.memory_files)),
        ),
        (
            "unloaded_memory_files".to_string(),
            json!(context.unloaded_memory_files),
        ),
    ]))
}

pub fn check_boot_preflight_health(context: &StatusContext) -> DiagnosticCheck {
    let preflight = &context.boot_preflight;
    let missing_binaries = preflight
        .required_binaries
        .iter()
        .filter(|binary| !binary.available)
        .map(|binary| binary.name)
        .collect::<Vec<_>>();
    let socket_details = preflight
        .control_sockets
        .iter()
        .map(|socket| {
            format!(
                "Control socket  {} configured={} exists={} path={}",
                socket.name,
                socket.configured,
                socket.exists,
                socket.path.as_deref().unwrap_or("<none>")
            )
        })
        .collect::<Vec<_>>();
    let mut details = vec![
        format!("Repo exists      {}", preflight.repo_exists),
        format!("Worktree exists  {}", preflight.worktree_exists),
        format!("Git dir exists   {}", preflight.git_dir_exists),
        format!("Branch behind    {}", preflight.branch_freshness.behind),
        format!(
            "Trust allowlist  {}",
            preflight
                .trust_gate_allowed
                .map_or("unknown".to_string(), |v| v.to_string())
        ),
        format!("Trusted roots    {}", preflight.trusted_roots_count),
        // #736: keep compound values readable but use " · " as intra-value separator
        // so the two-space prose splitter yields key="MCP eligible" value="true · servers 0"
        format!(
            "MCP eligible     {}",
            format!(
                "{}  ·  servers {}",
                preflight.mcp_startup_eligible, preflight.mcp_servers_configured
            )
        ),
        format!(
            "Plugin eligible  {}",
            format!(
                "{}  ·  configured {}",
                preflight.plugin_startup_eligible, preflight.plugins_configured
            )
        ),
        format!(
            // #736: use two-space separator so the detail_entries prose splitter
            // can extract key="Last failed boot" value="<none>|<reason>"
            "Last failed boot  {}",
            preflight
                .last_failed_boot_reason
                .as_deref()
                .unwrap_or("<none>")
        ),
    ];
    details.extend(preflight.required_binaries.iter().map(|binary| {
        format!(
            // #736: two-space separator → key="Required binary <name>" value="available=true|false"
            "Required binary {}  available={}",
            binary.name, binary.available
        )
    }));
    details.extend(socket_details);
    DiagnosticCheck::new(
        "Boot preflight",
        if preflight.repo_exists && preflight.worktree_exists && missing_binaries.is_empty() {
            DiagnosticLevel::Ok
        } else {
            DiagnosticLevel::Warn
        },
        preflight.summary(),
    )
    .with_details(details)
    .with_hint(
        // #778: stable remediation hint for automation
        if !preflight.repo_exists || !preflight.worktree_exists {
            "Ensure you are inside a git worktree (`git init` or `git worktree add`)."
        } else if !missing_binaries.is_empty() {
            "Install the listed missing required binaries."
        } else {
            ""
        },
    )
    .with_data(Map::from_iter([(
        "boot_preflight".to_string(),
        preflight.json_value(),
    )]))
}

pub fn check_sandbox_health(status: &runtime::SandboxStatus) -> DiagnosticCheck {
    let degraded = status.enabled && !status.active;
    let mut details = vec![
        format!("Enabled          {}", status.enabled),
        format!("Active           {}", status.active),
        format!("Supported        {}", status.supported),
        format!("Filesystem mode  {}", status.filesystem_mode.as_str()),
        format!("Filesystem live  {}", status.filesystem_active),
    ];
    if let Some(reason) = &status.fallback_reason {
        details.push(format!("Fallback reason  {reason}"));
    }
    DiagnosticCheck::new(
        "Sandbox",
        if degraded {
            DiagnosticLevel::Warn
        } else {
            DiagnosticLevel::Ok
        },
        if degraded {
            "sandbox was requested but is not currently active"
        } else if status.active {
            "sandbox protections are active"
        } else {
            "sandbox is not active for this session"
        },
    )
    .with_details(details)
    .with_hint(
        // #778: stable remediation hint — sandbox degraded on non-Linux hosts is expected, not an error
        if degraded && !status.supported {
            "Sandbox namespace isolation requires Linux with `unshare`. On macOS/non-Linux hosts this warning is expected and can be ignored. Filesystem isolation is still active."
        } else if degraded {
            "Check that the `unshare` binary is available and the process has the required capabilities."
        } else {
            ""
        },
    )
    .with_data(Map::from_iter([
        ("enabled".to_string(), json!(status.enabled)),
        ("active".to_string(), json!(status.active)),
        ("supported".to_string(), json!(status.supported)),
        (
            "namespace_supported".to_string(),
            json!(status.namespace_supported),
        ),
        (
            "namespace_active".to_string(),
            json!(status.namespace_active),
        ),
        (
            "network_supported".to_string(),
            json!(status.network_supported),
        ),
        ("network_active".to_string(), json!(status.network_active)),
        (
            "filesystem_mode".to_string(),
            json!(status.filesystem_mode.as_str()),
        ),
        (
            "filesystem_active".to_string(),
            json!(status.filesystem_active),
        ),
        ("allowed_mounts".to_string(), json!(status.allowed_mounts)),
        ("in_container".to_string(), json!(status.in_container)),
        (
            "container_markers".to_string(),
            json!(status.container_markers),
        ),
        ("fallback_reason".to_string(), json!(status.fallback_reason)),
    ]))
}

pub fn check_system_health(cwd: &Path, config: Option<&runtime::RuntimeConfig>) -> DiagnosticCheck {
    let default_model = config.and_then(runtime::RuntimeConfig::model);
    let mut details = vec![
        format!(
            "OS               {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!("Working dir      {}", cwd.display()),
        format!("Version          {}", VERSION),
        format!("Build target     {}", BUILD_TARGET.unwrap_or("<unknown>")),
        format!("Git SHA          {}", GIT_SHA.unwrap_or("<unknown>")),
        format!(
            "Output format env  CLAW_OUTPUT_FORMAT={}",
            std::env::var("CLAW_OUTPUT_FORMAT").unwrap_or_else(|_| "<unset>".to_string())
        ),
        format!(
            "Logging env      CLAW_LOG={} RUST_LOG={}",
            std::env::var("CLAW_LOG").unwrap_or_else(|_| "<unset>".to_string()),
            std::env::var("RUST_LOG").unwrap_or_else(|_| "<unset>".to_string())
        ),
    ];
    if let Some(model) = default_model {
        details.push(format!("Default model    {model}"));
    }
    let binary_provenance = binary_provenance_for(Some(cwd));
    details.push(format!(
        "Binary provenance  status={} workspace_match={}",
        binary_provenance.status(),
        binary_provenance
            .workspace_match
            .map_or_else(|| "unknown".to_string(), |matches| matches.to_string())
    ));
    DiagnosticCheck::new(
        "System",
        DiagnosticLevel::Ok,
        "captured local runtime metadata",
    )
    .with_details(details)
    .with_data(Map::from_iter([
        ("os".to_string(), json!(std::env::consts::OS)),
        ("arch".to_string(), json!(std::env::consts::ARCH)),
        ("working_dir".to_string(), json!(cwd.display().to_string())),
        ("version".to_string(), json!(VERSION)),
        ("build_target".to_string(), json!(BUILD_TARGET)),
        ("git_sha".to_string(), json!(GIT_SHA)),
        (
            "binary_provenance".to_string(),
            binary_provenance.json_value(),
        ),
        ("default_model".to_string(), json!(default_model)),
        (
            "claw_output_format".to_string(),
            json!(std::env::var("CLAW_OUTPUT_FORMAT").ok()),
        ),
        (
            "claw_log".to_string(),
            json!(std::env::var("CLAW_LOG").ok()),
        ),
        (
            "rust_log".to_string(),
            json!(std::env::var("RUST_LOG").ok()),
        ),
    ]))
}
