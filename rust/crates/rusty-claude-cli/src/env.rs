use crate::{
    config_model_for_current_dir, resolve_model_alias_with_config, validate_model_syntax,
    DEFAULT_MODEL,
};
use runtime::PermissionMode;
use std::env;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
    /// Explicit `--model` / `--model=` CLI flag.
    Flag,
    /// Runtime model environment variable (when no flag was passed).
    Env,
    /// `model` key in `.claw.json` / `.claw/settings.json` (when neither
    /// flag nor env set it).
    Config,
    /// Compiled-in `DEFAULT_MODEL` fallback.
    Default,
}

impl ModelSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelSource::Flag => "flag",
            ModelSource::Env => "env",
            ModelSource::Config => "config",
            ModelSource::Default => "default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProvenance {
    /// Resolved model string (after alias expansion).
    pub resolved: String,
    /// Raw user input before alias resolution. None when source is Default.
    pub raw: Option<String>,
    /// Where the resolved model string originated.
    pub source: ModelSource,
    /// Alias-expanded target when `raw` differs from `resolved`.
    pub alias_resolved_to: Option<String>,
    /// Environment variable that supplied the model, when source is Env.
    pub env_var: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionModeSource {
    Flag,
    Env,
    Config,
    Default,
}

impl PermissionModeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::Config => "config",
            Self::Default => "default",
        }
    }

    pub fn is_explicit(self) -> bool {
        !matches!(self, Self::Default)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionModeProvenance {
    pub mode: PermissionMode,
    pub source: PermissionModeSource,
    pub env_var: Option<&'static str>,
}

impl PermissionModeProvenance {
    pub fn from_flag(mode: PermissionMode) -> Self {
        Self {
            mode,
            source: PermissionModeSource::Flag,
            env_var: None,
        }
    }

    pub fn default_fallback() -> Self {
        Self {
            mode: PermissionMode::WorkspaceWrite,
            source: PermissionModeSource::Default,
            env_var: None,
        }
    }
}

pub struct EnvModel {
    pub name: &'static str,
    pub value: String,
}

impl ModelProvenance {
    pub fn default_fallback() -> Self {
        Self {
            resolved: DEFAULT_MODEL.to_string(),
            raw: None,
            source: ModelSource::Default,
            alias_resolved_to: None,
            env_var: None,
        }
    }

    pub fn from_flag(raw: &str, resolved: &str) -> Self {
        Self::from_resolved(raw, resolved, ModelSource::Flag, None)
    }

    pub fn from_raw(raw: &str, source: ModelSource, env_var: Option<&str>) -> Self {
        let resolved = resolve_model_alias_with_config(raw);
        Self::from_resolved(raw, &resolved, source, env_var)
    }

    pub fn from_resolved(
        raw: &str,
        resolved: &str,
        source: ModelSource,
        env_var: Option<&str>,
    ) -> Self {
        let raw_trimmed = raw.trim();
        let alias_resolved_to = (raw_trimmed != resolved).then(|| resolved.to_string());
        Self {
            resolved: resolved.to_string(),
            raw: Some(raw.to_string()),
            source,
            alias_resolved_to,
            env_var: env_var.map(str::to_string),
        }
    }

    pub fn from_env_or_config_or_default(cli_model: &str) -> Result<Self, String> {
        // Only called when no --model flag was passed. Probe env first,
        // then config, else fall back to default. Mirrors the logic in
        // resolve_repl_model() but captures the source.
        if cli_model != DEFAULT_MODEL {
            let provenance = Self::from_resolved(cli_model, cli_model, ModelSource::Flag, None);
            provenance.validate()?;
            return Ok(provenance);
        }
        if let Some(env_model) = env_model_for_runtime() {
            let provenance =
                Self::from_raw(&env_model.value, ModelSource::Env, Some(env_model.name));
            provenance.validate()?;
            return Ok(provenance);
        }
        if let Some(config_model) = config_model_for_current_dir() {
            let provenance = Self::from_raw(&config_model, ModelSource::Config, None);
            provenance.validate()?;
            return Ok(provenance);
        }
        Ok(Self::default_fallback())
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_model_syntax(&self.resolved).map_err(|error| {
            let source = match self.source {
                ModelSource::Flag => "--model",
                ModelSource::Env => self.env_var.as_deref().unwrap_or("environment"),
                ModelSource::Config => "config model",
                ModelSource::Default => "default model",
            };
            if let Some(raw) = &self.raw {
                format!(
                    "invalid_model: {source} model `{raw}` is invalid after alias resolution to `{}`.\n{error}",
                    self.resolved
                )
            } else {
                error
            }
        })
    }
}

pub fn env_model_for_runtime() -> Option<EnvModel> {
    ["CLAW_MODEL", "ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_MODEL"]
        .into_iter()
        .find_map(|name| {
            env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(|value| EnvModel { name, value })
        })
}

pub fn max_tokens_for_model(model: &str) -> u32 {
    api::max_tokens_for_model(model)
}
