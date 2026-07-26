use std::collections::{BTreeMap, BTreeSet};

use plugins::PluginTool;
use serde_json::Value;
use runtime::{
    McpDegradedReport,
    PermissionMode,
    security::permission_enforcer::PermissionEnforcer,
    ContextBudget,
};

use api::ToolDefinition;
use crate::tool_types::*;
use crate::util::*;
use crate::tool_specs::mvp_tool_specs;
use crate::execute::execute_tool_with_enforcer;
use crate::normalization;
use crate::normalization::canonical_allowed_tool_name;
use crate::tool_search::{normalize_tool_search_query, search_tool_specs};
use crate::task_graph::validate_active_task_for_tool;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolManifestEntry {
    pub name: String,
    pub source: ToolSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    Base,
    Conditional,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolRegistry {
    entries: Vec<ToolManifestEntry>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new(entries: Vec<ToolManifestEntry>) -> Self {
        Self { entries }
    }

    #[must_use]
    pub fn entries(&self) -> &[ToolManifestEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub required_permission: PermissionMode,
}

#[derive(Debug, Clone)]
pub struct GlobalToolRegistry {
    plugin_tools: Vec<PluginTool>,
    runtime_tools: std::sync::Arc<std::sync::Mutex<Vec<RuntimeToolDefinition>>>,
    enforcer: Option<PermissionEnforcer>,
    pub injected_tools: std::sync::Arc<std::sync::Mutex<Option<BTreeSet<String>>>>,
    pub allowed_tools: std::sync::Arc<std::sync::Mutex<Option<BTreeSet<String>>>>,
    pub budget: ContextBudget,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub required_permission: PermissionMode,
}

impl GlobalToolRegistry {
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            plugin_tools: Vec::new(),
            runtime_tools: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            enforcer: None,
            injected_tools: std::sync::Arc::new(std::sync::Mutex::new(None)),
            allowed_tools: std::sync::Arc::new(std::sync::Mutex::new(None)),
            budget: ContextBudget::default_budget(),
        }
    }

    pub fn with_plugin_tools(plugin_tools: Vec<PluginTool>) -> Result<Self, String> {
        let builtin_names = mvp_tool_specs()
            .into_iter()
            .map(|spec| spec.name.to_string())
            .collect::<BTreeSet<_>>();
        let mut seen_plugin_names = BTreeSet::new();

        for tool in &plugin_tools {
            let name = tool.definition().name.clone();
            if builtin_names.contains(&name) {
                return Err(format!(
                    "plugin tool `{name}` conflicts with a built-in tool name"
                ));
            }
            if !seen_plugin_names.insert(name.clone()) {
                return Err(format!("duplicate plugin tool name `{name}`"));
            }
        }

        Ok(Self {
            plugin_tools,
            runtime_tools: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            enforcer: None,
            injected_tools: std::sync::Arc::new(std::sync::Mutex::new(None)),
            allowed_tools: std::sync::Arc::new(std::sync::Mutex::new(None)),
            budget: ContextBudget::default_budget(),
        })
    }

    #[must_use]
    pub fn with_injected_tools(self, allowed: Option<BTreeSet<String>>) -> Self {
        if let Ok(mut lock) = self.injected_tools.lock() {
            *lock = allowed;
        }
        self
    }

    #[must_use]
    pub fn with_allowed_tools(self, allowed: Option<BTreeSet<String>>) -> Self {
        if let Ok(mut lock) = self.allowed_tools.lock() {
            *lock = allowed;
        }
        self
    }

    pub fn with_runtime_tools(
        self,
        runtime_tools: Vec<RuntimeToolDefinition>,
    ) -> Result<Self, String> {
        let mut seen_names = mvp_tool_specs()
            .into_iter()
            .map(|spec| spec.name.to_string())
            .chain(
                self.plugin_tools
                    .iter()
                    .map(|tool| tool.definition().name.clone()),
            )
            .collect::<BTreeSet<_>>();

        for tool in &runtime_tools {
            if !seen_names.insert(tool.name.clone()) {
                return Err(format!(
                    "runtime tool `{}` conflicts with an existing tool name",
                    tool.name
                ));
            }
        }

        if let Ok(mut lock) = self.runtime_tools.lock() {
            *lock = runtime_tools;
        }
        Ok(self)
    }

    #[must_use]
    pub fn with_enforcer(mut self, enforcer: PermissionEnforcer) -> Self {
        self.set_enforcer(enforcer);
        self
    }

    #[must_use]
    pub fn with_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = budget;
        self
    }

    pub fn normalize_tool_list(
        &self,
        values: &[String],
        flag_name: &str,
    ) -> Result<Option<BTreeSet<String>>, String> {
        if values.is_empty() {
            return Ok(None);
        }

        let actual_names = self.actual_tool_names();
        let canonical_names = self.canonical_allowed_tool_names();
        let canonical_name_set = canonical_names.iter().cloned().collect::<BTreeSet<_>>();
        let mut name_map = BTreeMap::new();
        for actual in &actual_names {
            let canonical = canonical_allowed_tool_name(actual);
            name_map.insert(allowed_tool_lookup_key(actual), canonical.clone());
            name_map.insert(allowed_tool_lookup_key(&canonical), canonical);
        }

        for (alias, canonical) in self.allowed_tool_aliases() {
            if canonical_name_set.contains(&canonical) {
                name_map.insert(allowed_tool_lookup_key(&alias), canonical);
            }
        }

        let mut allowed = BTreeSet::new();
        for value in values {
            for token in value
                .split(|ch: char| ch == ',' || ch.is_whitespace())
                .filter(|token| !token.is_empty())
            {
                if token.starts_with("mcp_") {
                    allowed.insert(canonical_allowed_tool_name(token));
                    continue;
                }

                let canonical = name_map.get(&allowed_tool_lookup_key(token)).ok_or_else(|| {
                    format!(
                        "invalid_tool_name: unsupported tool in {flag_name}: {token}\nAvailable: {}\nAliases: {}\nHint: Use canonical snake_case tool names from Available or aliases from Aliases.",
                        canonical_names.join(", "),
                        format_allowed_tool_aliases(&self.allowed_tool_aliases())
                    )
                })?;
                allowed.insert(canonical.clone());
            }
        }

        if allowed.is_empty() {
            return Err(format!(
                "{flag_name} was provided with no usable tool names (got `{}`). Omit the flag to allow all tools.",
                values.join(" ")
            ));
        }

        Ok(Some(allowed))
    }

    pub fn is_tool_injected(&self, name: &str) -> bool {
        let canonical_name = canonical_allowed_tool_name(name);
        let lock = self.injected_tools.lock().unwrap();
        lock.is_none()
            || lock
                .as_ref()
                .is_some_and(|allowed| allowed.contains(&canonical_name))
    }

    pub fn is_tool_allowed(&self, name: &str) -> bool {
        let canonical_name = canonical_allowed_tool_name(name);
        let lock = self.allowed_tools.lock().unwrap();
        lock.is_none()
            || lock
                .as_ref()
                .is_some_and(|allowed| allowed.contains(&canonical_name))
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let builtin = mvp_tool_specs()
            .into_iter()
            .filter(|spec| self.is_tool_injected(spec.name))
            .map(|spec| ToolDefinition {
                name: spec.name.to_string(),
                description: Some(spec.description.to_string()),
                input_schema: spec.input_schema.clone(),
            });

        let plugins = self.plugin_tools.iter().filter_map(|tool| {
            if self.is_tool_injected(&tool.definition().name) {
                Some(ToolDefinition {
                    name: tool.definition().name.clone(),
                    description: tool.definition().description.clone(),
                    input_schema: tool.definition().input_schema.clone(),
                })
            } else {
                None
            }
        });

        let runtime_lock = self.runtime_tools.lock().unwrap();
        let runtime = runtime_lock.iter().filter_map(|tool| {
            if self.is_tool_injected(&tool.name) {
                Some(ToolDefinition {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.input_schema.clone(),
                })
            } else {
                None
            }
        }).collect::<Vec<_>>();

        builtin.chain(plugins).chain(runtime.into_iter()).collect()
    }

    pub fn permission_specs(
        &self,
        tools: Option<&BTreeSet<String>>,
    ) -> Result<Vec<(String, PermissionMode)>, String> {
        let builtin = mvp_tool_specs()
            .into_iter()
            .filter(|spec| {
                tools
                    .is_none_or(|allowed| allowed.contains(&canonical_allowed_tool_name(spec.name)))
            })
            .map(|spec| (spec.name.to_string(), spec.required_permission));

        let runtime_lock = self.runtime_tools.lock().unwrap();
        let runtime = runtime_lock
            .iter()
            .filter(|tool| {
                tools.is_none_or(|allowed| {
                    allowed.contains(&canonical_allowed_tool_name(&tool.name))
                })
            })
            .map(|tool| (tool.name.clone(), tool.required_permission))
            .collect::<Vec<_>>();

        let plugin = self
            .plugin_tools
            .iter()
            .filter(|tool| {
                tools.is_none_or(|allowed| {
                    allowed.contains(&canonical_allowed_tool_name(
                        tool.definition().name.as_str(),
                    ))
                })
            })
            .map(|tool| {
                permission_mode_from_plugin(tool.required_permission())
                    .map(|permission| (tool.definition().name.clone(), permission))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(builtin.chain(runtime.into_iter()).chain(plugin).collect())
    }

    #[must_use]
    pub fn actual_tool_names(&self) -> Vec<String> {
        let runtime_lock = self.runtime_tools.lock().unwrap();
        let runtime = runtime_lock.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>();
        mvp_tool_specs()
            .iter()
            .map(|spec| spec.name.to_string())
            .chain(
                self.plugin_tools
                    .iter()
                    .map(|tool| tool.definition().name.clone()),
            )
            .chain(runtime.into_iter())
            .collect()
    }

    pub fn register_dynamic_tools(&self, new_tools: Vec<RuntimeToolDefinition>) {
        if let Ok(mut runtime_tools) = self.runtime_tools.lock() {
            for tool in &new_tools {
                if !runtime_tools.iter().any(|t| t.name == tool.name) {
                    runtime_tools.push(tool.clone());
                }
            }
        }
        if let Ok(mut injected) = self.injected_tools.lock() {
            if let Some(set) = injected.as_mut() {
                for tool in &new_tools {
                    set.insert(canonical_allowed_tool_name(&tool.name));
                }
            }
        }
        if let Ok(mut allowed) = self.allowed_tools.lock() {
            if let Some(set) = allowed.as_mut() {
                for tool in &new_tools {
                    set.insert(canonical_allowed_tool_name(&tool.name));
                }
            }
        }
    }

    #[must_use]
    pub fn canonical_allowed_tool_names(&self) -> Vec<String> {
        self.actual_tool_names()
            .into_iter()
            .map(|name| canonical_allowed_tool_name(&name))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    #[must_use]
    pub fn allowed_tool_aliases(&self) -> BTreeMap<String, String> {
        let mut aliases = BTreeMap::from([
            ("read".to_string(), "read_file".to_string()),
            ("Read".to_string(), "read_file".to_string()),
            ("write".to_string(), "write_file".to_string()),
            ("Write".to_string(), "write_file".to_string()),
            ("edit".to_string(), "edit_file".to_string()),
            ("Edit".to_string(), "edit_file".to_string()),
            ("glob".to_string(), "glob_search".to_string()),
            ("Glob".to_string(), "glob_search".to_string()),
            ("grep".to_string(), "grep_search".to_string()),
            ("Grep".to_string(), "grep_search".to_string()),
        ]);
        for actual in self.actual_tool_names() {
            let canonical = canonical_allowed_tool_name(&actual);
            if actual != canonical {
                aliases.insert(actual, canonical);
            }
        }
        aliases
    }
    #[must_use]
    pub fn has_runtime_tool(&self, name: &str) -> bool {
        self.runtime_tools.lock().unwrap().iter().any(|tool| tool.name == name)
    }

    #[must_use]
    pub fn search(
        &self,
        query: &str,
        max_results: usize,
        pending_mcp_servers: Option<Vec<String>>,
        mcp_degraded: Option<McpDegradedReport>,
    ) -> ToolSearchOutput {
        let query = query.trim().to_string();
        let normalized_query = normalize_tool_search_query(&query);
        let matches = search_tool_specs(&query, max_results.max(1), &self.searchable_tool_specs());

        ToolSearchOutput {
            matches,
            query,
            normalized_query,
            total_deferred_tools: self.searchable_tool_specs().len(),
            pending_mcp_servers,
            mcp_degraded,
        }
    }

    pub fn set_enforcer(&mut self, enforcer: PermissionEnforcer) {
        self.enforcer = Some(enforcer);
    }

    pub fn execute(&self, raw_name: &str, input: &Value) -> Result<String, String> {
        if raw_name == "ToolSearch" {
            let search_input = serde_json::from_value::<ToolSearchInput>(input.clone())
                .map_err(|e| e.to_string())?;
            let output = self.search(
                &search_input.query,
                search_input.max_results.unwrap_or(5),
                None,
                None,
            );
            return to_pretty_json(output);
        }

        let coerced_input = normalization::coerce_tool_input(input.clone());

        // First try finding it exactly (ignoring case)
        if let Some(spec) = mvp_tool_specs()
            .iter()
            .find(|spec| spec.name.eq_ignore_ascii_case(raw_name))
        {
            return execute_tool_with_enforcer(self.enforcer.as_ref(), spec.name, &coerced_input, self.budget);
        }

        // Then try canonical names
        let resolved_name = self
            .allowed_tool_aliases()
            .get(raw_name)
            .cloned()
            .unwrap_or_else(|| normalization::canonical_allowed_tool_name(raw_name));

        if let Some(spec) = mvp_tool_specs()
            .iter()
            .find(|spec| normalization::canonical_allowed_tool_name(spec.name) == resolved_name)
        {
            return execute_tool_with_enforcer(self.enforcer.as_ref(), spec.name, &coerced_input, self.budget);
        }

        // Check plugin tools
        if let Some(tool) = self
            .plugin_tools
            .iter()
            .find(|tool| tool.definition().name.eq_ignore_ascii_case(raw_name))
        {
            // Plugin/MCP tools MUST also pass TaskGraph enforcement
            validate_active_task_for_tool(raw_name, &coerced_input)?;
            return tool.execute(&coerced_input).map_err(|e| e.to_string());
        }

        if let Some(tool) = self.plugin_tools.iter().find(|tool| {
            normalization::canonical_allowed_tool_name(&tool.definition().name) == resolved_name
        }) {
            // Plugin/MCP tools MUST also pass TaskGraph enforcement
            validate_active_task_for_tool(&resolved_name, &coerced_input)?;
            return tool.execute(&coerced_input).map_err(|e| e.to_string());
        }

        Err(format!("unsupported tool: {resolved_name}"))
    }

    fn searchable_tool_specs(&self) -> Vec<SearchableToolSpec> {
        let builtin = mvp_tool_specs()
            .into_iter()
            .filter(|spec| {
                let is_hardcoded_ignored = matches!(
                    spec.name,
                    "bash"
                        | "read_file"
                        | "write_file"
                        | "edit_file"
                        | "glob_search"
                        | "grep_search"
                );

                let is_injected = self.is_tool_injected(spec.name);

                !is_hardcoded_ignored && !is_injected
            })
            .map(|spec| SearchableToolSpec {
                name: spec.name.to_string(),
                description: spec.description.to_string(),
            });
        let runtime_lock = self.runtime_tools.lock().unwrap();
        let runtime = runtime_lock
            .iter()
            .filter(|tool| !self.is_tool_injected(&tool.name))
            .map(|tool| SearchableToolSpec {
                name: tool.name.clone(),
                description: tool.description.clone().unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let plugin = self
            .plugin_tools
            .iter()
            .filter(|tool| !self.is_tool_injected(&tool.definition().name))
            .map(|tool| SearchableToolSpec {
                name: tool.definition().name.clone(),
                description: tool.definition().description.clone().unwrap_or_default(),
            });
        builtin.chain(runtime.into_iter()).chain(plugin).collect()
    }
}

pub(crate) fn allowed_tool_lookup_key(value: &str) -> String {
    canonical_allowed_tool_name(value).replace('_', "")
}

pub(crate) fn format_allowed_tool_aliases(aliases: &BTreeMap<String, String>) -> String {
    aliases
        .iter()
        .map(|(alias, canonical)| format!("{alias}={canonical}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn permission_mode_from_plugin(value: &str) -> Result<PermissionMode, String> {
    match value {
        "read-only" => Ok(PermissionMode::ReadOnly),
        "workspace-write" => Ok(PermissionMode::WorkspaceWrite),
        "danger-full-access" => Ok(PermissionMode::DangerFullAccess),
        other => Err(format!("unsupported plugin permission: {other}")),
    }
}
