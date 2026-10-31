pub type RuntimePluginStateBuildOutput = (
    Option<std::sync::Arc<std::sync::Mutex<RuntimeMcpState>>>,
    Vec<tools::RuntimeToolDefinition>,
);
use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::Mutex;

/// the same surface the in-process agent loop uses.
pub fn run_mcp_serve() -> Result<(), Box<dyn std::error::Error>> {
    let tools = mvp_tool_specs()
        .into_iter()
        .map(|spec| McpTool {
            name: spec.name.to_string(),
            description: Some(spec.description.to_string()),
            input_schema: Some(spec.input_schema),
            annotations: None,
            meta: None,
        })
        .collect();

    let spec = McpServerSpec {
        server_name: "claw".to_string(),
        server_version: VERSION.to_string(),
        tools,
        tool_handler: Box::new(execute_tool),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let mut server = McpServer::new(spec);
        server.run().await
    })?;
    Ok(())
}

pub fn check_mcp_validation_health(summary: &McpValidationSummary) -> DiagnosticCheck {
    let mut details = vec![
        format!("Total entries     {}", summary.total_configured),
        format!("Valid entries     {}", summary.valid_count),
        format!("Invalid entries   {}", summary.invalid_count()),
    ];
    details.extend(
        summary
            .invalid_servers
            .iter()
            .map(|server| format!("Invalid server   {} ({})", server.name, server.reason)),
    );

    DiagnosticCheck::new(
        "MCP validation",
        if summary.has_invalid_servers() {
            DiagnosticLevel::Warn
        } else {
            DiagnosticLevel::Ok
        },
        if summary.has_invalid_servers() {
            format!(
                "{} MCP server entries are invalid; {} valid entries remain loaded",
                summary.invalid_count(),
                summary.valid_count
            )
        } else {
            format!("{} MCP server entries validated", summary.valid_count)
        },
    )
    .with_hint(if summary.has_invalid_servers() {
        "Inspect `claw mcp list --output-format json` invalid_servers and fix each rejected mcpServers entry."
    } else {
        ""
    })
    .with_details(details)
    .with_data(Map::from_iter([
        (
            "total_configured".to_string(),
            json!(summary.total_configured),
        ),
        ("valid_count".to_string(), json!(summary.valid_count)),
        ("invalid_count".to_string(), json!(summary.invalid_count())),
        (
            "invalid_servers".to_string(),
            Value::Array(invalid_mcp_servers_json(&summary.invalid_servers)),
        ),
    ]))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpValidationSummary {
    pub total_configured: usize,
    pub valid_count: usize,
    pub invalid_servers: Vec<McpInvalidServerConfig>,
}

pub fn invalid_mcp_servers_json(
    invalid_servers: &[McpInvalidServerConfig],
) -> Vec<serde_json::Value> {
    invalid_servers
        .iter()
        .map(|server| {
            json!({
                "name": &server.name,
                "scope": config_source_json_value(server.scope),
                "path": server.path.display().to_string(),
                "error_field": &server.error_field,
                "reason": &server.reason,
                "valid": false,
            })
        })
        .collect()
}

pub struct RuntimeMcpState {
    runtime: tokio::runtime::Runtime,
    manager: McpServerManager,
    pending_servers: Vec<String>,
    degraded_report: Option<runtime::McpDegradedReport>,
    available_servers: std::collections::BTreeMap<String, runtime::ScopedMcpServerConfig>,
    /// Servers that failed to load, with how many times and why. A dead server
    /// costs a full spawn plus two protocol timeouts per attempt, and the agent
    /// will keep paying it while it believes one more retry is the fix. After
    /// MCP_LOAD_FAILURE_LIMIT attempts the state answers from this record
    /// instead of trying again, and re-arms once MCP_LOAD_RETRY_AFTER passes.
    load_failures: std::collections::BTreeMap<String, McpLoadFailure>,
}

const MCP_LOAD_FAILURE_LIMIT: u32 = 2;
const MCP_LOAD_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(600);

#[derive(Debug, Clone)]
struct McpLoadFailure {
    attempts: u32,
    last_attempt: std::time::Instant,
    reason: String,
}

#[derive(Debug, Deserialize)]
pub struct McpToolRequest {
    #[serde(rename = "qualifiedName")]
    pub qualified_name: Option<String>,
    pub tool: Option<String>,
    pub arguments: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct ListMcpResourcesRequest {
    pub server: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReadMcpResourceRequest {
    pub server: String,
    pub uri: String,
}

impl RuntimeMcpState {
    fn new(
        runtime_config: &runtime::RuntimeConfig,
    ) -> Result<Option<(Self, runtime::McpToolDiscoveryReport)>, Box<dyn std::error::Error>> {
        let mut manager = McpServerManager::from_runtime_config(runtime_config);
        let mut available_servers = runtime_config.available_mcp().servers().clone();
        for (name, config) in runtime_config.mcp().servers() {
            if !available_servers.contains_key(name) {
                available_servers.insert(name.clone(), config.clone());
            }
        }
        if manager.server_names().is_empty()
            && manager.unsupported_servers().is_empty()
            && available_servers.is_empty()
        {
            return Ok(None);
        }

        let runtime = tokio::runtime::Runtime::new()?;
        let discovery = runtime.block_on(manager.discover_tools_best_effort());
        let pending_servers = discovery
            .failed_servers
            .iter()
            .map(|failure| failure.server_name.clone())
            .chain(
                discovery
                    .unsupported_servers
                    .iter()
                    .map(|server| server.server_name.clone()),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let available_tools = discovery
            .tools
            .iter()
            .map(|tool| tool.qualified_name.clone())
            .collect::<Vec<_>>();
        let failed_server_names = pending_servers.iter().cloned().collect::<BTreeSet<_>>();
        let working_servers = manager
            .server_names()
            .into_iter()
            .filter(|server_name| !failed_server_names.contains(server_name))
            .collect::<Vec<_>>();
        let failed_servers =
            discovery
                .failed_servers
                .iter()
                .map(|failure| runtime::McpFailedServer {
                    server_name: failure.server_name.clone(),
                    phase: runtime::McpLifecyclePhase::ToolDiscovery,
                    error: runtime::McpErrorSurface::new(
                        runtime::McpLifecyclePhase::ToolDiscovery,
                        Some(failure.server_name.clone()),
                        failure.error.clone(),
                        std::collections::BTreeMap::from([(
                            "required".to_string(),
                            failure.required.to_string(),
                        )]),
                        true,
                    ),
                })
                .chain(discovery.unsupported_servers.iter().map(|server| {
                    runtime::McpFailedServer {
                        server_name: server.server_name.clone(),
                        phase: runtime::McpLifecyclePhase::ServerRegistration,
                        error: runtime::McpErrorSurface::new(
                            runtime::McpLifecyclePhase::ServerRegistration,
                            Some(server.server_name.clone()),
                            server.reason.clone(),
                            std::collections::BTreeMap::from([
                                (
                                    "transport".to_string(),
                                    format!("{:?}", server.transport).to_ascii_lowercase(),
                                ),
                                ("required".to_string(), server.required.to_string()),
                            ]),
                            false,
                        ),
                    }
                }))
                .collect::<Vec<_>>();
        let degraded_report = (!failed_servers.is_empty()).then(|| {
            runtime::McpDegradedReport::new(
                working_servers,
                failed_servers,
                available_tools.clone(),
                available_tools,
            )
        });

        Ok(Some((
            Self {
                runtime,
                manager,
                pending_servers,
                degraded_report,
                available_servers,
                load_failures: std::collections::BTreeMap::new(),
            },
            discovery,
        )))
    }

    pub fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.block_on(self.manager.shutdown())?;
        Ok(())
    }

    pub fn pending_servers(&self) -> Option<Vec<String>> {
        (!self.pending_servers.is_empty()).then(|| self.pending_servers.clone())
    }

    pub fn degraded_report(&self) -> Option<runtime::McpDegradedReport> {
        self.degraded_report.clone()
    }

    fn server_names(&self) -> Vec<String> {
        self.manager.server_names()
    }

    pub fn available_servers(
        &self,
    ) -> &std::collections::BTreeMap<String, runtime::ScopedMcpServerConfig> {
        &self.available_servers
    }

    pub fn loaded_servers(&self) -> Vec<String> {
        self.manager.server_names()
    }

    pub fn load_server(&mut self, name: &str) -> Result<Vec<runtime::ManagedMcpTool>, String> {
        // If the server is already loaded with tools, return the existing tools
        // instead of restarting the process. Prevents unnecessary restarts when
        // AI calls McpSearch(load_server) multiple times.
        if self.manager.server_names().contains(&name.to_string()) {
            let existing_tools = self.manager.tools_for_server(name);
            if !existing_tools.is_empty() {
                self.load_failures.remove(name);
                return Ok(existing_tools);
            }
        }

        let Some(config) = self.available_servers.get(name).cloned() else {
            let known = self
                .available_servers
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "Server '{name}' is not defined in availableMcpServers. Configured servers: \
                 [{known}]"
            ));
        };

        // A server that has already refused to come up is not going to change its
        // mind because the agent asked again; answer from the record instead of
        // paying another spawn and two protocol timeouts.
        if let Some(failure) = self.load_failures.get(name) {
            if failure.attempts >= MCP_LOAD_FAILURE_LIMIT
                && failure.last_attempt.elapsed() < MCP_LOAD_RETRY_AFTER
            {
                return Err(format!(
                    "MCP server `{name}` is unavailable: {} failed load attempts in a row. \
                     Last failure: {}",
                    failure.attempts, failure.reason
                ));
            }
        }

        let attempt = self.runtime.block_on(async {
            self.manager
                .load_and_discover_server(name.to_string(), config)
                .await
        });

        match attempt {
            Ok(tools) => {
                self.load_failures.remove(name);
                Ok(tools)
            }
            Err(error) => {
                let reason = error.to_string();
                let failure =
                    self.load_failures
                        .entry(name.to_string())
                        .or_insert(McpLoadFailure {
                            attempts: 0,
                            last_attempt: std::time::Instant::now(),
                            reason: reason.clone(),
                        });
                failure.attempts += 1;
                failure.last_attempt = std::time::Instant::now();
                failure.reason = reason.clone();
                Err(if failure.attempts >= MCP_LOAD_FAILURE_LIMIT {
                    format!(
                        "MCP server `{name}` is unavailable: {} failed load attempts in a row. \
                         Last failure: {reason}",
                        failure.attempts
                    )
                } else {
                    format!("MCP server `{name}` failed to load: {reason}")
                })
            }
        }
    }

    /// Whether `name` is a server the session was actually configured with.
    #[must_use]
    pub fn server_is_configured(&self, name: &str) -> bool {
        self.available_servers.contains_key(name)
    }

    /// Failed load attempts recorded for `name`, for callers that have to
    /// explain why a configured server is not reachable.
    #[must_use]
    pub fn load_failure_count(&self, name: &str) -> u32 {
        self.load_failures.get(name).map_or(0, |f| f.attempts)
    }

    fn extract_server_name(&self, qualified_tool_name: &str) -> Option<String> {
        if let Some(rest) = qualified_tool_name.strip_prefix("mcp__") {
            if let Some((server_name, _)) = rest.split_once("__") {
                return Some(server_name.to_string());
            }
        }
        for name in self.available_servers.keys() {
            if qualified_tool_name.contains(name) {
                return Some(name.clone());
            }
        }
        None
    }

    pub fn call_tool(
        &mut self,
        qualified_tool_name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<String, ToolError> {
        let first_result = self.runtime.block_on(
            self.manager
                .call_tool(qualified_tool_name, arguments.clone()),
        );

        let response = match first_result {
            Ok(resp) => resp,
            Err(first_err) => {
                if let Some(server_name) = self.extract_server_name(qualified_tool_name) {
                    if self.load_server(&server_name).is_ok() {
                        if let Ok(retry_resp) = self
                            .runtime
                            .block_on(self.manager.call_tool(qualified_tool_name, arguments))
                        {
                            retry_resp
                        } else {
                            return Err(ToolError::new(first_err.to_string()));
                        }
                    } else {
                        return Err(ToolError::new(first_err.to_string()));
                    }
                } else {
                    return Err(ToolError::new(first_err.to_string()));
                }
            }
        };

        if let Some(error) = response.error {
            return Err(ToolError::new(format!(
                "MCP tool `{qualified_tool_name}` returned JSON-RPC error: {} ({})",
                error.message, error.code
            )));
        }

        let result = response.result.ok_or_else(|| {
            ToolError::new(format!(
                "MCP tool `{qualified_tool_name}` returned no result payload"
            ))
        })?;
        serde_json::to_string_pretty(&result).map_err(|error| ToolError::new(error.to_string()))
    }

    pub fn list_resources_for_server(&mut self, server_name: &str) -> Result<String, ToolError> {
        let result = self
            .runtime
            .block_on(self.manager.list_resources(server_name))
            .map_err(|error| ToolError::new(error.to_string()))?;
        serde_json::to_string_pretty(&json!({
            "server": server_name,
            "resources": result.resources,
        }))
        .map_err(|error| ToolError::new(error.to_string()))
    }

    pub fn list_resources_for_all_servers(&mut self) -> Result<String, ToolError> {
        let mut resources = Vec::new();
        let mut failures = Vec::new();

        for server_name in self.server_names() {
            match self
                .runtime
                .block_on(self.manager.list_resources(&server_name))
            {
                Ok(result) => resources.push(json!({
                    "server": server_name,
                    "resources": result.resources,
                })),
                Err(error) => failures.push(json!({
                    "server": server_name,
                    "error": error.to_string(),
                })),
            }
        }

        if resources.is_empty() && !failures.is_empty() {
            let message = failures
                .iter()
                .filter_map(|failure| failure.get("error").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ToolError::new(message));
        }

        serde_json::to_string_pretty(&json!({
            "resources": resources,
            "failures": failures,
        }))
        .map_err(|error| ToolError::new(error.to_string()))
    }

    pub fn read_resource(&mut self, server_name: &str, uri: &str) -> Result<String, ToolError> {
        let result = self
            .runtime
            .block_on(self.manager.read_resource(server_name, uri))
            .map_err(|error| ToolError::new(error.to_string()))?;
        serde_json::to_string_pretty(&json!({
            "server": server_name,
            "contents": result.contents,
        }))
        .map_err(|error| ToolError::new(error.to_string()))
    }
}

pub fn build_runtime_mcp_state(
    runtime_config: &runtime::RuntimeConfig,
) -> Result<RuntimePluginStateBuildOutput, Box<dyn std::error::Error>> {
    let Some((mcp_state, discovery)) = RuntimeMcpState::new(runtime_config)? else {
        return Ok((None, Vec::new()));
    };

    let mut runtime_tools = discovery
        .tools
        .iter()
        .map(mcp_runtime_tool_definition)
        .collect::<Vec<_>>();
    if !mcp_state.server_names().is_empty() {
        runtime_tools.extend(mcp_wrapper_tool_definitions());
    }

    Ok((Some(Arc::new(Mutex::new(mcp_state))), runtime_tools))
}

pub fn mcp_runtime_tool_definition(tool: &runtime::ManagedMcpTool) -> RuntimeToolDefinition {
    RuntimeToolDefinition {
        name: tool.qualified_name.clone(),
        description: Some(
            tool.tool
                .description
                .clone()
                .unwrap_or_else(|| format!("Invoke MCP tool `{}`.", tool.qualified_name)),
        ),
        input_schema: tool
            .tool
            .input_schema
            .clone()
            .unwrap_or_else(|| json!({ "type": "object", "additionalProperties": true })),
        required_permission: permission_mode_for_mcp_tool(&tool.tool),
    }
}

pub fn mcp_wrapper_tool_definitions() -> Vec<RuntimeToolDefinition> {
    vec![
        RuntimeToolDefinition {
            name: "MCPTool".to_string(),
            description: Some(
                "Call a configured MCP tool by its qualified name and JSON arguments.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "qualifiedName": { "type": "string" },
                    "arguments": {}
                },
                "required": ["qualifiedName"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::DangerFullAccess,
        },
        RuntimeToolDefinition {
            name: "ListMcpResourcesTool".to_string(),
            description: Some(
                "List MCP resources from one configured server or from every connected server."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" }
                },
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
        RuntimeToolDefinition {
            name: "ReadMcpResourceTool".to_string(),
            description: Some("Read a specific MCP resource from a configured server.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "uri": { "type": "string" }
                },
                "required": ["server", "uri"],
                "additionalProperties": false
            }),
            required_permission: PermissionMode::ReadOnly,
        },
    ]
}

pub fn permission_mode_for_mcp_tool(tool: &McpTool) -> PermissionMode {
    let read_only = mcp_annotation_flag(tool, "readOnlyHint");
    let destructive = mcp_annotation_flag(tool, "destructiveHint");
    let open_world = mcp_annotation_flag(tool, "openWorldHint");

    if read_only && !destructive && !open_world {
        PermissionMode::ReadOnly
    } else if destructive || open_world {
        PermissionMode::DangerFullAccess
    } else {
        PermissionMode::WorkspaceWrite
    }
}

pub fn mcp_annotation_flag(tool: &McpTool, key: &str) -> bool {
    tool.annotations
        .as_ref()
        .and_then(|annotations| annotations.get(key))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

impl McpValidationSummary {
    pub fn from_collection(collection: &McpConfigCollection) -> Self {
        Self {
            total_configured: collection.total_configured(),
            valid_count: collection.valid_count(),
            invalid_servers: collection.invalid_servers().to_vec(),
        }
    }

    pub fn invalid_count(&self) -> usize {
        self.invalid_servers.len()
    }

    pub fn has_invalid_servers(&self) -> bool {
        !self.invalid_servers.is_empty()
    }

    pub fn json_value(&self) -> serde_json::Value {
        json!({
            "total_configured": self.total_configured,
            "valid_count": self.valid_count,
            "invalid_count": self.invalid_count(),
            "invalid_servers": invalid_mcp_servers_json(&self.invalid_servers),
        })
    }
}
