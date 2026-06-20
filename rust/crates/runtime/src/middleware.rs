use std::collections::BTreeMap;

use crate::hooks::{HookAbortSignal, HookProgressReporter, HookRunResult, HookRunner};
use crate::permissions::{
    PermissionContext, PermissionOutcome, PermissionPolicy, PermissionPrompter,
};
use crate::session::ConversationMessage;
use crate::tool_dispatch::ToolCallRequest;
use serde_json::{Map, Value};
use telemetry::SessionTracer;

/// State of a single tool call as it passes through the middleware pipeline.
#[derive(Debug, Clone)]
pub struct ToolCallState {
    pub request: ToolCallRequest,
    pub permission_context: PermissionContext,
    pub pre_hook_messages: Vec<String>,
}

impl ToolCallState {
    #[must_use]
    pub fn new(request: ToolCallRequest) -> Self {
        Self {
            request,
            permission_context: PermissionContext::new(None, None),
            pre_hook_messages: Vec::new(),
        }
    }
}

/// Context passed through the middleware pipeline for a batch of tool calls.
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    pub calls: Vec<ToolCallState>,
    pub iteration: usize,
    pub metadata: BTreeMap<String, String>,
}

impl ToolCallContext {
    #[must_use]
    pub fn new(requests: Vec<ToolCallRequest>, iteration: usize) -> Self {
        let calls = requests.into_iter().map(ToolCallState::new).collect();
        Self {
            calls,
            iteration,
            metadata: BTreeMap::new(),
        }
    }
}

/// The outcome of the middleware pipeline.
pub struct ToolCallOutcome {
    pub messages: Vec<ConversationMessage>,
}

/// A middleware component in the processing pipeline.
pub trait TurnMiddleware<'a, 'p> {
    fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        next: &mut dyn FnMut(
            ToolCallContext,
            &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        ) -> ToolCallOutcome,
    ) -> ToolCallOutcome;
}

/// A chain of middlewares that handles tool execution.
pub struct MiddlewareChain<'a, 'p: 'a> {
    middlewares: Vec<Box<dyn TurnMiddleware<'a, 'p> + 'a>>,
}

impl<'a, 'p: 'a> Default for MiddlewareChain<'a, 'p> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, 'p: 'a> MiddlewareChain<'a, 'p> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Add a middleware to the end of the chain.
    #[must_use]
    pub fn with(mut self, middleware: impl TurnMiddleware<'a, 'p> + 'a) -> Self {
        self.middlewares.push(Box::new(middleware));
        self
    }

    /// Process a tool call context through the entire middleware chain.
    /// Consumes the chain.
    pub fn process(
        mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        terminal: impl FnMut(
                ToolCallContext,
                &mut Option<&mut (dyn PermissionPrompter + 'p)>,
            ) -> ToolCallOutcome
            + 'a,
    ) -> ToolCallOutcome {
        #[allow(clippy::type_complexity)]
        let mut next: Box<
            dyn FnMut(
                    ToolCallContext,
                    &mut Option<&mut (dyn PermissionPrompter + 'p)>,
                ) -> ToolCallOutcome
                + 'a,
        > = Box::new(terminal);

        // Pop middlewares from the end, so they execute in the order they were added.
        while let Some(mut mw) = self.middlewares.pop() {
            let mut current_next = next;
            next = Box::new(move |ctx, prompter| mw.process(ctx, prompter, &mut *current_next));
        }

        next(ctx, prompter)
    }
}

/// Middleware that handles permission checking using a PermissionPolicy.
pub struct PermissionMiddleware<'a> {
    pub policy: &'a PermissionPolicy,
}

impl<'a> PermissionMiddleware<'a> {
    pub fn new(policy: &'a PermissionPolicy) -> Self {
        Self { policy }
    }
}

