use std::collections::{BTreeMap, BTreeSet};
use api::{
    resolve_model_alias, ApiError, ContentBlockDelta, InputContentBlock,
    InputMessage, MessageRequest, MessageResponse, OutputContentBlock, ProviderClient,
    StreamEvent as ApiStreamEvent, ToolChoice, ToolDefinition, ToolResultContentBlock,
};
use runtime::{
    ApiClient, ApiRequest, AssistantEvent, ContentBlock, ConversationMessage,
    MessageRole, PromptCacheEvent, ProviderFallbackConfig,
    RuntimeError, ToolError, ToolExecutor, ContextBudget, ConfigLoader,
    security::permission_enforcer::PermissionEnforcer,
};
use crate::provider_pipeline::{ResilientProviderChain, ProviderEntry};
use crate::registry::ToolSpec;
use crate::normalization::canonical_allowed_tool_name;
use crate::execute::execute_tool_with_enforcer;
use crate::tool_specs::mvp_tool_specs;

pub(crate) struct ProviderRuntimeClient {
    runtime: tokio::runtime::Runtime,
    pub(crate) chain: ResilientProviderChain,
    tools: BTreeSet<String>,
}

impl ProviderRuntimeClient {
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn new(model: String, tools: BTreeSet<String>) -> Result<Self, String> {
        let fallback_config = load_provider_fallback_config();
        Self::new_with_fallback_config(model, tools, &fallback_config)
    }

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn new_with_fallback_config(
        model: String,
        tools: BTreeSet<String>,
        fallback_config: &ProviderFallbackConfig,
    ) -> Result<Self, String> {
        let primary_model = fallback_config.primary().map_or(model, str::to_string);
        let primary = build_provider_entry(&primary_model)?;
        let mut chain = vec![primary];
        for fallback_model in fallback_config.fallbacks() {
            match build_provider_entry(fallback_model) {
                Ok(entry) => chain.push(entry),
                Err(error) => {
                    eprintln!(
                        "warning: skipping unavailable fallback provider {fallback_model}: {error}"
                    );
                }
            }
        }
        Ok(Self {
            runtime: tokio::runtime::Runtime::new().map_err(|error| error.to_string())?,
            chain: ResilientProviderChain::new(primary_model, chain),
            tools,
        })
    }
}

pub(crate) fn build_provider_entry(model: &str) -> Result<ProviderEntry, String> {
    let resolved = resolve_model_alias(model).clone();
    let client = ProviderClient::from_model(&resolved).map_err(|error| error.to_string())?;
    Ok(ProviderEntry {
        model: resolved,
        client,
    })
}

pub(crate) fn load_provider_fallback_config() -> ProviderFallbackConfig {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| ConfigLoader::default_for(cwd).load().ok())
        .map_or_else(ProviderFallbackConfig::default, |config| {
            config.provider_fallbacks().clone()
        })
}

impl ApiClient for ProviderRuntimeClient {
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
        let tools = tool_specs_for_tools(Some(&self.tools))
            .into_iter()
            .map(|spec| ToolDefinition {
                name: spec.name.to_string(),
                description: Some(spec.description.to_string()),
                input_schema: spec.input_schema,
            })
            .collect::<Vec<_>>();
        let messages = convert_messages(&request.messages);
        let system =
            (!request.system_prompt.is_empty()).then(|| request.system_prompt.join("\n\n"));
        let tool_choice = (!self.tools.is_empty()).then_some(ToolChoice::Auto);

        let tools_opt = (!tools.is_empty()).then_some(tools);

