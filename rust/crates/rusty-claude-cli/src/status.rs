use crate::*;
use runtime::TokenUsage;
use serde_json::{json, Value as JsonValue};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct StatusContext {
    pub cwd: PathBuf,
    pub session_path: Option<PathBuf>,
    pub loaded_config_files: usize,
    pub discovered_config_files: usize,
    pub memory_file_count: usize,
    pub memory_files: Vec<MemoryFileSummary>,
    pub unloaded_memory_files: Vec<String>,
    pub project_root: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub git_summary: GitWorkspaceSummary,
    pub branch_freshness: BranchFreshness,
    pub stale_base_state: BaseCommitState,
    pub session_lifecycle: SessionLifecycleSummary,
    pub boot_preflight: BootPreflightSnapshot,
    pub sandbox_status: runtime::SandboxStatus,
    pub binary_provenance: BinaryProvenance,
    /// #143: when `.claw.json` (or another loaded config file) fails to parse,
    /// we capture the parse error here and still populate every field that
    /// doesn't depend on runtime config (workspace, git, sandbox defaults,
    /// discovery counts). Top-level JSON output then reports
    /// `status: "degraded"` so claws can distinguish "status ran but config
    /// is broken" from "status ran cleanly".
    pub config_load_error: Option<String>,
    /// #143: machine-readable kind for the config load error, derived from
    /// `classify_error_kind`. Included in JSON output alongside the human
    /// readable string so downstream claws can switch on the kind token
    /// instead of regex-scraping the prose.
    pub config_load_error_kind: Option<&'static str>,
    pub mcp_validation: McpValidationSummary,

    pub hook_validation: HookValidationSummary,
    /// #468: duplicate global flag occurrences for provenance reporting
    pub duplicate_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPreflight {
    pub name: &'static str,
    pub available: bool,
}

impl BinaryPreflight {
    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "name": self.name,
            "available": self.available,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlSocketPreflight {
    pub name: &'static str,
    pub configured: bool,
    pub exists: bool,
    pub path: Option<String>,
}

impl ControlSocketPreflight {
    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "name": self.name,
            "configured": self.configured,
            "exists": self.exists,
            "path": self.path,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootPreflightSnapshot {
    pub repo_exists: bool,
    pub worktree_exists: bool,
    pub git_dir_exists: bool,
    pub branch_freshness: BranchFreshness,
    pub trust_gate_allowed: Option<bool>,
    pub trusted_roots_count: usize,
    pub required_binaries: Vec<BinaryPreflight>,
    pub control_sockets: Vec<ControlSocketPreflight>,
    pub mcp_startup_eligible: bool,
    pub mcp_servers_configured: usize,
    pub plugin_startup_eligible: bool,
    pub plugins_configured: usize,
    pub last_failed_boot_reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct StatusUsage {
    pub message_count: usize,
    pub turns: u32,
    pub latest: TokenUsage,
    pub cumulative: TokenUsage,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLifecycleKind {
    RunningProcess,
    IdleShell,
    SavedOnly,
}

impl SessionLifecycleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunningProcess => "running_process",
            Self::IdleShell => "idle_shell",
            Self::SavedOnly => "saved_only",
        }
    }

    pub fn human_label(self) -> &'static str {
        match self {
            Self::RunningProcess => "running process",
            Self::IdleShell => "idle shell",
            Self::SavedOnly => "saved only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLifecycleSummary {
    pub kind: SessionLifecycleKind,
    pub pane_id: Option<String>,
    pub pane_command: Option<String>,
    pub pane_path: Option<PathBuf>,
    pub workspace_dirty: bool,
    pub abandoned: bool,
    // #326: all panes matching this workspace, not just the first one
    pub all_panes: Vec<TmuxPaneSnapshot>,
}

impl BootPreflightSnapshot {
    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "repo": {
                "exists": self.repo_exists,
                "worktree_exists": self.worktree_exists,
                "git_dir_exists": self.git_dir_exists,
            },
            "branch_freshness": self.branch_freshness.json_value(),
            "trust_gate": {
                "allowlisted": self.trust_gate_allowed,
                "trusted_roots_count": self.trusted_roots_count,
            },
            "required_binaries": self.required_binaries.iter().map(BinaryPreflight::json_value).collect::<Vec<_>>(),
            "control_sockets": self.control_sockets.iter().map(ControlSocketPreflight::json_value).collect::<Vec<_>>(),
            "mcp_startup": {
                "eligible": self.mcp_startup_eligible,
                "servers_configured": self.mcp_servers_configured,
            },
            "plugin_startup": {
                "eligible": self.plugin_startup_eligible,
                "plugins_configured": self.plugins_configured,
            },
            "last_failed_boot_reason": self.last_failed_boot_reason,
        })
    }
    pub fn summary(&self) -> String {
        let trust = self
            .trust_gate_allowed
            .map(|value| {
                if value {
                    "allowlisted"
                } else {
                    "not allowlisted"
                }
            })
            .unwrap_or("unknown");
        let freshness = self
            .branch_freshness
            .fresh
            .map(|fresh| if fresh { "fresh" } else { "behind" })
            .unwrap_or("no upstream");
        format!(
            "repo={} worktree={} branch={} trust={} mcp={} plugins={} last_failed={}",
            self.repo_exists,
            self.worktree_exists,
            freshness,
            trust,
            self.mcp_startup_eligible,
            self.plugin_startup_eligible,
            self.last_failed_boot_reason.as_deref().unwrap_or("none")
        )
    }
}

