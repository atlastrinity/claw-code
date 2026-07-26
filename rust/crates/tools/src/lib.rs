// ─── External crate imports ─────────────────────────────────────────────────
use runtime::{
    lsp_client::LspRegistry,
    mcp::mcp_tool_bridge::McpToolRegistry,
    task_registry::TaskRegistry,
    team_cron_registry::{CronRegistry, TeamRegistry},
    worker_boot::WorkerRegistry,
};

// ─── Existing modules (unchanged) ──────────────────────────────────────────
mod provider_pipeline;
pub use provider_pipeline::*;

pub mod task_graph;
pub use task_graph::*;

mod pipeline_error;
pub use pipeline_error::*;

pub mod normalization;
pub use normalization::canonical_allowed_tool_name;

pub mod lane_completion;
pub mod pdf_extract;

// ─── New modules (extracted from lib.rs) ────────────────────────────────────
pub(crate) mod util;

pub(crate) mod tool_types;
pub(crate) use tool_types::*;

pub mod registry;
pub use registry::*;

pub mod tool_specs;
pub use tool_specs::*;

pub mod execute;
pub use execute::*;

pub(crate) mod web;

pub mod skills;
pub use skills::*;

pub(crate) mod agent;
pub(crate) use agent::*;

pub(crate) mod runners;
pub(crate) use runners::*;

pub(crate) mod tool_search;
pub(crate) use tool_search::*;

pub(crate) mod config;
pub(crate) use config::*;

pub(crate) mod shell;
pub(crate) use shell::*;

// ─── Global registries ─────────────────────────────────────────────────────

/// Global LSP registry shared across tool invocations within a session.
pub(crate) fn global_lsp_registry() -> &'static LspRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<LspRegistry> = OnceLock::new();
    REGISTRY.get_or_init(LspRegistry::new)
}

pub(crate) fn global_mcp_registry() -> &'static McpToolRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<McpToolRegistry> = OnceLock::new();
    REGISTRY.get_or_init(McpToolRegistry::new)
}

pub(crate) fn global_team_registry() -> &'static TeamRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<TeamRegistry> = OnceLock::new();
    REGISTRY.get_or_init(TeamRegistry::new)
}

pub(crate) fn global_cron_registry() -> &'static CronRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<CronRegistry> = OnceLock::new();
    REGISTRY.get_or_init(CronRegistry::new)
}

pub(crate) fn global_task_registry() -> &'static TaskRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<TaskRegistry> = OnceLock::new();
    REGISTRY.get_or_init(TaskRegistry::new)
}

pub(crate) fn global_worker_registry() -> &'static WorkerRegistry {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<WorkerRegistry> = OnceLock::new();
    REGISTRY.get_or_init(WorkerRegistry::new)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