impl<'a, 'p> TurnMiddleware<'a, 'p> for PermissionMiddleware<'a> {
    fn process(
        &mut self,
        mut ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        next: &mut dyn FnMut(
            ToolCallContext,
            &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        ) -> ToolCallOutcome,
    ) -> ToolCallOutcome {
        let mut allowed_calls = Vec::new();
        let mut denied_messages = Vec::new();

        // 1. Process all permissions
        for call_state in ctx.calls {
            let outcome = if let Some(p) = prompter.as_mut() {
                self.policy.authorize_with_context(
                    &call_state.request.tool_name,
                    &call_state.request.input,
                    &call_state.permission_context,
                    Some(*p),
                )
            } else {
                self.policy.authorize_with_context(
                    &call_state.request.tool_name,
                    &call_state.request.input,
                    &call_state.permission_context,
                    None,
                )
            };

            match outcome {
                PermissionOutcome::Allow => allowed_calls.push(call_state),
                PermissionOutcome::Deny { reason } => {
                    let final_reason = if call_state.pre_hook_messages.is_empty() {
                        reason
                    } else {
                        format!("{}\n\n{}", call_state.pre_hook_messages.join("\n"), reason)
                    };
                    denied_messages.push(ConversationMessage::tool_result(
                        call_state.request.tool_use_id,
                        call_state.request.tool_name,
                        final_reason,
                        true,
                    ));
                }
            }
        }

        ctx.calls = allowed_calls;

        let mut outcome = if ctx.calls.is_empty() {
            ToolCallOutcome {
                messages: Vec::new(),
            }
        } else {
            next(ctx, prompter)
        };

        outcome.messages.extend(denied_messages);
        outcome
    }
}

// I will implement HookMiddleware and TracingMiddleware next.

/// Middleware that handles grouping tool calls into batches (parallel vs sequential)
/// and optionally uses RAG to avoid known bad concurrent combinations.
pub struct RagBatchingMiddleware<'a> {
    pub rag: &'a RagClient,
}

impl<'a> RagBatchingMiddleware<'a> {
    pub fn new(rag: &'a RagClient) -> Self {
        Self { rag }
    }
}

impl<'a, 'p> TurnMiddleware<'a, 'p> for RagBatchingMiddleware<'a> {
    fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        next: &mut dyn FnMut(
            ToolCallContext,
            &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        ) -> ToolCallOutcome,
    ) -> ToolCallOutcome {
        let requests: Vec<_> = ctx.calls.iter().map(|c| c.request.clone()).collect();
        let mut batches = crate::tool_dispatch::batch_tool_calls(requests);

        // Optional RAG hint lookup to modify batches if needed
        if !batches.is_empty() {
            let mut modified_batches = Vec::new();
            for batch in batches {
                match batch {
                    crate::tool_dispatch::ToolBatch::Parallel(calls) if calls.len() > 1 => {
                        // Check if this specific combo has historically failed due to concurrency
                        let mut tool_names: Vec<_> =
                            calls.iter().map(|c| c.tool_name.as_str()).collect();
                        tool_names.sort_unstable(); // normalize order
                        tool_names.dedup();
                        let query = format!("parallel batch error: {}", tool_names.join(" "));
                        if let Some(hits) = self.rag.query(&query, 1) {
                            if hits.iter().any(|h| {
                                h.snippet.contains("error") && h.snippet.contains("concurrent")
                            }) {
                                // Demote to sequential batches
                                for call in calls {
                                    modified_batches
                                        .push(crate::tool_dispatch::ToolBatch::Sequential(call));
                                }
                                continue;
                            }
                        }
                        modified_batches.push(crate::tool_dispatch::ToolBatch::Parallel(calls));
                    }
                    other => modified_batches.push(other),
                }
            }
            batches = modified_batches;
        }

        let mut all_messages = Vec::new();

        for batch in batches {
            let batch_calls = match batch {
                crate::tool_dispatch::ToolBatch::Parallel(calls) => calls,
                crate::tool_dispatch::ToolBatch::Sequential(call) => vec![call],
            };

            let mut sub_states = Vec::new();
            for req in batch_calls {
                if let Some(state) = ctx
                    .calls
                    .iter()
                    .find(|s| s.request.tool_use_id == req.tool_use_id)
                {
                    sub_states.push(state.clone());
                }
            }

            if sub_states.is_empty() {
                continue;
            }

            let sub_ctx = ToolCallContext {
                calls: sub_states,
                iteration: ctx.iteration,
                metadata: ctx.metadata.clone(),
            };

            let outcome = next(sub_ctx, prompter);
            all_messages.extend(outcome.messages);

            // If the batch resulted in any fatal errors, we could break here.
            // But currently the system executes all queued tools.
        }

        ToolCallOutcome {
            messages: all_messages,
        }
    }
}

