use crate::env::{PermissionModeProvenance, PermissionModeSource};
use runtime::{ConfigLoader, PermissionMode, ResolvedPermissionMode};

pub fn resolve_model_alias(model: &str) -> &str {
    match model {
        "opus" => "anthropic/claude-opus-4-7",
        "sonnet" => "anthropic/claude-sonnet-4-6",
        "haiku" => "anthropic/claude-haiku-4-5-20251213",
        _ => model,
    }
}

/// Resolve a model name through user-defined config aliases first, then fall
/// back to the built-in alias table. This is the entry point used wherever a
/// user-supplied model string is about to be dispatched to a provider.
pub fn resolve_model_alias_with_config(model: &str) -> String {
    let trimmed = model.trim();
    if let Some(resolved) = config_alias_for_current_dir(trimmed) {
        return resolve_model_alias(&resolved).to_string();
    }
    resolve_model_alias(trimmed).to_string()
}

/// Validate model syntax at parse time.
/// Accepts: known aliases (opus, sonnet, haiku) or provider/model pattern.
/// Rejects: empty, whitespace-only, strings with spaces, or invalid chars.
pub fn validate_model_syntax(model: &str) -> Result<(), String> {
    let trimmed = model.trim();
    // Ollama models use names like "qwen3:8b" that don't match provider/model
    // syntax. Skip strict validation when OLLAMA_HOST is configured.
    if std::env::var_os("OLLAMA_HOST").is_some() {
        if trimmed.is_empty() {
            return Err("invalid model syntax: model string cannot be empty.\nUsage: --model <model-name>  e.g. --model qwen3:8b".to_string());
        }
        return Ok(());
    }
    if trimmed.is_empty() {
        return Err("invalid model syntax: model string cannot be empty.\nUsage: --model <provider/model>  e.g. --model anthropic/claude-opus-4-7".to_string());
    }
    // Check for spaces (malformed)
    if trimmed.contains(' ') {
        return Err(format!(
            "invalid model syntax: '{}' contains spaces.\nUse provider/model format (e.g., anthropic/claude-opus-4-7) or a known alias.",
            trimmed
        ));
    }
    if is_bare_provider_model(trimmed) {
        return Ok(());
    }
    if is_local_openai_model_syntax(trimmed) {
        return Ok(());
    }
    // Check provider/model format: provider_id/model_id
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        // #154: hint if the model looks like it belongs to a different provider
        let mut err_msg = format!(
            "invalid model syntax: '{}'.\nExpected provider/model (e.g., anthropic/claude-opus-4-7)",
            trimmed
        );
        if trimmed.starts_with("gpt-") || trimmed.starts_with("gpt_") {
            err_msg.push_str("\nDid you mean `openai/");
            err_msg.push_str(trimmed);
            err_msg.push_str("`? (Requires OPENAI_API_KEY env var)");
        } else if trimmed.starts_with("qwen") && trimmed.contains(':') {
            err_msg.push_str("\nFor a local Ollama model, set `OPENAI_BASE_URL=http://127.0.0.1:11434/v1` before using tagged names like `");
            err_msg.push_str(trimmed);
            err_msg.push_str("`.");
        } else if trimmed.starts_with("qwen") {
            err_msg.push_str("\nDid you mean `qwen/");
            err_msg.push_str(trimmed);
            err_msg.push_str("`? (Requires DASHSCOPE_API_KEY env var)");
        } else if trimmed.starts_with("grok") {
            err_msg.push_str("\nDid you mean `xai/");
            err_msg.push_str(trimmed);
            err_msg.push_str("`? (Requires XAI_API_KEY env var)");
        }
        return Err(err_msg);
    }
    Ok(())
}

pub fn is_bare_provider_model(model: &str) -> bool {
    model.starts_with("claude-")
        || model.starts_with("gpt-")
        || model.starts_with("gemini-")
        || model.starts_with("grok-")
        || model.starts_with("kimi-")
        || model.starts_with("glm-")
}

pub fn is_local_openai_model_syntax(model: &str) -> bool {
    if let Some(rest) = model.strip_prefix("local/") {
        return !rest.is_empty() && rest.split('/').all(|segment| !segment.is_empty());
    }
    std::env::var_os("OPENAI_BASE_URL").is_some() && (model.contains(':') || model.contains('.'))
}

pub fn config_alias_for_current_dir(alias: &str) -> Option<String> {
    if alias.is_empty() {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;
    let loader = ConfigLoader::default_for(&cwd);
    let config = loader.load().ok()?;
    config.aliases().get(alias).cloned()
}

pub fn normalize_permission_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "default" | "plan" | "read-only" => Some("read-only"),
        "acceptEdits" | "auto" | "workspace-write" => Some("workspace-write"),
        "dontAsk" | "bypassPermissions" | "dangerFullAccess" | "danger-full-access" => {
            Some("danger-full-access")
        }
        _ => None,
    }
}

pub fn parse_permission_mode_arg(value: &str) -> Result<PermissionMode, String> {
    normalize_permission_mode(value)
        .ok_or_else(|| {
            format!(
                "invalid_permission_mode: unsupported permission mode '{value}'.\nUsage: --permission-mode read-only|workspace-write|danger-full-access"
            )
        })
        .map(permission_mode_from_label)
}

pub fn permission_mode_from_label(mode: &str) -> PermissionMode {
    match mode {
        "read-only" => PermissionMode::ReadOnly,
        "workspace-write" => PermissionMode::WorkspaceWrite,
        "danger-full-access" => PermissionMode::DangerFullAccess,
        other => panic!("unsupported permission mode label: {other}"),
    }
}

pub fn permission_mode_from_resolved(mode: ResolvedPermissionMode) -> PermissionMode {
    match mode {
        ResolvedPermissionMode::ReadOnly => PermissionMode::ReadOnly,
        ResolvedPermissionMode::WorkspaceWrite => PermissionMode::WorkspaceWrite,
        ResolvedPermissionMode::DangerFullAccess => PermissionMode::DangerFullAccess,
    }
}

pub fn default_permission_mode() -> PermissionMode {
    permission_mode_provenance_for_current_dir().mode
}

pub fn permission_mode_provenance_for_current_dir() -> PermissionModeProvenance {
    if let Some(mode) = std::env::var("RUSTY_CLAUDE_PERMISSION_MODE")
        .ok()
        .as_deref()
        .and_then(normalize_permission_mode)
        .map(permission_mode_from_label)
    {
        return PermissionModeProvenance {
            mode,
            source: PermissionModeSource::Env,
            env_var: Some("RUSTY_CLAUDE_PERMISSION_MODE"),
        };
    }

    if let Some(mode) = config_permission_mode_for_current_dir() {
        return PermissionModeProvenance {
            mode,
            source: PermissionModeSource::Config,
            env_var: None,
        };
    }

    PermissionModeProvenance::default_fallback()
}

pub fn config_permission_mode_for_current_dir() -> Option<PermissionMode> {
    let cwd = std::env::current_dir().ok()?;
    let loader = ConfigLoader::default_for(&cwd);
    loader
        .load()
        .ok()?
        .permission_mode()
        .map(permission_mode_from_resolved)
}

pub fn config_model_for_current_dir() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let loader = ConfigLoader::default_for(&cwd);
    loader.load().ok()?.model().map(ToOwned::to_owned)
}