        self.chain
            .stream_with_fallback(&self.runtime, messages, system, tools_opt, tool_choice)
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn stream_with_provider(
    client: &ProviderClient,
    message_request: &MessageRequest,
) -> Result<Vec<AssistantEvent>, ApiError> {
    let mut stream = client.stream_message(message_request).await?;
    let mut events = Vec::new();
    let mut pending_tools: BTreeMap<u32, (String, String, String, Option<String>)> = BTreeMap::new();
    let mut pending_thinking: BTreeMap<u32, (String, Option<String>)> = BTreeMap::new();
    let mut saw_stop = false;
    let mut token_limit_exceeded = false;

    while let Some(event) = stream.next_event().await? {
        match event {
            ApiStreamEvent::MessageStart(start) => {
                for block in start.message.content {
                    push_output_block(
                        block,
                        0,
                        &mut events,
                        &mut pending_tools,
                        &mut pending_thinking,
                        true,
                    );
                }
            }
            ApiStreamEvent::ContentBlockStart(start) => {
                push_output_block(
                    start.content_block,
                    start.index,
                    &mut events,
                    &mut pending_tools,
                    &mut pending_thinking,
                    true,
                );
            }
            ApiStreamEvent::ContentBlockDelta(delta) => match delta.delta {
                ContentBlockDelta::TextDelta { text } => {
                    if !text.is_empty() {
                        events.push(AssistantEvent::TextDelta(text));
                    }
                }
                ContentBlockDelta::InputJsonDelta { partial_json } => {
                    if let Some((_, _, input, _)) = pending_tools.get_mut(&delta.index) {
                        input.push_str(&partial_json);
                    }
                }
                ContentBlockDelta::ThinkingDelta { thinking } => {
                    if let Some((pending, _)) = pending_thinking.get_mut(&delta.index) {
                        pending.push_str(&thinking);
                    }
                }
                ContentBlockDelta::SignatureDelta { signature } => {
                    if let Some((_, _, _, pending_signature)) = pending_tools.get_mut(&delta.index) {
                        pending_signature
                            .get_or_insert_with(String::new)
                            .push_str(&signature);
                    } else if let Some((_, pending_signature)) = pending_thinking.get_mut(&delta.index) {
                        pending_signature
                            .get_or_insert_with(String::new)
                            .push_str(&signature);
                    }
                }
            },
            ApiStreamEvent::ContentBlockStop(stop) => {
                if let Some((thinking, signature)) = pending_thinking.remove(&stop.index) {
                    events.push(AssistantEvent::Thinking {
                        thinking,
                        signature,
                    });
                }
                if let Some((id, name, input, signature)) = pending_tools.remove(&stop.index) {
                    events.push(AssistantEvent::ToolUse { id, name, input, signature });
                }
            }
            ApiStreamEvent::MessageDelta(delta) => {
                events.push(AssistantEvent::Usage(delta.usage.token_usage()));
                if let Some(reason) = delta.delta.stop_reason.as_deref() {
                    if reason == "length" || reason == "model_context_window_exceeded" || reason == "max_tokens" {
                        token_limit_exceeded = true;
                    }
                }
            }
            ApiStreamEvent::MessageStop(_) => {
                saw_stop = true;
                events.push(AssistantEvent::MessageStop);
            }
        }
    }

    push_prompt_cache_record(client, &mut events);

    if !saw_stop
        && events.iter().any(|event| {
            matches!(event, AssistantEvent::TextDelta(text) if !text.is_empty())
                || matches!(event, AssistantEvent::ToolUse { .. })
        })
    {
        events.push(AssistantEvent::MessageStop);
    }

    if token_limit_exceeded {
        let has_content = events.iter().any(|event| {
            matches!(event, AssistantEvent::TextDelta(text) if !text.is_empty())
                || matches!(event, AssistantEvent::ToolUse { .. })
        });
        if !has_content {
            return Err(ApiError::Api(Box::new(api::ApiErrorInfo {
                status: reqwest::StatusCode::BAD_REQUEST,
                error_type: Some("context_window_exceeded".to_string()),
                message: Some("token limit exceeded: the payload size of this request exceeds the available context size".to_string()),
                request_id: None,
                body: String::new(),
                retryable: false,
                suggested_action: None,
                retry_after: None,
            })));
        }
    }

    if events
        .iter()
        .any(|event| matches!(event, AssistantEvent::MessageStop))
    {
        return Ok(events);
    }

    let response = client
        .send_message(&MessageRequest {
            stream: false,
            ..message_request.clone()
        })
        .await?;
    let mut events = response_to_events(response);
    push_prompt_cache_record(client, &mut events);
    Ok(events)
}

pub(crate) struct SubagentToolExecutor {
    tools: BTreeSet<String>,
    enforcer: Option<PermissionEnforcer>,
    budget: ContextBudget,
}

impl SubagentToolExecutor {
    pub(crate) fn new(tools: BTreeSet<String>) -> Self {
        Self {
            tools,
            enforcer: None,
            budget: ContextBudget::default_budget(),
        }
    }

    pub(crate) fn with_enforcer(mut self, enforcer: PermissionEnforcer) -> Self {
        self.enforcer = Some(enforcer);
        self
    }

