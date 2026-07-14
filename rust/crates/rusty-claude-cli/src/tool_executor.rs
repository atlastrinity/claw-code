use crate::mcp::{ListMcpResourcesRequest, McpToolRequest, ReadMcpResourceRequest};
use crate::{
    format_tool_result, GlobalToolRegistry, RuntimeMcpState, TerminalRenderer, ToolSearchRequest,
};
use serde::Deserialize;
use api::ToolResultContentBlock;
use runtime::{RuntimeError, ToolError, ToolExecutor};
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex};
pub struct CliToolExecutor {
    renderer: std::sync::Mutex<TerminalRenderer>,
    emit_output: bool,
    tool_registry: GlobalToolRegistry,
    mcp_state: Option<Arc<Mutex<RuntimeMcpState>>>,
}

impl CliToolExecutor {
    pub fn new(
        emit_output: bool,
        tool_registry: GlobalToolRegistry,
        mcp_state: Option<Arc<Mutex<RuntimeMcpState>>>,
    ) -> Self {
        Self {
            renderer: std::sync::Mutex::new(TerminalRenderer::new()),
            emit_output,
            tool_registry,
            mcp_state,
        }
    }
    fn execute_search_tool(&self, value: serde_json::Value) -> Result<String, ToolError> {
        let input: ToolSearchRequest = serde_json::from_value(value)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
        let (pending_mcp_servers, mcp_degraded) =
            self.mcp_state.as_ref().map_or((None, None), |state| {
                let state = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (state.pending_servers(), state.degraded_report())
            });
        serde_json::to_string_pretty(&self.tool_registry.search(
            &input.query,
            input.max_results.unwrap_or(5),
            pending_mcp_servers,
            mcp_degraded,
        ))
        .map_err(|error| ToolError::new(error.to_string()))
    }
    fn execute_runtime_tool(
        &self,
        tool_name: &str,
        value: serde_json::Value,
    ) -> Result<String, ToolError> {
        let Some(mcp_state) = &self.mcp_state else {
            return Err(ToolError::new(format!(
                "runtime tool `{tool_name}` is unavailable without configured MCP servers"
            )));
        };
        let mut mcp_state = mcp_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match tool_name {
            "MCPTool" => {
                let input: McpToolRequest = serde_json::from_value(value)
                    .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
                let qualified_name = input
                    .qualified_name
                    .or(input.tool)
                    .ok_or_else(|| ToolError::new("missing required field `qualifiedName`"))?;
                mcp_state.call_tool(&qualified_name, input.arguments)
            }
            "ListMcpResourcesTool" => {
                let input: ListMcpResourcesRequest = serde_json::from_value(value)
                    .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
                match input.server {
                    Some(server_name) => mcp_state.list_resources_for_server(&server_name),
                    None => mcp_state.list_resources_for_all_servers(),
                }
            }
            "ReadMcpResourceTool" => {
                let input: ReadMcpResourceRequest = serde_json::from_value(value)
                    .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
                mcp_state.read_resource(&input.server, &input.uri)
            }
            _ => mcp_state.call_tool(tool_name, Some(value)),
        }
    }

    fn execute_mcp_search_tool(&self, value: serde_json::Value) -> Result<String, ToolError> {
        #[derive(Debug, Deserialize)]
        struct McpSearchRequest {
            query: Option<String>,
            load_server: Option<String>,
        }
        let input: McpSearchRequest = serde_json::from_value(value)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;

        let Some(mcp_state) = &self.mcp_state else {
            return Err(ToolError::new("MCP state is not initialized"));
        };
        let mut state = mcp_state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(server_name) = input.load_server {
            let tools = state.load_server(&server_name)
                .map_err(|err| ToolError::new(format!("failed to load MCP server {server_name}: {err}")))?;

            let runtime_tools = tools
                .iter()
                .map(|t| crate::mcp::mcp_runtime_tool_definition(t))
                .collect::<Vec<_>>();
            self.tool_registry.register_dynamic_tools(runtime_tools);

            let tool_names = tools.iter().map(|t| t.qualified_name.clone()).collect::<Vec<_>>();
            return serde_json::to_string_pretty(&serde_json::json!({
                "status": "success",
                "message": format!("Successfully loaded MCP server '{}' with {} tools. You can now use the 'Skill' tool to load any paired instructions or documentation for this MCP server.", server_name, tools.len()),
                "tools": tool_names
            })).map_err(|error| ToolError::new(error.to_string()));
        }

        let query = input.query.unwrap_or_default().to_lowercase();
        let available = state.available_servers();
        let loaded = state.loaded_servers();

        let mut results = Vec::new();
        for (name, config) in available {
            if name.to_lowercase().contains(&query) || format!("{:?}", config.transport()).to_lowercase().contains(&query) {
                let is_loaded = loaded.contains(name);
                results.push(serde_json::json!({
                    "name": name,
                    "description": config.description.clone(),
                    "loaded": is_loaded,
                    "required": config.required,
                    "transport": format!("{:?}", config.transport())
                }));
            }
        }

        serde_json::to_string_pretty(&serde_json::json!({
            "query": query,
            "servers": results
        })).map_err(|error| ToolError::new(error.to_string()))
    }