impl SessionLifecycleSummary {
    pub fn signal(&self) -> String {
        let mut parts = vec![self.kind.human_label().to_string()];
        if self.workspace_dirty {
            parts.push("dirty worktree".to_string());
        }
        if self.abandoned {
            parts.push("abandoned?".to_string());
        }
        if let Some(command) = self.pane_command.as_deref() {
            parts.push(format!("cmd={command}"));
        }
        parts.join(" · ")
    }

    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "kind": self.kind.as_str(),
            "pane_id": self.pane_id,
            "pane_command": self.pane_command,
            "pane_path": self.pane_path.as_ref().map(|path| path.display().to_string()),
            "workspace_dirty": self.workspace_dirty,
            "abandoned": self.abandoned,
            // #326: include all workspace panes in the JSON output
            "panes": self.all_panes.iter().map(|p| {
                json!({
                    "pane_id": p.pane_id,
                    "pane_command": p.current_command,
                    "pane_path": p.current_path.display().to_string(),
                })
            }).collect::<Vec<_>>(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFileSummary {
    pub path: String,
    pub source: String,
    pub chars: usize,
    pub origin: String,
    pub scope_path: String,
    pub outside_project: bool,
    pub contributes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryProvenance {
    pub git_sha: Option<String>,
    pub git_sha_short: Option<String>,
    pub is_dirty: bool,
    pub branch: Option<String>,
    pub commit_date: String,
    pub commit_timestamp: i64,
    pub rustc_version: String,
    pub target: Option<String>,
    pub build_date: String,
    pub executable_path: Option<String>,
    pub workspace_git_sha: Option<String>,
    pub workspace_match: Option<bool>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookValidationSummary {
    pub valid_count: usize,
    pub invalid_hooks: Vec<RuntimeInvalidHookConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneSnapshot {
    pub pane_id: String,
    pub current_command: String,
    pub current_path: PathBuf,
}

impl HookValidationSummary {
    pub fn from_config(config: &runtime::RuntimeConfig) -> Self {
        let hooks = config.hooks();
        Self {
            valid_count: hooks.pre_tool_use_entries().len()
                + hooks.post_tool_use_entries().len()
                + hooks.post_tool_use_failure_entries().len(),
            invalid_hooks: hooks.invalid_hooks().to_vec(),
        }
    }

    pub fn invalid_count(&self) -> usize {
        self.invalid_hooks.len()
    }

    pub fn has_invalid_hooks(&self) -> bool {
        !self.invalid_hooks.is_empty()
    }

    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "valid_count": self.valid_count,
            "invalid_count": self.invalid_count(),
            "invalid_hooks": invalid_hooks_json(&self.invalid_hooks),
        })
    }
}
