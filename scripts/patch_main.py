import re

with open("rust/crates/rusty-claude-cli/src/main.rs", "r") as f:
    text = f.read()

# 1. Update RuntimePluginState
runtime_plugin_state_def = """struct RuntimePluginState {
    feature_config: runtime::RuntimeFeatureConfig,
    tool_registry: GlobalToolRegistry,
    plugin_registry: PluginRegistry,
    mcp_state: Option<Arc<Mutex<RuntimeMcpState>>>,
    config_allowed_tools: Option<AllowedToolSet>,
}"""
text = re.sub(r'struct RuntimePluginState \{\n    feature_config: runtime::RuntimeFeatureConfig,\n    tool_registry: GlobalToolRegistry,\n    plugin_registry: PluginRegistry,\n    mcp_state: Option<Arc<Mutex<RuntimeMcpState>>>,\n\}', runtime_plugin_state_def, text)

# 2. Update build_runtime_plugin_state_with_loader return value
loader_return = """    let config_allowed_tools = match runtime_config.allowed_tools() {
        Some(tools) => tool_registry.normalize_allowed_tools(&tools).unwrap_or(None),
        None => None,
    };
    Ok(RuntimePluginState {
        feature_config,
        tool_registry,
        plugin_registry,
        mcp_state,
        config_allowed_tools,
    })"""
text = re.sub(r'    Ok\(RuntimePluginState \{\n        feature_config,\n        tool_registry,\n        plugin_registry,\n        mcp_state,\n    \}\)', loader_return, text)

# 3. Update build_runtime_with_plugin_state
builder_replace = """    let RuntimePluginState {
        feature_config,
        tool_registry,
        plugin_registry,
        mcp_state,
        config_allowed_tools,
    } = runtime_plugin_state;
    let effective_allowed_tools = allowed_tools.or(config_allowed_tools);
    plugin_registry.initialize()?;
    let policy = permission_policy(permission_mode, &feature_config, &tool_registry)
        .map_err(std::io::Error::other)?;
    let mut runtime = ConversationRuntime::new_with_features(
        session,
        AnthropicRuntimeClient::new(
            session_id,
            model,
            enable_tools,
            emit_output,
            effective_allowed_tools.clone(),
            tool_registry.clone(),
            progress_reporter,
        )?,
        effective_allowed_tools,"""

text = re.sub(r'    let RuntimePluginState \{\n        feature_config,\n        tool_registry,\n        plugin_registry,\n        mcp_state,\n    \} = runtime_plugin_state;\n    plugin_registry\.initialize\(\)\?;\n    let policy = permission_policy\(permission_mode, &feature_config, &tool_registry\)\n        \.map_err\(std::io::Error::other\)\?;\n    let mut runtime = ConversationRuntime::new_with_features\(\n        session,\n        AnthropicRuntimeClient::new\(\n            session_id,\n            model,\n            enable_tools,\n            emit_output,\n            allowed_tools\.clone\(\),\n            tool_registry\.clone\(\),\n            progress_reporter,\n        \)\?,\n        allowed_tools,', builder_replace, text)

with open("rust/crates/rusty-claude-cli/src/main.rs", "w") as f:
    f.write(text)