/// Middleware that runs the pre/post execution hooks.
pub struct HookMiddleware<'a> {
    pub runner: &'a HookRunner,
    pub abort_signal: &'a HookAbortSignal,
    pub reporter: &'a mut Option<Box<dyn HookProgressReporter>>,
}

impl<'a> HookMiddleware<'a> {
    pub fn new(
        runner: &'a HookRunner,
        abort_signal: &'a HookAbortSignal,
        reporter: &'a mut Option<Box<dyn HookProgressReporter>>,
    ) -> Self {
        Self {
            runner,
            abort_signal,
            reporter,
        }
    }
}

impl<'a, 'p> TurnMiddleware<'a, 'p> for HookMiddleware<'a> {
    fn process(
        &mut self,
        mut ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        next: &mut dyn FnMut(
            ToolCallContext,
            &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        ) -> ToolCallOutcome,
    ) -> ToolCallOutcome {
        let mut executed_calls = Vec::new();
        let mut denied_messages = Vec::new();

        // 1. Run Pre-Hook for all tools in the context
        for mut call_state in ctx.calls {
            let tool_name = &call_state.request.tool_name;
            let input = &call_state.request.input;

            let pre_hook_result = if let Some(reporter) = self.reporter.as_deref_mut() {
                self.runner.run_pre_tool_use_with_context(
                    tool_name,
                    input,
                    Some(self.abort_signal),
                    Some(reporter),
                )
            } else {
                self.runner.run_pre_tool_use_with_context(
                    tool_name,
                    input,
                    Some(self.abort_signal),
                    None,
                )
            };

            // Propagate input updates
            if let Some(new_input) = pre_hook_result.updated_input() {
                call_state.request.input = new_input.to_owned();
            }

            // Propagate permission overrides
            if pre_hook_result.permission_override().is_some()
                || pre_hook_result.permission_reason().is_some()
            {
                call_state.permission_context = PermissionContext::new(
                    pre_hook_result.permission_override(),
                    pre_hook_result.permission_reason().map(ToOwned::to_owned),
                );
            }

            // Record any messages added by the hook
            call_state
                .pre_hook_messages
                .extend(pre_hook_result.messages().iter().cloned());

            if pre_hook_result.is_cancelled()
                || pre_hook_result.is_failed()
                || pre_hook_result.is_denied()
            {
                let status = if pre_hook_result.is_cancelled() {
                    "cancelled"
                } else if pre_hook_result.is_failed() {
                    "failed for"
                } else {
                    "denied"
                };

                let reason = format_hook_message(
                    &pre_hook_result,
                    &format!("PreToolUse hook {} tool `{}`", status, tool_name),
                );

                denied_messages.push(ConversationMessage::tool_result(
                    call_state.request.tool_use_id,
                    call_state.request.tool_name,
                    reason,
                    true,
                ));
            } else {
                executed_calls.push(call_state);
            }
        }

        ctx.calls = executed_calls.clone();

        let mut outcome = if ctx.calls.is_empty() {
            ToolCallOutcome {
                messages: Vec::new(),
            }
        } else {
            next(ctx, prompter)
        };

        // 2. Run Post-Hooks
        // We need a map of the allowed calls to match their inputs/states against the outcome messages
        let mut call_map = executed_calls
            .into_iter()
            .map(|state| (state.request.tool_use_id.clone(), state))
            .collect::<std::collections::HashMap<_, _>>();

        for message in &mut outcome.messages {
            let Some(crate::session::ContentBlock::ToolResult {
                tool_use_id,
                tool_name,
                output,
                is_error,
            }) = message.blocks.first_mut()
            else {
                continue;
            };

            if let Some(state) = call_map.remove(tool_use_id) {
                let input = &state.request.input;
                let is_err = *is_error;

                let post_hook_result = if is_err {
                    self.runner.run_post_tool_use_failure_with_context(
                        tool_name,
                        input,
                        output,
                        Some(self.abort_signal),
                        self.reporter.as_deref_mut(),
                    )
                } else {
                    self.runner.run_post_tool_use_with_context(
                        tool_name,
                        input,
                        output,
                        is_err,
                        Some(self.abort_signal),
                        self.reporter.as_deref_mut(),
                    )
                };

                let new_is_error = is_err
                    || post_hook_result.is_denied()
                    || post_hook_result.is_failed()
                    || post_hook_result.is_cancelled();
                let mut all_messages = state.pre_hook_messages.clone();
                all_messages.extend(post_hook_result.messages().iter().cloned());
                let mut new_output =
                    merge_hook_feedback(&all_messages, output.to_string(), new_is_error);

                if new_is_error {
                    new_output.push_str("\n\n[SYSTEM DIRECTIVE]: The tool execution failed. DO NOT give up. You are a fully autonomous agent. Please analyze the error, think step-by-step about why it happened, and try an alternative approach. You must continue until the problem is solved.");
                }

                *output = new_output;
                *is_error = new_is_error;
            }
        }

        // Add the denied messages from the pre-hook phase
        outcome.messages.extend(denied_messages);
        outcome
    }
}