    pub(crate) fn with_budget(mut self, budget: ContextBudget) -> Self {
        self.budget = budget;
        self
    }
}

impl ToolExecutor for SubagentToolExecutor {
    fn execute(&self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        if !self.tools.contains(&canonical_allowed_tool_name(tool_name)) {
            return Err(ToolError::new(format!(
                "tool `{tool_name}` is not enabled for this sub-agent"
            )));
        }
        let value = serde_json::from_str(input)
            .map_err(|error| ToolError::new(format!("invalid tool input JSON: {error}")))?;
        execute_tool_with_enforcer(self.enforcer.as_ref(), tool_name, &value, self.budget)
            .map_err(ToolError::new)
    }
}

pub(crate) fn tool_specs_for_tools(_tools: Option<&BTreeSet<String>>) -> Vec<ToolSpec> {
    mvp_tool_specs()
}

pub(crate) fn convert_messages(messages: &[ConversationMessage]) -> Vec<InputMessage> {
    messages
        .iter()
        .filter_map(|message| {
            let role = match message.role {
                MessageRole::System | MessageRole::User | MessageRole::Tool => "user",
                MessageRole::Assistant => "assistant",
            };
            let content = message
                .blocks
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => InputContentBlock::Text { text: text.clone() },
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                    } => InputContentBlock::Thinking {
                        thinking: thinking.clone(),
                        signature: signature.clone(),
                    },
                    ContentBlock::ToolUse { id, name, input, signature } => InputContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::from_str(input)
                            .unwrap_or_else(|_| serde_json::json!({ "raw": input })),
                        signature: signature.clone(),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        output,
                        is_error,
                        ..
                    } => InputContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: vec![ToolResultContentBlock::Text {
                            text: output.clone(),
                        }],
                        is_error: *is_error,
                    },
                })
                .filter(
                    |block| !matches!(block, InputContentBlock::Text { text } if text.is_empty()),
                )
                .collect::<Vec<_>>();
            (!content.is_empty()).then(|| InputMessage {
                role: role.to_string(),
                content,
            })
        })
        .collect()
}

pub(crate) fn push_output_block(
    block: OutputContentBlock,
    block_index: u32,
    events: &mut Vec<AssistantEvent>,
    pending_tools: &mut BTreeMap<u32, (String, String, String, Option<String>)>,
    pending_thinking: &mut BTreeMap<u32, (String, Option<String>)>,
    streaming_tool_input: bool,
) {
    match block {
        OutputContentBlock::Text { text } => {
            if !text.is_empty() {
                events.push(AssistantEvent::TextDelta(text));
            }
        }
        OutputContentBlock::ToolUse { id, name, input, signature } => {
            let initial_input = if streaming_tool_input
                && input.is_object()
                && input.as_object().is_some_and(serde_json::Map::is_empty)
            {
                String::new()
            } else {
                input.to_string()
            };
            pending_tools.insert(block_index, (id, name, initial_input, signature));
        }
        OutputContentBlock::Thinking {
            thinking,
            signature,
        } => {
            if streaming_tool_input {
                pending_thinking.insert(block_index, (thinking, signature));
            } else {
                events.push(AssistantEvent::Thinking {
                    thinking,
                    signature,
                });
            }
        }
        OutputContentBlock::RedactedThinking { .. } => {}
    }
}

pub(crate) fn response_to_events(response: MessageResponse) -> Vec<AssistantEvent> {
    let mut events = Vec::new();
    let mut pending_tools = BTreeMap::new();
    let mut pending_thinking = BTreeMap::new();

    for (index, block) in response.content.into_iter().enumerate() {
        let index = u32::try_from(index).expect("response block index overflow");
        push_output_block(
            block,
            index,
            &mut events,
            &mut pending_tools,
            &mut pending_thinking,
            false,
        );
        if let Some((id, name, input, signature)) = pending_tools.remove(&index) {
            events.push(AssistantEvent::ToolUse { id, name, input, signature });
        }
    }

    events.push(AssistantEvent::Usage(response.usage.token_usage()));
    events.push(AssistantEvent::MessageStop);
    events
}

pub(crate) fn push_prompt_cache_record(client: &ProviderClient, events: &mut Vec<AssistantEvent>) {
    if let Some(record) = client.take_last_prompt_cache_record() {
        if let Some(event) = prompt_cache_record_to_runtime_event(record) {
            events.push(AssistantEvent::PromptCache(event));
        }
    }
}

pub(crate) fn prompt_cache_record_to_runtime_event(
    record: api::PromptCacheRecord,
) -> Option<PromptCacheEvent> {
    let cache_break = record.cache_break?;
    Some(PromptCacheEvent {
        unexpected: cache_break.unexpected,
        reason: cache_break.reason,
        previous_cache_read_input_tokens: cache_break.previous_cache_read_input_tokens,
        current_cache_read_input_tokens: cache_break.current_cache_read_input_tokens,
        token_drop: cache_break.token_drop,
    })
}

pub(crate) fn final_assistant_text(summary: &runtime::TurnSummary) -> String {
    summary
        .assistant_messages
        .last()
        .map(|message| {
            message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

