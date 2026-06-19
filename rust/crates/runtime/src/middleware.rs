use std::collections::BTreeMap;

use crate::hooks::{
    HookAbortSignal, HookProgressReporter, HookRunResult, HookRunner,
};
use crate::permissions::{PermissionContext, PermissionOutcome, PermissionPolicy, PermissionPrompter};
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
        terminal: impl FnMut(ToolCallContext, &mut Option<&mut (dyn PermissionPrompter + 'p)>) -> ToolCallOutcome + 'a,
    ) -> ToolCallOutcome {
        let mut next: Box<
            dyn FnMut(
                ToolCallContext,
                &mut Option<&mut (dyn PermissionPrompter + 'p)>,
            ) -> ToolCallOutcome + 'a,
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
            ToolCallOutcome { messages: Vec::new() }
        } else {
            next(ctx, prompter)
        };

        outcome.messages.extend(denied_messages);
        outcome
    }
}

// I will implement HookMiddleware and TracingMiddleware next.

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
            call_state.pre_hook_messages.extend(pre_hook_result.messages().iter().cloned());

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
            ToolCallOutcome { messages: Vec::new() }
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
            let Some(crate::session::ContentBlock::ToolResult { tool_use_id, tool_name, output, is_error }) = message.blocks.first_mut() else { continue };
            
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

                let new_is_error = is_err || post_hook_result.is_denied() || post_hook_result.is_failed() || post_hook_result.is_cancelled();
                let mut all_messages = state.pre_hook_messages.clone();
                all_messages.extend(post_hook_result.messages().iter().cloned());
                let mut new_output = merge_hook_feedback(&all_messages, output.to_string(), new_is_error);

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
                }) = message.blocks.first() {
                    let mut attributes = Map::new();
                    attributes.insert("iteration".to_string(), Value::from(iteration as u64));
                    attributes.insert(
                        "tool_name".to_string(),
                        Value::String(tool_name.clone()),
                    );
                    attributes.insert(
                        "status".to_string(),
                        Value::String(if *is_error { "error".to_string() } else { "success".to_string() }),
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