/// Middleware that records tracing events (tool_started, tool_finished).
pub struct TracingMiddleware<'a> {
    pub tracer: &'a mut Option<SessionTracer>,
}

impl<'a> TracingMiddleware<'a> {
    pub fn new(tracer: &'a mut Option<SessionTracer>) -> Self {
        Self { tracer }
    }
}

impl<'a, 'p> TurnMiddleware<'a, 'p> for TracingMiddleware<'a> {
    fn process(
        &mut self,
        ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        next: &mut dyn FnMut(
            ToolCallContext,
            &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        ) -> ToolCallOutcome,
    ) -> ToolCallOutcome {
        let iteration = ctx.iteration;

        // Before execution: record started
        if let Some(tracer) = self.tracer.as_mut() {
            for call in &ctx.calls {
                let mut attributes = Map::new();
                attributes.insert("iteration".to_string(), Value::from(iteration as u64));
                attributes.insert(
                    "tool_name".to_string(),
                    Value::String(call.request.tool_name.clone()),
                );
                tracer.record("tool_execution_started", attributes);
            }
        }

        let outcome = next(ctx, prompter);

        // After execution: record finished
        if let Some(tracer) = self.tracer.as_mut() {
            for message in &outcome.messages {
                if let Some(crate::session::ContentBlock::ToolResult {
                    tool_name,
                    is_error,
                    ..
                }) = message.blocks.first()
                {
                    let mut attributes = Map::new();
                    attributes.insert("iteration".to_string(), Value::from(iteration as u64));
                    attributes.insert("tool_name".to_string(), Value::String(tool_name.clone()));
                    attributes.insert(
                        "status".to_string(),
                        Value::String(if *is_error {
                            "error".to_string()
                        } else {
                            "success".to_string()
                        }),
                    );
                    tracer.record("tool_execution_completed", attributes);
                }
            }
        }

        outcome
    }
}

fn format_hook_message(result: &HookRunResult, fallback: &str) -> String {
    if result.messages().is_empty() {
        fallback.to_string()
    } else {
        result.messages().join("\n")
    }
}

fn merge_hook_feedback(messages: &[String], output: String, is_error: bool) -> String {
    if messages.is_empty() {
        return output;
    }

    let mut sections = Vec::new();
    if !output.trim().is_empty() {
        sections.push(output);
    }
    let label = if is_error {
        "Hook feedback (error)"
    } else {
        "Hook feedback"
    };
    sections.push(format!("{label}:\n{}", messages.join("\n")));
    sections.join("\n\n")
}