    fn execute_skill_tool(&self, value: serde_json::Value) -> Result<String, ToolError> {
        let output = self.tool_registry
            .execute("Skill", &value)
            .map_err(ToolError::new)?;

        let Some(mcp_state) = &self.mcp_state else {
            return Ok(output);
        };

        #[derive(Debug, Deserialize)]
        struct SkillRequest {
            skill: String,
        }
        let request: SkillRequest = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(_) => return Ok(output),
        };

        let relative_path = std::path::Path::new(".agents")
            .join("skills")
            .join(&request.skill)
            .join("SKILL.md");

        let contents = if relative_path.exists() {
            std::fs::read_to_string(&relative_path).ok()
        } else {
            let global_path = std::path::Path::new("/Users/dev/.gemini/config")
                .join("skills")
                .join(&request.skill)
                .join("SKILL.md");
            std::fs::read_to_string(&global_path).ok()
        };

        let Some(contents) = contents else {
            return Ok(output);
        };

        let coupled_servers = tools::parse_skill_mcp_servers(&contents);
        if coupled_servers.is_empty() {
            return Ok(output);
        }

        let mut state = mcp_state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut loaded_status = Vec::new();

        for server_name in coupled_servers {
            if state.loaded_servers().contains(&server_name) {
                loaded_status.push(format!("Server '{}' is already active.", server_name));
                continue;
            }

            match state.load_server(&server_name) {
                Ok(tools) => {
                    let runtime_tools = tools
                        .iter()
                        .map(|t| crate::mcp::mcp_runtime_tool_definition(t))
                        .collect::<Vec<_>>();
                    self.tool_registry.register_dynamic_tools(runtime_tools);
                    loaded_status.push(format!("Successfully started coupled MCP server '{}' with {} tools.", server_name, tools.len()));
                }
                Err(err) => {
                    loaded_status.push(format!("Failed to start coupled MCP server '{}': {}", server_name, err));
                }
            }
        }

        let status_message = format!(
            "\n\n### [Coupled MCP Servers]\n{}",
            loaded_status.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")
        );

        Ok(format!("{}{}", output, status_message))
    }
}

impl ToolExecutor for CliToolExecutor {
    fn execute(&self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        tracing::debug!(tool = %tool_name, "executing tool");
        if !self.tool_registry.is_tool_allowed(tool_name) {
            tracing::warn!(tool = %tool_name, "tool not allowed by --tools setting");
            return Err(ToolError::new(format!(
                "tool `{tool_name}` is not enabled by the current --tools setting"
            )));
        }
        let value = serde_json::from_str(input)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
        let result = if tool_name == "ToolSearch" {
            self.execute_search_tool(value)
        } else if tool_name == "McpSearch" {
            self.execute_mcp_search_tool(value)
        } else if tool_name == "Skill" {
            self.execute_skill_tool(value)
        } else if self.tool_registry.has_runtime_tool(tool_name) {
            self.execute_runtime_tool(tool_name, value)
        } else {
            self.tool_registry
                .execute(tool_name, &value)
                .map_err(ToolError::new)
        };
        match result {
            Ok(output) => {
                tracing::debug!(tool = %tool_name, output_len = output.len(), "tool succeeded");
                if self.emit_output {
                    let markdown = format_tool_result(tool_name, &output, false);
                    let renderer = self.renderer.lock().unwrap();
                    renderer
                        .stream_markdown(&markdown, &mut io::stdout())
                        .map_err(|error| ToolError::new(error.to_string()))?;
                }
                Ok(output)
            }
            Err(error) => {
                tracing::warn!(tool = %tool_name, error = %error, "tool failed");
                if self.emit_output {
                    let markdown = format_tool_result(tool_name, &error.to_string(), true);
                    let renderer = self.renderer.lock().unwrap();
                    renderer
                        .stream_markdown(&markdown, &mut io::stdout())
                        .map_err(|stream_error| ToolError::new(stream_error.to_string()))?;
                }
                Err(error)
            }
        }
    }
}