// ---------------------------------------------------------------------------
// RAG Client — shared HTTP helper for RAG service communication
// ---------------------------------------------------------------------------

/// Lightweight, fire-and-forget HTTP client for the local RAG service.
/// All operations are best-effort: failures are silently ignored so the
/// primary pipeline is never blocked.
#[derive(Clone)]
pub struct RagClient {
    endpoint: String,
    client: reqwest::blocking::Client,
    timeout: std::time::Duration,
}

impl std::fmt::Debug for RagClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RagClient")
            .field("endpoint", &self.endpoint)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl RagClient {
    /// Create a new RAG client pointing at the given base URL (e.g. `http://127.0.0.1:8787`).
    #[must_use]
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            client: reqwest::blocking::Client::new(),
            timeout: std::time::Duration::from_secs(2),
        }
    }

    /// Create a client with default localhost endpoint.
    #[must_use]
    pub fn localhost() -> Self {
        let port: u16 = std::env::var("CLAW_RAG_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8787);
        Self::new(format!("http://127.0.0.1:{port}"))
    }

    /// Query the RAG index for relevant snippets.
    /// Returns `None` if the service is unreachable or returns no hits.
    pub fn query(&self, query: &str, top_k: u32) -> Option<Vec<RagHitResult>> {
        let body = serde_json::json!({
            "query": query,
            "top_k": top_k
        });
        let res = self
            .client
            .post(format!("{}/v1/query", self.endpoint))
            .timeout(self.timeout)
            .json(&body)
            .send()
            .ok()?;

        let response: RagQueryResponse = res.json().ok()?;
        if response.hits.is_empty() {
            None
        } else {
            Some(response.hits)
        }
    }

    /// Ingest a document into the RAG index (fire-and-forget).
    /// Returns `true` if ingestion succeeded.
    pub fn ingest(&self, path: &str, content: &str) -> bool {
        self.client
            .post(format!("{}/v1/ingest", self.endpoint))
            .timeout(self.timeout)
            .json(&serde_json::json!({
                "path": path,
                "content": content,
            }))
            .send()
            .is_ok_and(|r| r.status().is_success())
    }

    /// Non-blocking ingest: spawns a thread so the caller never blocks.
    pub fn ingest_async(&self, path: String, content: String) {
        let client = self.clone();
        std::thread::spawn(move || {
            client.ingest(&path, &content);
        });
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RagHitResult {
    pub path: String,
    pub snippet: String,
    pub score: Option<f32>,
}

#[derive(Debug, serde::Deserialize)]
struct RagQueryResponse {
    hits: Vec<RagHitResult>,
}

// ---------------------------------------------------------------------------
// RagContextMiddleware — enrich tool calls with RAG context + ingest results
// ---------------------------------------------------------------------------

/// Middleware that:
/// 1. **Pre-execution**: queries the RAG service for context relevant to the
///    tool being called (e.g., prior errors, related code snippets).
/// 2. **Post-execution**: ingests significant tool results (errors, large
///    outputs) back into the RAG index so they are available in future turns
///    and sessions.
///
/// This makes RAG an active memory layer rather than a one-shot system prompt.
pub struct RagContextMiddleware {
    rag: RagClient,
    /// Minimum output size (bytes) to trigger post-execution ingest for successful results.
    ingest_threshold_bytes: usize,
}

impl RagContextMiddleware {
    pub fn new(rag: RagClient) -> Self {
        Self {
            rag,
            ingest_threshold_bytes: 1024,
        }
    }

    #[must_use]
    pub fn with_ingest_threshold(mut self, bytes: usize) -> Self {
        self.ingest_threshold_bytes = bytes;
        self
    }

    /// Query RAG for context relevant to a tool call.
    fn query_context(&self, tool_name: &str, input: &str) -> Option<String> {
        // Build a semantic query from the tool name + input
        let query = format!("{} {}", tool_name, truncate(input, 200));
        let hits = self.rag.query(&query, 3)?;

        let context = hits
            .into_iter()
            .map(|h| format!("[{}] {}", h.path, h.snippet))
            .collect::<Vec<_>>()
            .join("\n---\n");

        Some(format!(
            "\n<rag_context>\nRelevant prior context:\n{context}\n</rag_context>"
        ))
    }

    /// Ingest a tool result into RAG for future reference.
    fn ingest_result(&self, tool_name: &str, _tool_use_id: &str, output: &str, is_error: bool) {
        // Always ingest errors; only ingest large successful results
        if !is_error && output.len() < self.ingest_threshold_bytes {
            return;
        }

        let label = if is_error { "error" } else { "result" };
        let path = format!("tool-history/{tool_name}/{label}");
        let content = truncate(output, 4096).to_string();

        self.rag.ingest_async(path, content);
    }
}

impl<'a, 'p> TurnMiddleware<'a, 'p> for RagContextMiddleware {
    fn process(
        &mut self,
        mut ctx: ToolCallContext,
        prompter: &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        next: &mut dyn FnMut(
            ToolCallContext,
            &mut Option<&mut (dyn PermissionPrompter + 'p)>,
        ) -> ToolCallOutcome,
    ) -> ToolCallOutcome {
        // ---- Pre-execution: enrich metadata with RAG context ----
        for call in &mut ctx.calls {
            if let Some(context) = self.query_context(&call.request.tool_name, &call.request.input)
            {
                call.pre_hook_messages.push(context);
            }
        }

        // Run the rest of the middleware chain
        let outcome = next(ctx, prompter);

        // ---- Post-execution: ingest significant results into RAG ----
        for message in &outcome.messages {
            if let Some(crate::session::ContentBlock::ToolResult {
                tool_use_id,
                tool_name,
                output,
                is_error,
            }) = message.blocks.first()
            {
                self.ingest_result(tool_name, tool_use_id, output, *is_error);
            }
        }

        outcome
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    // Find a char boundary at or before `max`
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod rag_middleware_tests {
    use super::*;
    use crate::tool_dispatch::ToolCallRequest;

    #[test]
    fn rag_client_query_returns_none_when_service_unreachable() {
        // Point at a port that definitely has nothing listening
        let client = RagClient::new("http://127.0.0.1:1");
        assert!(client.query("test", 3).is_none());
    }

    #[test]
    fn rag_client_ingest_returns_false_when_service_unreachable() {
        let client = RagClient::new("http://127.0.0.1:1");
        assert!(!client.ingest("test/path", "content"));
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        let s = "hello世界test";
        let t = truncate(s, 7);
        // 'h','e','l','l','o' = 5 bytes, '世' = 3 bytes = 8 bytes total
        // truncating at 7 should go back to 5 (before '世')
        assert_eq!(t, "hello");

        let t2 = truncate(s, 100);
        assert_eq!(t2, s);
    }

    #[test]
    fn rag_context_middleware_passes_through_when_rag_unavailable() {
        // This test verifies that when RAG is unreachable, the middleware
        // passes through transparently without modifying the pipeline.
        let rag = RagClient::new("http://127.0.0.1:1");
        let mut mw = RagContextMiddleware::new(rag);

        let req = ToolCallRequest {
            tool_use_id: "t1".into(),
            tool_name: "read_file".into(),
            input: r#"{"path":"foo.rs"}"#.into(),
        };
        let ctx = ToolCallContext::new(vec![req], 1);

        let mut prompter: Option<&mut (dyn PermissionPrompter + '_)> = None;
        let outcome = mw.process(ctx, &mut prompter, &mut |ctx, _| {
            // Terminal: return a simple result
            let messages = ctx
                .calls
                .into_iter()
                .map(|s| {
                    ConversationMessage::tool_result(
                        s.request.tool_use_id,
                        s.request.tool_name,
                        "file content here".to_string(),
                        false,
                    )
                })
                .collect();
            ToolCallOutcome { messages }
        });

        assert_eq!(outcome.messages.len(), 1);
    }
}
