use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::sync::Mutex;

use serde_json::{Map, Value};
use telemetry::SessionTracer;

use crate::compact::{
    compact_session, estimate_session_tokens, CompactionConfig, CompactionResult,
};
use crate::config::RuntimeFeatureConfig;
use crate::fractal::{BifurcationEngine, OrbitSample, Regime};
use crate::hooks::{HookAbortSignal, HookProgressReporter, HookRunner};
use crate::security::permissions::{PermissionPolicy, PermissionPrompter};
use crate::session::error_tracker::ErrorTracker;
use crate::session::state::{ContentBlock, ConversationMessage, Session};
use crate::session::usage::{TokenUsage, UsageTracker};

const DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD: u32 = 100_000;
const AUTO_COMPACTION_THRESHOLD_ENV_VAR: &str = "CLAUDE_CODE_AUTO_COMPACT_INPUT_TOKENS";
const BIFURCATION_HALT_ENV_VAR: &str = "CLAW_FRACTAL_HALT_AT_CHAOS";

/// Fully assembled request payload sent to the upstream model client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub system_prompt: Vec<String>,
    pub messages: Vec<ConversationMessage>,
}

/// Streamed events emitted while processing a single assistant turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantEvent {
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    TextDelta(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
        signature: Option<String>,
    },
    Usage(TokenUsage),
    PromptCache(PromptCacheEvent),
    MessageStop,
}

/// Prompt-cache telemetry captured from the provider response stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCacheEvent {
    pub unexpected: bool,
    pub reason: String,
    pub previous_cache_read_input_tokens: u32,
    pub current_cache_read_input_tokens: u32,
    pub token_drop: u32,
}

/// Minimal streaming API contract required by [`ConversationRuntime`].
pub trait ApiClient {
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError>;
}

/// Trait implemented by tool dispatchers that execute model-requested tools.
pub trait ToolExecutor: Send + Sync {
    fn execute(&self, tool_name: &str, input: &str) -> Result<String, ToolError>;
}

/// Error returned when a tool invocation fails locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    message: String,
}

impl ToolError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ToolError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolError {}

/// Error returned when a conversation turn cannot be completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
    /// Failure that retrying cannot fix — the δ breaker shed the request, or
    /// the provider permanently rejected it. Outer loops must stop rather than
    /// re-sending, which is the hammering the breaker exists to prevent.
    terminal: bool,
}

impl RuntimeError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            terminal: false,
        }
    }

    #[must_use]
    pub fn terminal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            terminal: true,
        }
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.terminal
    }
}

impl Display for RuntimeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

/// Summary of one completed runtime turn, including tool results and usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    pub assistant_messages: Vec<ConversationMessage>,
    pub tool_results: Vec<ConversationMessage>,
    pub prompt_cache_events: Vec<PromptCacheEvent>,
    pub iterations: usize,
    pub usage: TokenUsage,
    pub auto_compaction: Option<AutoCompactionEvent>,
    /// Cognitive regime the turn ended in. `Laminar` means the loop never hit
    /// enough resistance to force an orbit split.
    pub bifurcation_regime: Regime,
}

/// Details about automatic session compaction applied during a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoCompactionEvent {
    pub removed_message_count: usize,
}

/// Coordinates the model loop, tool execution, hooks, and session updates.
pub struct ConversationRuntime<C, T> {
    session: Session,
    api_client: C,
    tool_executor: T,
    permission_policy: PermissionPolicy,
    system_prompt: Vec<String>,
    max_iterations: usize,
    usage_tracker: UsageTracker,
    hook_runner: HookRunner,
    auto_compaction_input_tokens_threshold: u32,
    hook_abort_signal: HookAbortSignal,
    hook_progress_reporter: Option<Box<dyn HookProgressReporter>>,
    session_tracer: Option<SessionTracer>,
    error_tracker: Mutex<ErrorTracker>,
    bifurcation: Mutex<BifurcationEngine>,
}

impl<C, T> ConversationRuntime<C, T>
where
    C: ApiClient,
    T: ToolExecutor,
{
    #[must_use]
    pub fn new(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
    ) -> Self {
        Self::new_with_features(
            session,
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
            &RuntimeFeatureConfig::default(),
        )
    }

    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new_with_features(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
        feature_config: &RuntimeFeatureConfig,
    ) -> Self {
        let usage_tracker = UsageTracker::from_session(&session);
        Self {
            session,
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
            max_iterations: usize::MAX,
            usage_tracker,
            hook_runner: HookRunner::from_feature_config(feature_config),
            auto_compaction_input_tokens_threshold: auto_compaction_threshold_from_env(),
            hook_abort_signal: HookAbortSignal::default(),
            hook_progress_reporter: None,
            session_tracer: None,
            error_tracker: Mutex::new(ErrorTracker::new()),
            bifurcation: Mutex::new(
                BifurcationEngine::new().with_halt_at_chaos(bifurcation_halt_from_env()),
            ),
        }
    }

    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    #[must_use]
    pub fn with_auto_compaction_input_tokens_threshold(mut self, threshold: u32) -> Self {
        self.auto_compaction_input_tokens_threshold = threshold;
        self
    }

    /// Update the auto-compaction threshold after construction. This allows the
    /// caller to tune the threshold based on runtime information (e.g., the
    /// server-returned context window size from a 400 error).
    pub fn set_auto_compaction_input_tokens_threshold(&mut self, threshold: u32) {
        self.auto_compaction_input_tokens_threshold = threshold;
    }

    #[must_use]
    pub fn with_hook_abort_signal(mut self, hook_abort_signal: HookAbortSignal) -> Self {
        self.hook_abort_signal = hook_abort_signal;
        self
    }

    pub fn set_hook_abort_signal(&mut self, hook_abort_signal: HookAbortSignal) {
        self.hook_abort_signal = hook_abort_signal;
    }

    #[must_use]
    pub fn with_hook_progress_reporter(
        mut self,
        hook_progress_reporter: Box<dyn HookProgressReporter>,
    ) -> Self {
        self.hook_progress_reporter = Some(hook_progress_reporter);
        self
    }

    #[must_use]
    pub fn with_session_tracer(mut self, session_tracer: SessionTracer) -> Self {
        self.session_tracer = Some(session_tracer);
        self
    }

    /// Replace the bifurcation ladder configuration (thresholds, chaos halt).
    #[must_use]
    pub fn with_bifurcation(mut self, engine: BifurcationEngine) -> Self {
        self.bifurcation = Mutex::new(engine);
        self
    }

    /// Current thinking regime forced on the model by the ladder.
    #[must_use]
    pub fn bifurcation_regime(&self) -> Regime {
        self.bifurcation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .regime()
    }

    /// Diagnostic view of the cognitive ladder, mirroring the transport
    /// breaker's `bifurcation_summary`.
    #[must_use]
    pub fn bifurcation_summary(&self) -> String {
        self.bifurcation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .summary()
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch_tool_calls<'a>(
        &mut self,
        pending_tool_uses: Vec<(String, String, String)>,
        iterations: usize,
        mut prompter: Option<&mut (dyn PermissionPrompter + 'a)>,
    ) -> (Vec<ConversationMessage>, bool) {
        use crate::loop_detection::LoopDetectionMiddleware;
        use crate::middleware::{
            HookMiddleware, MiddlewareChain, PermissionMiddleware, RagBatchingMiddleware,
            RagClient, RagContextMiddleware, ToolCallContext, ToolCallOutcome, ToolCallState,
            TracingMiddleware,
        };
        use crate::security::permissions::PermissionContext;
        use crate::tool_dispatch::{
            batch_tool_calls, execute_parallel_batch, ToolBatch, ToolCallRequest,
        };

        let calls = pending_tool_uses
            .into_iter()
            .map(|(tool_use_id, tool_name, input)| ToolCallRequest {
                tool_use_id,
                tool_name,
                input,
            })
            .collect::<Vec<_>>();

        // Hard probe gate (M2), evaluated over the whole step in emission
        // order before batching: `batch_tool_calls` splits reads and mutations
        // into different middleware contexts, so a per-batch check would
        // false-block legal probe→mutation steps. A probe emitted ahead of a
        // mutation proves the agent looked; a blind mutation is refused
        // pre-execution.
        let gate_pairs: Vec<(String, String)> = calls
            .iter()
            .map(|call| (call.tool_name.clone(), call.input.clone()))
            .collect();
        let gate_blocked: Vec<usize> = self
            .bifurcation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .gate_mutations_for_step(&gate_pairs);
        let mut gate_refusals = Vec::new();
        for &index in &gate_blocked {
            let call = &calls[index];
            gate_refusals.push((
                index,
                ConversationMessage::tool_result(
                    call.tool_use_id.clone(),
                    call.tool_name.clone(),
                    BifurcationEngine::probe_gate_refusal(&call.tool_name),
                    true,
                ),
            ));
        }
        let calls: Vec<ToolCallRequest> = calls
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !gate_blocked.contains(index))
            .map(|(_, call)| call)
            .collect();

        let batches = batch_tool_calls(calls);
        let mut batch_ids: Vec<String> = Vec::new();
        for batch in &batches {
            match batch {
                ToolBatch::Sequential(call) => batch_ids.push(call.tool_use_id.clone()),
                ToolBatch::Parallel(calls) => {
                    batch_ids.extend(calls.iter().map(|call| call.tool_use_id.clone()));
                }
            }
        }
        let mut emission_order: std::collections::HashMap<String, usize> = batch_ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect();
        for (index, refusal) in &gate_refusals {
            if let ContentBlock::ToolResult { tool_use_id, .. } = &refusal.blocks[0] {
                emission_order.insert(tool_use_id.clone(), *index);
            }
        }
        let mut result_messages = Vec::new();
        let mut sample = OrbitSample::new(iterations);
        let bifurcation = &self.bifurcation;

        for batch in batches {
            let reqs = match batch {
                ToolBatch::Sequential(call) => vec![call],
                ToolBatch::Parallel(calls) => calls,
            };

            let ctx = ToolCallContext {
                calls: reqs
                    .into_iter()
                    .map(|req| ToolCallState {
                        request: req,
                        permission_context: PermissionContext::new(None, None),
                        pre_hook_messages: Vec::new(),
                    })
                    .collect(),
                iteration: iterations,
                metadata: std::collections::BTreeMap::new(),
            };

            let rag_client = RagClient::localhost();

            let chain = MiddlewareChain::new()
                .with(LoopDetectionMiddleware::new(&self.session, bifurcation))
                .with(RagContextMiddleware::new(rag_client.clone()))
                .with(PermissionMiddleware::new(&self.permission_policy))
                .with(RagBatchingMiddleware::new(&rag_client))
                .with(HookMiddleware::new(
                    &self.hook_runner,
                    &self.hook_abort_signal,
                    &mut self.hook_progress_reporter,
                ))
                .with(TracingMiddleware::new(&mut self.session_tracer));

            let mut prompter_ref = prompter.as_deref_mut();
            let executor = &self.tool_executor;
            let error_tracker = &self.error_tracker;

            let outcome = chain.process(ctx, &mut prompter_ref, |ctx, _| {
                let mut terminal_messages = Vec::new();
                let calls_to_execute = ctx
                    .calls
                    .into_iter()
                    .map(|state| state.request)
                    .collect::<Vec<_>>();

                if calls_to_execute.is_empty() {
                    return ToolCallOutcome {
                        messages: terminal_messages,
                    };
                }

                if calls_to_execute.len() == 1 {
                    let req = calls_to_execute.into_iter().next().unwrap();
                    let (output, is_error) = match executor.execute(&req.tool_name, &req.input) {
                        Ok(output) => (output, false),
                        Err(error) => (
                            serde_json::to_string(
                                &serde_json::json!({ "error": error.to_string() }),
                            )
                            .unwrap_or_else(|_| error.to_string()),
                            true,
                        ),
                    };
                    // Track errors and successes for auto-learning.
                    let effective_name = crate::session::error_tracker::resolve_effective_tool_name(
                        &req.tool_name,
                        &req.input,
                    );
                    let category = if is_error {
                        crate::session::error_tracker::normalize_error_category(&output)
                    } else {
                        String::new()
                    };
                    // Feed the bifurcation ladder before the skill forge, so an
                    // escape detected here can label the skill it writes.
                    let escape = {
                        let mut engine = bifurcation
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        engine.note_attempt(&effective_name, &req.input);
                        engine.note_outcome(&effective_name, &req.input, is_error, &category);
                        engine.take_escape()
                    };
                    record_step_outcome(
                        &mut sample,
                        &effective_name,
                        &req.input,
                        &category,
                        is_error,
                        output.contains(crate::syntax_guard::SYNTAX_BACKPRESSURE_MARKER),
                    );
                    let output = if is_error {
                        let mut tracker = error_tracker
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        tracker.record_error(&effective_name, &output, &req.input);
                        // Per-tool skill hint: append fix directly to error output.
                        if let Some(hint) = tracker.get_skill_hint(&effective_name, &output) {
                            format!("{output}{hint}")
                        } else {
                            output
                        }
                    } else {
                        if let Some(skill) = error_tracker
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .record_success_with_escape(
                                &effective_name,
                                &req.input,
                                &output,
                                escape.as_ref(),
                            )
                        {
                            tracing::info!(
                                skill = %skill.name,
                                tool = %skill.tool_name,
                                "Dynamic skill created from recurring error pattern"
                            );
                        }
                        output
                    };
                    terminal_messages.push(ConversationMessage::tool_result(
                        req.tool_use_id,
                        req.tool_name,
                        output,
                        is_error,
                    ));
                } else {
                    // The parallel executor takes an `Fn`, so per-call outcomes
                    // land in a side buffer and are folded into the sample once
                    // the batch has finished.
                    let step_outcomes: Mutex<Vec<StepOutcome>> = Mutex::new(Vec::new());
                    let execution_results =
                        execute_parallel_batch(&calls_to_execute, |tool_name, input| {
                            let result = match executor.execute(tool_name, input) {
                                Ok(output) => (output, false),
                                Err(error) => (
                                    serde_json::to_string(
                                        &serde_json::json!({ "error": error.to_string() }),
                                    )
                                    .unwrap_or_else(|_| error.to_string()),
                                    true,
                                ),
                            };
                            // Track errors and successes for auto-learning.
                            let effective_name =
                                crate::session::error_tracker::resolve_effective_tool_name(
                                    tool_name, input,
                                );
                            let category = if result.1 {
                                crate::session::error_tracker::normalize_error_category(&result.0)
                            } else {
                                String::new()
                            };
                            let escape = {
                                let mut engine = bifurcation
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                engine.note_attempt(&effective_name, input);
                                engine.note_outcome(&effective_name, input, result.1, &category);
                                engine.take_escape()
                            };
                            step_outcomes
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push((
                                    effective_name.to_string(),
                                    input.to_string(),
                                    category,
                                    result.1,
                                    result
                                        .0
                                        .contains(crate::syntax_guard::SYNTAX_BACKPRESSURE_MARKER),
                                ));
                            let (output, is_error) = if result.1 {
                                let mut tracker = error_tracker
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                tracker.record_error(&effective_name, &result.0, input);
                                // Per-tool skill hint: append fix directly to error output.
                                let output = if let Some(hint) =
                                    tracker.get_skill_hint(&effective_name, &result.0)
                                {
                                    format!("{}{hint}", result.0)
                                } else {
                                    result.0
                                };
                                (output, true)
                            } else {
                                let _ = error_tracker
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .record_success_with_escape(
                                        &effective_name,
                                        input,
                                        &result.0,
                                        escape.as_ref(),
                                    );
                                result
                            };
                            (output, is_error)
                        });

                    for (result, req) in execution_results
                        .into_iter()
                        .zip(calls_to_execute.into_iter())
                    {
                        terminal_messages.push(ConversationMessage::tool_result(
                            req.tool_use_id,
                            req.tool_name,
                            result.output,
                            result.is_error,
                        ));
                    }

                    for (tool, input, category, is_error, syntax_hit) in step_outcomes
                        .into_inner()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                    {
                        record_step_outcome(
                            &mut sample,
                            &tool,
                            &input,
                            &category,
                            is_error,
                            syntax_hit,
                        );
                    }
                }

                ToolCallOutcome {
                    messages: terminal_messages,
                }
            });

            result_messages.extend(outcome.messages);
        }

        // Refused-by-gate results merge back in emission order so the API
        // pairing of tool_use blocks and tool_result blocks is preserved.
        result_messages.extend(gate_refusals.into_iter().map(|(_, message)| message));
        result_messages.sort_by_key(|message| {
            message
                .blocks
                .iter()
                .find_map(|block| match block {
                    ContentBlock::ToolResult { tool_use_id, .. } => {
                        emission_order.get(tool_use_id).copied()
                    }
                    _ => None,
                })
                .unwrap_or(usize::MAX)
        });

        // One trajectory point per iteration: this is where the ladder decides
        // whether the next iteration runs laminar or under a forced orbit split.
        let crossed = self
            .bifurcation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observe(&sample)
            .is_some();

        (result_messages, crossed)
    }

    /// Run a session health probe to verify the runtime is functional after compaction.
    /// Returns Ok(()) if healthy, Err if the session appears broken.
    fn run_session_health_probe(&mut self) -> Result<(), String> {
        // Check if we have basic session integrity
        if self.session.messages.is_empty() && self.session.compaction.is_some() {
            // Freshly compacted with no messages - this is normal
            return Ok(());
        }

        // Verify tool executor is responsive with a non-destructive probe
        // Using glob_search with a pattern that won't match anything
        let probe_input = r#"{"pattern": "*.health-check-probe-"}"#;
        match self.tool_executor.execute("glob_search", probe_input) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Tool executor probe failed: {e}")),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub fn run_turn(
        &mut self,
        user_input: impl Into<String>,
        mut prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<TurnSummary, RuntimeError> {
        let user_input = user_input.into();

        // ROADMAP #38: Session-health canary - probe if context was compacted
        if self.session.compaction.is_some() {
            if let Err(error) = self.run_session_health_probe() {
                return Err(RuntimeError::new(format!(
                    "Session health probe failed after compaction: {error}. \
                     The session may be in an inconsistent state. \
                     Consider starting a fresh session with /session new."
                )));
            }
        }

        self.record_turn_started(&user_input);
        tracing::info!(
            session_id = %self.session.session_id,
            input_len = user_input.len(),
            message_count = self.session.messages.len(),
            "turn started"
        );
        self.session
            .push_user_text(user_input.clone())
            .map_err(|error| RuntimeError::new(error.to_string()))?;
        // The turn's transactional working memory starts empty; the probe gate
        // carries the session's regime into this turn — a turn that begins past
        // the first bifurcation is not entitled to a blind write.
        crate::turn_state::begin_turn();
        self.bifurcation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .rearm_probe_gate_for_turn();

        let mut assistant_messages = Vec::new();
        let mut tool_results = Vec::new();
        let mut prompt_cache_events = Vec::new();
        let mut iterations = 0;
        let mut auto_compaction = None;

        loop {
            iterations += 1;
            if iterations > self.max_iterations {
                let error = RuntimeError::new(
                    "conversation loop exceeded the maximum number of iterations",
                );
                self.record_turn_failed(iterations, &error);
                return Err(error);
            }

            let mut system_prompt = self.system_prompt.clone();
            // Inject dynamic learned skills into the system prompt.
            if let Ok(tracker) = self.error_tracker.lock() {
                if let Some(skills_section) = tracker.prompt_section() {
                    system_prompt.push(skills_section);
                }
            }
            // Inject the bifurcation directive when accumulated resistance has
            // forced a wider thinking mode. Laminar flow adds nothing, so a
            // converging turn pays no token cost for the ladder.
            {
                let mut engine = self
                    .bifurcation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                engine.set_turn_file_summary(crate::turn_state::modified_files_summary());
                if let Some(directive) = engine.directive_section() {
                    system_prompt.push(directive);
                }
            }
            // Disabled automatic RAG context querying on initial user input to save tokens.
            // if iterations == 1 {
            //     if let Some(rag_context) = fetch_rag_context(&user_input) {
            //         system_prompt.push(rag_context);
            //     }
            // }

            let request = ApiRequest {
                system_prompt,
                messages: self.session.messages.clone(),
            };
            let mut retries = 0;
            let max_retries = 2;
            let (mut assistant_message, usage, turn_prompt_cache_events) = loop {
                let stream_res = self
                    .api_client
                    .stream(request.clone())
                    .and_then(build_assistant_message);

                match stream_res {
                    Ok(res) => break res,
                    Err(error) => {
                        // A shed or permanently rejected request is not
                        // transient: re-sending it defeats the breaker that
                        // just refused it.
                        if !error.is_terminal() && retries < max_retries {
                            retries += 1;
                            tracing::warn!(
                                iteration = iterations,
                                retry = retries,
                                error = %error,
                                "API stream failed, retrying after backoff..."
                            );
                            // No sleep here: re-entering `stream` runs the δ
                            // breaker's gate again, and that gate already
                            // escalates because the failed chain charged the
                            // shared breaker. Sleeping on top of it doubled the
                            // pause and blocked a tokio worker while doing so.
                            continue;
                        }
                        tracing::error!(
                            iteration = iterations,
                            error = %error,
                            "API stream failed after retries"
                        );
                        self.record_turn_failed(iterations, &error);
                        return Err(error);
                    }
                }
            };
            if let Some(usage) = usage {
                // The API layer paces requests against a *byte-derived* token
                // estimate; this is the provider's own count for the same
                // request. Logging both is what makes the estimate calibratable
                // instead of guessed — an estimate that is 1.8x too high costs
                // real wall-clock time in the TPM window.
                tracing::info!(
                    iteration = iterations,
                    model = self.session.model.as_deref().unwrap_or("unknown"),
                    input_tokens = usage.input_tokens,
                    cache_read_input_tokens = usage.cache_read_input_tokens,
                    cache_creation_input_tokens = usage.cache_creation_input_tokens,
                    output_tokens = usage.output_tokens,
                    "provider-reported token usage"
                );
                self.usage_tracker.record(usage);
            }
            prompt_cache_events.extend(turn_prompt_cache_events);
            recover_textual_tool_calls(&mut assistant_message);
            let pending_tool_uses = assistant_message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse {
                        id, name, input, ..
                    } => Some((id.clone(), name.clone(), input.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            self.record_assistant_iteration(
                iterations,
                &assistant_message,
                pending_tool_uses.len(),
            );

            self.session
                .push_message(assistant_message.clone())
                .map_err(|error| RuntimeError::new(error.to_string()))?;
            assistant_messages.push(assistant_message);

            // Run auto-compaction check before next API call, including on the terminal
            // (no-tool) iteration, to prevent unbounded session growth (#3106).
            if let Some(compaction) = self.maybe_auto_compact() {
                tracing::warn!(
                    removed = compaction.removed_message_count,
                    "auto-compaction triggered"
                );
                auto_compaction = Some(compaction);
            }

            if pending_tool_uses.is_empty() {
                break;
            }

            let (dispatch_results, ladder_crossed) =
                self.dispatch_tool_calls(pending_tool_uses, iterations, prompter.as_deref_mut());
            for result_message in dispatch_results {
                self.session
                    .push_message(result_message.clone())
                    .map_err(|error| RuntimeError::new(error.to_string()))?;
                self.record_tool_finished(iterations, &result_message);
                tool_results.push(result_message);
            }

            // Maxwell's demon (M1): a crossing means the orbits documented in
            // this turn's older error results are provably dead. Collapse them
            // in the session context — ids stay, the model stops re-reading
            // stacktraces it has already been forced past.
            if ladder_crossed {
                let pruned = self
                    .bifurcation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .prune_dead_orbit_outputs(&mut self.session.messages);
                if pruned > 0 {
                    tracing::info!(
                        pruned,
                        iteration = iterations,
                        "dead-orbit context pruned after bifurcation crossing"
                    );
                }
            }

            // Past the accumulation point for too many steps, more iterations are
            // waste rather than search. Off unless CLAW_FRACTAL_HALT_AT_CHAOS is set.
            let halt = self
                .bifurcation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .halt_requested();
            if halt {
                let error = RuntimeError::terminal(format!(
                    "Bifurcation ladder reached its chaos point (r∞) and stayed past it for {} \
                     steps without leaving the basin. The turn was ended to stop an \
                     unproductive loop. Human input needed: the approaches tried are listed in \
                     the last directives, and the measured blocker is in the tool results.",
                    crate::fractal::bifurcation::CHAOS_HALT_STEPS
                ));
                self.record_turn_failed(iterations, &error);
                return Err(error);
            }
        }

        let summary = TurnSummary {
            assistant_messages,
            tool_results,
            prompt_cache_events,
            iterations,
            usage: self.usage_tracker.cumulative_usage(),
            auto_compaction,
            bifurcation_regime: self
                .bifurcation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .regime(),
        };
        tracing::info!(
            iterations = summary.iterations,
            assistant_msgs = summary.assistant_messages.len(),
            tool_results = summary.tool_results.len(),
            total_tokens = summary.usage.total_tokens(),
            bifurcation_regime = summary.bifurcation_regime.as_str(),
            "turn completed"
        );
        self.record_turn_completed(&summary);

        Ok(summary)
    }

    #[must_use]
    pub fn compact(&self, config: CompactionConfig) -> CompactionResult {
        compact_session(&self.session, config)
    }

    #[must_use]
    pub fn estimated_tokens(&self) -> usize {
        estimate_session_tokens(&self.session)
    }

    #[must_use]
    pub fn usage(&self) -> &UsageTracker {
        &self.usage_tracker
    }

    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn api_client_mut(&mut self) -> &mut C {
        &mut self.api_client
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Returns a reference to the error tracker for shutdown processing.
    pub fn error_tracker(&self) -> std::sync::MutexGuard<'_, ErrorTracker> {
        self.error_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[must_use]
    pub fn fork_session(&self, branch_name: Option<String>) -> Session {
        self.session.fork(branch_name)
    }

    #[must_use]
    pub fn into_session(self) -> Session {
        self.session
    }

    fn maybe_auto_compact(&mut self) -> Option<AutoCompactionEvent> {
        if self.estimated_tokens() < self.auto_compaction_input_tokens_threshold as usize {
            return None;
        }

        let result = compact_session(
            &self.session,
            CompactionConfig {
                max_estimated_tokens: 0,
                ..CompactionConfig::default()
            },
        );

        if result.removed_message_count == 0 {
            return None;
        }

        self.session = result.compacted_session;
        Some(AutoCompactionEvent {
            removed_message_count: result.removed_message_count,
        })
    }

    fn record_turn_started(&self, user_input: &str) {
        let Some(session_tracer) = &self.session_tracer else {
            return;
        };

        let mut attributes = Map::new();
        attributes.insert(
            "user_input".to_string(),
            Value::String(user_input.to_string()),
        );
        session_tracer.record("turn_started", attributes);
    }

    fn record_assistant_iteration(
        &self,
        iteration: usize,
        assistant_message: &ConversationMessage,
        pending_tool_use_count: usize,
    ) {
        let Some(session_tracer) = &self.session_tracer else {
            return;
        };

        let mut attributes = Map::new();
        attributes.insert("iteration".to_string(), Value::from(iteration as u64));
        attributes.insert(
            "assistant_blocks".to_string(),
            Value::from(assistant_message.blocks.len() as u64),
        );
        attributes.insert(
            "pending_tool_use_count".to_string(),
            Value::from(pending_tool_use_count as u64),
        );
        session_tracer.record("assistant_iteration_completed", attributes);
    }

    fn record_tool_finished(&self, iteration: usize, result_message: &ConversationMessage) {
        let Some(session_tracer) = &self.session_tracer else {
            return;
        };

        let Some(ContentBlock::ToolResult {
            tool_name,
            is_error,
            ..
        }) = result_message.blocks.first()
        else {
            return;
        };

        let mut attributes = Map::new();
        attributes.insert("iteration".to_string(), Value::from(iteration as u64));
        attributes.insert("tool_name".to_string(), Value::String(tool_name.clone()));
        attributes.insert("is_error".to_string(), Value::Bool(*is_error));
        session_tracer.record("tool_execution_finished", attributes);
    }

    fn record_turn_completed(&self, summary: &TurnSummary) {
        let Some(session_tracer) = &self.session_tracer else {
            return;
        };

        let mut attributes = Map::new();
        attributes.insert(
            "iterations".to_string(),
            Value::from(summary.iterations as u64),
        );
        attributes.insert(
            "assistant_messages".to_string(),
            Value::from(summary.assistant_messages.len() as u64),
        );
        attributes.insert(
            "tool_results".to_string(),
            Value::from(summary.tool_results.len() as u64),
        );
        attributes.insert(
            "prompt_cache_events".to_string(),
            Value::from(summary.prompt_cache_events.len() as u64),
        );
        session_tracer.record("turn_completed", attributes);
    }

    fn record_turn_failed(&self, iteration: usize, error: &RuntimeError) {
        let Some(session_tracer) = &self.session_tracer else {
            return;
        };

        let mut attributes = Map::new();
        attributes.insert("iteration".to_string(), Value::from(iteration as u64));
        attributes.insert("error".to_string(), Value::String(error.to_string()));
        session_tracer.record("turn_failed", attributes);
    }
}

/// Reads the automatic compaction threshold from the environment.
#[must_use]
pub fn auto_compaction_threshold_from_env() -> u32 {
    parse_auto_compaction_threshold(
        std::env::var(AUTO_COMPACTION_THRESHOLD_ENV_VAR)
            .ok()
            .as_deref(),
    )
}

/// Opt in to ending a turn once the bifurcation ladder sits past its accumulation
/// point for too long. Off unless `CLAW_FRACTAL_HALT_AT_CHAOS` is truthy.
#[must_use]
fn bifurcation_halt_from_env() -> bool {
    matches!(
        std::env::var(BIFURCATION_HALT_ENV_VAR)
            .ok()
            .map(|raw| raw.trim().to_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

#[must_use]
fn parse_auto_compaction_threshold(value: Option<&str>) -> u32 {
    value
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|threshold| *threshold > 0)
        .unwrap_or(DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD)
}

/// One executed call as the parallel batch reports it back: effective tool name,
/// raw input, error category, whether it failed, whether the syntax guard
/// flagged the artifact.
type StepOutcome = (String, String, String, bool, bool);

/// Fold one tool outcome into the orbit sample for the current iteration.
fn record_step_outcome(
    sample: &mut OrbitSample,
    tool: &str,
    input: &str,
    category: &str,
    is_error: bool,
    syntax_hit: bool,
) {
    if syntax_hit {
        sample.syntax_backpressure = true;
    }
    if is_error {
        let fingerprint = BifurcationEngine::fingerprint(tool, category);
        if BifurcationEngine::is_transient_category(category) {
            sample.transient_failures.push(fingerprint.clone());
        }
        sample.failures.push(fingerprint);
        return;
    }
    if BifurcationEngine::is_probe_call(tool, input) {
        sample.successful_probe = true;
    }
    if !BifurcationEngine::is_read_only_tool(tool) {
        sample.mutated_state = true;
    }
    sample.successes.push(tool.to_string());
}

fn build_assistant_message(
    events: Vec<AssistantEvent>,
) -> Result<
    (
        ConversationMessage,
        Option<TokenUsage>,
        Vec<PromptCacheEvent>,
    ),
    RuntimeError,
> {
    let mut text = String::new();
    let mut blocks = Vec::new();
    let mut prompt_cache_events = Vec::new();
    let mut finished = false;
    let mut usage = None;

    for event in events {
        match event {
            AssistantEvent::Thinking {
                thinking,
                signature,
            } => {
                flush_text_block(&mut text, &mut blocks);
                blocks.push(ContentBlock::Thinking {
                    thinking,
                    signature,
                });
            }
            AssistantEvent::TextDelta(delta) => text.push_str(&delta),
            AssistantEvent::ToolUse {
                id,
                name,
                input,
                signature,
            } => {
                flush_text_block(&mut text, &mut blocks);
                blocks.push(ContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    signature,
                });
            }
            AssistantEvent::Usage(value) => usage = Some(value),
            AssistantEvent::PromptCache(event) => prompt_cache_events.push(event),
            AssistantEvent::MessageStop => {
                finished = true;
            }
        }
    }

    flush_text_block(&mut text, &mut blocks);

    if !finished {
        return Err(RuntimeError::new(
            "assistant stream ended without a message stop event",
        ));
    }
    if blocks.is_empty() {
        return Err(RuntimeError::new("assistant stream produced no content"));
    }

    Ok((
        ConversationMessage::assistant_with_usage(blocks, usage),
        usage,
        prompt_cache_events,
    ))
}

fn flush_text_block(text: &mut String, blocks: &mut Vec<ContentBlock>) {
    if !text.is_empty() {
        blocks.push(ContentBlock::Text {
            text: std::mem::take(text),
        });
    }
}

#[allow(dead_code)]
fn fetch_rag_context(query: &str) -> Option<String> {
    let client = reqwest::blocking::Client::new();
    let body = serde_json::json!({
        "query": query,
        "top_k": 5
    });
    let res = client
        .post("http://127.0.0.1:8787/v1/query")
        .timeout(std::time::Duration::from_secs(2))
        .json(&body)
        .send()
        .ok()?;

    #[derive(serde::Deserialize)]
    struct RagHit {
        path: String,
        snippet: String,
    }

    #[derive(serde::Deserialize)]
    struct RagResponse {
        hits: Vec<RagHit>,
    }

    let response: RagResponse = res.json().ok()?;
    if response.hits.is_empty() {
        None
    } else {
        let hits_str = response
            .hits
            .into_iter()
            .map(|h| format!("File: {}\n{}", h.path, h.snippet))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Some(format!("# Retrieved RAG Context\nThe following historical context or related code snippets were dynamically retrieved based on the user's latest query:\n\n{}", hits_str))
    }
}

type ToolHandler = Box<dyn FnMut(&str) -> Result<String, ToolError> + Send + Sync>;

/// Simple in-memory tool executor for tests and lightweight integrations.
#[derive(Default)]
pub struct StaticToolExecutor {
    handlers: std::sync::Mutex<BTreeMap<String, ToolHandler>>,
}

impl StaticToolExecutor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn register(
        mut self,
        tool_name: impl Into<String>,
        handler: impl FnMut(&str) -> Result<String, ToolError> + Send + Sync + 'static,
    ) -> Self {
        self.handlers
            .get_mut()
            .unwrap()
            .insert(tool_name.into(), Box::new(handler));
        self
    }
}

impl ToolExecutor for StaticToolExecutor {
    fn execute(&self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        let mut handlers = self.handlers.lock().unwrap();
        handlers
            .get_mut(tool_name)
            .ok_or_else(|| ToolError::new(format!("unknown tool: {tool_name}")))?(input)
    }
}

/// Fallback extractor for models that emit tool calls as plain text or pseudo-JSON
pub fn parse_textual_tool_call(text: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Pattern 1: <tool_call> ... </tool_call>
    if let Some(start_idx) = trimmed.find("<tool_call>") {
        if let Some(end_idx) = trimmed[start_idx..].find("</tool_call>") {
            let inner = trimmed[start_idx + 11..start_idx + end_idx].trim();
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(inner) {
                if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                    let input = val
                        .get("arguments")
                        .or_else(|| val.get("input"))
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    return Some((name.to_string(), input));
                }
            }
        }
    }

    // Pattern 2: Markdown code block ```json ... ```
    if trimmed.starts_with("```") {
        let stripped = trimmed
            .trim_start_matches('`')
            .trim_start_matches("json")
            .trim_end_matches('`')
            .trim();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(stripped) {
            if let Some(name) = val
                .get("name")
                .or_else(|| val.get("tool"))
                .and_then(|n| n.as_str())
            {
                let input = val
                    .get("arguments")
                    .or_else(|| val.get("input"))
                    .or_else(|| val.get("parameters"))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                return Some((name.to_string(), input));
            }
        }
    }

    // Pattern 3: Missing opening JSON bracket / direct key-value syntax:
    // e.g. `mcp__sequential-thinking__sequentialthinking": {"prompt": ...}`
    // or `"mcp__...": { ... }` or `mcp__...: { ... }`
    let clean_lead = trimmed.trim_start_matches('{').trim();
    if let Some((first_part, rest)) = clean_lead.split_once(':') {
        let tool_name = first_part
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim();
        let is_valid_tool_name = !tool_name.is_empty()
            && (tool_name.starts_with("mcp__")
                || tool_name.starts_with("mcp_")
                || tool_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));

        if is_valid_tool_name {
            let rest_trimmed = rest.trim();
            if rest_trimmed.starts_with('{') {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(rest_trimmed) {
                    if val.is_object() {
                        return Some((tool_name.to_string(), val));
                    }
                }
                if let Some(last_brace) = rest_trimmed.rfind('}') {
                    let sub = &rest_trimmed[..=last_brace];
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(sub) {
                        if val.is_object() {
                            return Some((tool_name.to_string(), val));
                        }
                    }
                }
            }
        }
    }

    // Pattern 4: Line-based tool invocation:
    // "read_file\n{\"path\": \"...\"}" or "bash\n{\"command\": \"...\"}"
    if let Some((first_line, rest)) = trimmed.split_once('\n') {
        let tool_name = first_line.trim().trim_end_matches(':').trim();
        let is_valid_tool_name = !tool_name.is_empty()
            && tool_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

        if is_valid_tool_name {
            let rest_trimmed = rest.trim();
            if rest_trimmed.starts_with('{') && rest_trimmed.ends_with('}') {
                if let Ok(input) = serde_json::from_str::<serde_json::Value>(rest_trimmed) {
                    if input.is_object() {
                        return Some((tool_name.to_string(), input));
                    }
                }
            }
        }
    }

    None
}

/// Recovers tool calls that the model accidentally emitted as textual output
/// rather than structured API tool_use blocks (e.g. Codestral, DeepSeek, Qwen, Mistral).
fn recover_textual_tool_calls(message: &mut ConversationMessage) {
    let has_structured_tool_use = message
        .blocks
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
    if has_structured_tool_use {
        return;
    }

    let mut recovered_blocks = Vec::new();
    for block in &message.blocks {
        if let ContentBlock::Text { text } = block {
            if let Some((name, input)) = parse_textual_tool_call(text) {
                let id = format!(
                    "tool_rec_{}_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    recovered_blocks.len()
                );
                recovered_blocks.push(ContentBlock::ToolUse {
                    id,
                    name,
                    input: input.to_string(),
                    signature: None,
                });
            }
        }
    }

    if !recovered_blocks.is_empty() {
        message.blocks.extend(recovered_blocks);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_assistant_message, parse_auto_compaction_threshold, ApiClient, ApiRequest,
        AssistantEvent, AutoCompactionEvent, ConversationRuntime, PromptCacheEvent, RuntimeError,
        StaticToolExecutor, ToolExecutor, DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD,
    };
    use crate::compact::CompactionConfig;
    use crate::config::{RuntimeFeatureConfig, RuntimeHookConfig};
    use crate::fractal::Regime;
    use crate::prompt::{ProjectContext, SystemPromptBuilder};
    use crate::security::permissions::{
        PermissionMode, PermissionPolicy, PermissionPromptDecision, PermissionPrompter,
        PermissionRequest,
    };
    use crate::session::state::{ContentBlock, MessageRole, Session};
    use crate::session::usage::TokenUsage;
    use crate::ToolError;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use telemetry::{MemoryTelemetrySink, SessionTracer, TelemetryEvent};

    struct ScriptedApiClient {
        call_count: usize,
    }

    impl ApiClient for ScriptedApiClient {
        fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
            self.call_count += 1;
            match self.call_count {
                1 => {
                    assert!(request
                        .messages
                        .iter()
                        .any(|message| message.role == MessageRole::User));
                    Ok(vec![
                        AssistantEvent::TextDelta("Let me calculate that.".to_string()),
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "add".to_string(),
                            input: "2,2".to_string(),
                            signature: None,
                        },
                        AssistantEvent::Usage(TokenUsage {
                            input_tokens: 20,
                            output_tokens: 6,
                            cache_creation_input_tokens: 1,
                            cache_read_input_tokens: 2,
                        }),
                        AssistantEvent::MessageStop,
                    ])
                }
                2 => {
                    let last_message = request
                        .messages
                        .last()
                        .expect("tool result should be present");
                    assert_eq!(last_message.role, MessageRole::Tool);
                    Ok(vec![
                        AssistantEvent::TextDelta("The answer is 4.".to_string()),
                        AssistantEvent::Usage(TokenUsage {
                            input_tokens: 24,
                            output_tokens: 4,
                            cache_creation_input_tokens: 1,
                            cache_read_input_tokens: 3,
                        }),
                        AssistantEvent::PromptCache(PromptCacheEvent {
                            unexpected: true,
                            reason:
                                "cache read tokens dropped while prompt fingerprint remained stable"
                                    .to_string(),
                            previous_cache_read_input_tokens: 6_000,
                            current_cache_read_input_tokens: 1_000,
                            token_drop: 5_000,
                        }),
                        AssistantEvent::MessageStop,
                    ])
                }
                _ => unreachable!("extra API call"),
            }
        }
    }

    struct PromptAllowOnce;

    impl PermissionPrompter for PromptAllowOnce {
        fn decide(&mut self, request: &PermissionRequest) -> PermissionPromptDecision {
            assert_eq!(request.tool_name, "add");
            PermissionPromptDecision::Allow
        }
    }

    #[test]
    fn runs_user_to_tool_to_result_loop_end_to_end_and_tracks_usage() {
        let api_client = ScriptedApiClient { call_count: 0 };
        let tool_executor = StaticToolExecutor::new().register("add", |input| {
            let total = input
                .split(',')
                .map(|part| part.parse::<i32>().expect("input must be valid integer"))
                .sum::<i32>();
            Ok(total.to_string())
        });
        let permission_policy = PermissionPolicy::new(PermissionMode::WorkspaceWrite);
        let system_prompt = SystemPromptBuilder::new()
            .with_project_context(ProjectContext {
                cwd: PathBuf::from("/tmp/project"),
                current_date: "2026-03-31".to_string(),
                git_status: None,
                git_diff: None,
                git_context: None,
                instruction_files: Vec::new(),
                file_tree: None,
            })
            .with_os("linux", "6.8")
            .build();
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            api_client,
            tool_executor,
            permission_policy,
            system_prompt,
        );

        let summary = runtime
            .run_turn("what is 2 + 2?", Some(&mut PromptAllowOnce))
            .expect("conversation loop should succeed");

        assert_eq!(summary.iterations, 2);
        assert_eq!(summary.assistant_messages.len(), 2);
        assert_eq!(summary.tool_results.len(), 1);
        assert_eq!(summary.prompt_cache_events.len(), 1);
        assert_eq!(runtime.session().messages.len(), 4);
        assert_eq!(summary.usage.output_tokens, 10);
        assert_eq!(summary.auto_compaction, None);
        assert!(matches!(
            runtime.session().messages[1].blocks[1],
            ContentBlock::ToolUse { .. }
        ));
        assert!(matches!(
            runtime.session().messages[2].blocks[0],
            ContentBlock::ToolResult {
                is_error: false,
                ..
            }
        ));
    }

    #[test]
    fn records_runtime_session_trace_events() {
        let sink = Arc::new(MemoryTelemetrySink::default());
        let tracer = SessionTracer::new("session-runtime", sink.clone());
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            ScriptedApiClient { call_count: 0 },
            StaticToolExecutor::new().register("add", |_input| Ok("4".to_string())),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        )
        .with_session_tracer(tracer);

        runtime
            .run_turn("what is 2 + 2?", Some(&mut PromptAllowOnce))
            .expect("conversation loop should succeed");

        let events = sink.events();
        let trace_names = events
            .iter()
            .filter_map(|event| match event {
                TelemetryEvent::SessionTrace(trace) => Some(trace.name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(trace_names.contains(&"turn_started"));
        assert!(trace_names.contains(&"assistant_iteration_completed"));
        assert!(trace_names.contains(&"tool_execution_started"));
        assert!(trace_names.contains(&"tool_execution_finished"));
        assert!(trace_names.contains(&"turn_completed"));
    }

    #[test]
    fn records_denied_tool_results_when_prompt_rejects() {
        struct RejectPrompter;
        impl PermissionPrompter for RejectPrompter {
            fn decide(&mut self, _request: &PermissionRequest) -> PermissionPromptDecision {
                PermissionPromptDecision::Deny {
                    reason: "not now".to_string(),
                }
            }
        }

        struct SingleCallApiClient;
        impl ApiClient for SingleCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                if request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool)
                {
                    return Ok(vec![
                        AssistantEvent::TextDelta("I could not use the tool.".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
                }
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: "tool-1".to_string(),
                        name: "blocked".to_string(),
                        input: "secret".to_string(),
                        signature: None,
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            SingleCallApiClient,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::WorkspaceWrite),
            vec!["system".to_string()],
        );

        let summary = runtime
            .run_turn("use the tool", Some(&mut RejectPrompter))
            .expect("conversation should continue after denied tool");

        assert_eq!(summary.tool_results.len(), 1);
        assert!(matches!(
            &summary.tool_results[0].blocks[0],
            ContentBlock::ToolResult { is_error: true, output, .. } if output.ends_with("not now")
        ));
    }

    #[test]
    fn denies_tool_use_when_pre_tool_hook_blocks() {
        struct SingleCallApiClient;
        impl ApiClient for SingleCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                if request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool)
                {
                    return Ok(vec![
                        AssistantEvent::TextDelta("blocked".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
                }
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: "tool-1".to_string(),
                        name: "blocked".to_string(),
                        input: r#"{"path":"secret.txt"}"#.to_string(),
                        signature: None,
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            SingleCallApiClient,
            StaticToolExecutor::new().register("blocked", |_input| {
                panic!("tool should not execute when hook denies")
            }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            &RuntimeFeatureConfig::default().with_hooks(RuntimeHookConfig::new(
                vec![shell_snippet("printf 'blocked by hook'; exit 2")],
                Vec::new(),
                Vec::new(),
            )),
        );

        let summary = runtime
            .run_turn("use the tool", None)
            .expect("conversation should continue after hook denial");

        assert_eq!(summary.tool_results.len(), 1);
        let ContentBlock::ToolResult {
            is_error, output, ..
        } = &summary.tool_results[0].blocks[0]
        else {
            panic!("expected tool result block");
        };
        assert!(
            *is_error,
            "hook denial should produce an error result: {output}"
        );
        assert!(
            output.contains("denied tool") || output.contains("blocked by hook"),
            "unexpected hook denial output: {output:?}"
        );
    }

    #[test]
    fn denies_tool_use_when_pre_tool_hook_fails() {
        struct SingleCallApiClient;
        impl ApiClient for SingleCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                if request
                    .messages
                    .iter()
                    .any(|message| message.role == MessageRole::Tool)
                {
                    return Ok(vec![
                        AssistantEvent::TextDelta("failed".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
                }
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: "tool-1".to_string(),
                        name: "blocked".to_string(),
                        input: r#"{"path":"secret.txt"}"#.to_string(),
                        signature: None,
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        // given
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            SingleCallApiClient,
            StaticToolExecutor::new().register("blocked", |_input| {
                panic!("tool should not execute when hook fails")
            }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            &RuntimeFeatureConfig::default().with_hooks(RuntimeHookConfig::new(
                vec![shell_snippet("printf 'broken hook'; exit 1")],
                Vec::new(),
                Vec::new(),
            )),
        );

        // when
        let summary = runtime
            .run_turn("use the tool", None)
            .expect("conversation should continue after hook failure");

        // then
        assert_eq!(summary.tool_results.len(), 1);
        let ContentBlock::ToolResult {
            is_error, output, ..
        } = &summary.tool_results[0].blocks[0]
        else {
            panic!("expected tool result block");
        };
        assert!(
            *is_error,
            "hook failure should produce an error result: {output}"
        );
        assert!(
            output.contains("exited with status 1") || output.contains("broken hook"),
            "unexpected hook failure output: {output:?}"
        );
    }

    #[test]
    fn appends_post_tool_hook_feedback_to_tool_result() {
        struct TwoCallApiClient {
            calls: usize,
        }

        impl ApiClient for TwoCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.calls += 1;
                match self.calls {
                    1 => Ok(vec![
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "add".to_string(),
                            input: r#"{"lhs":2,"rhs":2}"#.to_string(),
                            signature: None,
                        },
                        AssistantEvent::MessageStop,
                    ]),
                    2 => {
                        assert!(request
                            .messages
                            .iter()
                            .any(|message| message.role == MessageRole::Tool));
                        Ok(vec![
                            AssistantEvent::TextDelta("done".to_string()),
                            AssistantEvent::MessageStop,
                        ])
                    }
                    _ => unreachable!("extra API call"),
                }
            }
        }

        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            TwoCallApiClient { calls: 0 },
            StaticToolExecutor::new().register("add", |_input| Ok("4".to_string())),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            &RuntimeFeatureConfig::default().with_hooks(RuntimeHookConfig::new(
                vec![shell_snippet("printf 'pre hook ran'")],
                vec![shell_snippet("printf 'post hook ran'")],
                Vec::new(),
            )),
        );

        let summary = runtime
            .run_turn("use add", None)
            .expect("tool loop succeeds");

        assert_eq!(summary.tool_results.len(), 1);
        let ContentBlock::ToolResult {
            is_error, output, ..
        } = &summary.tool_results[0].blocks[0]
        else {
            panic!("expected tool result block");
        };
        assert!(
            !*is_error,
            "post hook should preserve non-error result: {output:?}"
        );
        assert!(
            output.contains('4'),
            "tool output missing value: {output:?}"
        );
        assert!(
            output.contains("pre hook ran"),
            "tool output missing pre hook feedback: {output:?}"
        );
        assert!(
            output.contains("post hook ran"),
            "tool output missing post hook feedback: {output:?}"
        );
    }

    #[test]
    fn appends_post_tool_use_failure_hook_feedback_to_tool_result() {
        struct TwoCallApiClient {
            calls: usize,
        }

        impl ApiClient for TwoCallApiClient {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.calls += 1;
                match self.calls {
                    1 => Ok(vec![
                        AssistantEvent::ToolUse {
                            id: "tool-1".to_string(),
                            name: "fail".to_string(),
                            input: r#"{"path":"README.md"}"#.to_string(),
                            signature: None,
                        },
                        AssistantEvent::MessageStop,
                    ]),
                    2 => {
                        assert!(request
                            .messages
                            .iter()
                            .any(|message| message.role == MessageRole::Tool));
                        Ok(vec![
                            AssistantEvent::TextDelta("done".to_string()),
                            AssistantEvent::MessageStop,
                        ])
                    }
                    _ => unreachable!("extra API call"),
                }
            }
        }

        // given
        let mut runtime = ConversationRuntime::new_with_features(
            Session::new(),
            TwoCallApiClient { calls: 0 },
            StaticToolExecutor::new()
                .register("fail", |_input| Err(ToolError::new("tool exploded"))),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
            &RuntimeFeatureConfig::default().with_hooks(RuntimeHookConfig::new(
                Vec::new(),
                vec![shell_snippet("printf 'post hook should not run'")],
                vec![shell_snippet("printf 'failure hook ran'")],
            )),
        );

        // when
        let summary = runtime
            .run_turn("use fail", None)
            .expect("tool loop succeeds");

        // then
        assert_eq!(summary.tool_results.len(), 1);
        let ContentBlock::ToolResult {
            is_error, output, ..
        } = &summary.tool_results[0].blocks[0]
        else {
            panic!("expected tool result block");
        };
        assert!(
            *is_error,
            "failure hook path should preserve error result: {output:?}"
        );
        assert!(
            output.contains("tool exploded"),
            "tool output missing failure reason: {output:?}"
        );
        assert!(
            output.contains("failure hook ran"),
            "tool output missing failure hook feedback: {output:?}"
        );
        assert!(
            !output.contains("post hook should not run"),
            "normal post hook should not run on tool failure: {output:?}"
        );
    }

    #[test]
    fn reconstructs_usage_tracker_from_restored_session() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut session = Session::new();
        session.messages.push(
            crate::session::state::ConversationMessage::assistant_with_usage(
                vec![ContentBlock::Text {
                    text: "earlier".to_string(),
                }],
                Some(TokenUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                    cache_creation_input_tokens: 2,
                    cache_read_input_tokens: 1,
                }),
            ),
        );

        let runtime = ConversationRuntime::new(
            session,
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        assert_eq!(runtime.usage().turns(), 1);
        assert_eq!(runtime.usage().cumulative_usage().total_tokens(), 21);
    }

    #[test]
    fn compacts_session_after_turns() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );
        runtime.run_turn("a", None).expect("turn a");
        runtime.run_turn("b", None).expect("turn b");
        runtime.run_turn("c", None).expect("turn c");

        let result = runtime.compact(CompactionConfig {
            preserve_recent_messages: 2,
            max_estimated_tokens: 1,
        });
        assert!(result.summary.contains("Conversation summary"));
        assert_eq!(
            result.compacted_session.messages[0].role,
            MessageRole::System
        );
        assert_eq!(
            result.compacted_session.session_id,
            runtime.session().session_id
        );
        assert!(result.compacted_session.compaction.is_some());
    }

    #[test]
    fn persists_conversation_turn_messages_to_jsonl_session() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let path = temp_session_path("persisted-turn");
        let session = Session::new().with_persistence_path(path.clone());
        let mut runtime = ConversationRuntime::new(
            session,
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        runtime
            .run_turn("persist this turn", None)
            .expect("turn should succeed");

        let restored = Session::load_from_path(&path).expect("persisted session should reload");
        fs::remove_file(&path).expect("temp session file should be removable");

        assert_eq!(restored.messages.len(), 2);
        assert_eq!(restored.messages[0].role, MessageRole::User);
        assert_eq!(restored.messages[1].role, MessageRole::Assistant);
        assert_eq!(restored.session_id, runtime.session().session_id);
    }

    #[test]
    fn forks_runtime_session_without_mutating_original() {
        let mut session = Session::new();
        session
            .push_user_text("branch me")
            .expect("message should append");

        let runtime = ConversationRuntime::new(
            session.clone(),
            ScriptedApiClient { call_count: 0 },
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        let forked = runtime.fork_session(Some("alt-path".to_string()));

        assert_eq!(forked.messages, session.messages);
        assert_ne!(forked.session_id, session.session_id);
        assert_eq!(
            forked
                .fork
                .as_ref()
                .map(|fork| (fork.parent_session_id.as_str(), fork.branch_name.as_deref())),
            Some((session.session_id.as_str(), Some("alt-path")))
        );
        assert!(runtime.session().fork.is_none());
    }

    fn temp_session_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("runtime-conversation-{label}-{nanos}.json"))
    }

    #[cfg(windows)]
    fn shell_snippet(script: &str) -> String {
        script.replace('\'', "\"")
    }

    #[cfg(not(windows))]
    fn shell_snippet(script: &str) -> String {
        script.to_string()
    }

    #[test]
    fn auto_compacts_when_estimated_input_threshold_is_crossed() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::Usage(TokenUsage {
                        input_tokens: 120_000,
                        output_tokens: 4,
                        cache_creation_input_tokens: 0,
                        cache_read_input_tokens: 0,
                    }),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut session = Session::new();
        session.messages = vec![
            crate::session::state::ConversationMessage::user_text("one"),
            crate::session::state::ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "two".to_string(),
            }]),
            crate::session::state::ConversationMessage::user_text("three"),
            crate::session::state::ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "four".to_string(),
            }]),
        ];

        let mut runtime = ConversationRuntime::new(
            session,
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_auto_compaction_input_tokens_threshold(5);

        let summary = runtime
            .run_turn("trigger", None)
            .expect("turn should succeed");

        assert_eq!(
            summary.auto_compaction,
            Some(AutoCompactionEvent {
                removed_message_count: 2,
            })
        );
        assert_eq!(runtime.session().messages[0].role, MessageRole::System);
    }

    #[test]
    fn skips_auto_compaction_below_threshold() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::Usage(TokenUsage {
                        input_tokens: 99_999,
                        output_tokens: 4,
                        cache_creation_input_tokens: 0,
                        cache_read_input_tokens: 0,
                    }),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            SimpleApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_auto_compaction_input_tokens_threshold(100_000);

        let summary = runtime
            .run_turn("trigger", None)
            .expect("turn should succeed");
        assert_eq!(summary.auto_compaction, None);
        assert_eq!(runtime.session().messages.len(), 2);
    }

    #[test]
    fn auto_compaction_threshold_defaults_and_parses_values() {
        assert_eq!(
            parse_auto_compaction_threshold(None),
            DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD
        );
        assert_eq!(parse_auto_compaction_threshold(Some("4321")), 4321);
        assert_eq!(
            parse_auto_compaction_threshold(Some("0")),
            DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD
        );
        assert_eq!(
            parse_auto_compaction_threshold(Some("not-a-number")),
            DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD
        );
    }

    #[test]
    fn compaction_health_probe_blocks_turn_when_tool_executor_is_broken() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                panic!("API should not run when health probe fails");
            }
        }

        let mut session = Session::new();
        session.record_compaction("summarized earlier work", 4);
        session
            .push_user_text("previous message")
            .expect("message should append");

        let tool_executor = StaticToolExecutor::new().register("glob_search", |_input| {
            Err(ToolError::new("transport unavailable"))
        });
        let mut runtime = ConversationRuntime::new(
            session,
            SimpleApi,
            tool_executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        let error = runtime
            .run_turn("trigger", None)
            .expect_err("health probe failure should abort the turn");
        assert!(
            error
                .to_string()
                .contains("Session health probe failed after compaction"),
            "unexpected error: {error}"
        );
        assert!(
            error.to_string().contains("transport unavailable"),
            "expected underlying probe error: {error}"
        );
    }

    #[test]
    fn compaction_health_probe_skips_empty_compacted_session() {
        struct SimpleApi;
        impl ApiClient for SimpleApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::TextDelta("done".to_string()),
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut session = Session::new();
        session.record_compaction("fresh summary", 2);

        let tool_executor = StaticToolExecutor::new().register("glob_search", |_input| {
            Err(ToolError::new(
                "glob_search should not run for an empty compacted session",
            ))
        });
        let mut runtime = ConversationRuntime::new(
            session,
            SimpleApi,
            tool_executor,
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        let summary = runtime
            .run_turn("trigger", None)
            .expect("empty compacted session should not fail health probe");
        assert_eq!(summary.auto_compaction, None);
        assert_eq!(runtime.session().messages.len(), 2);
    }

    #[test]
    fn build_assistant_message_requires_message_stop_event() {
        // given
        let events = vec![AssistantEvent::TextDelta("hello".to_string())];

        // when
        let error = build_assistant_message(events)
            .expect_err("assistant messages should require a stop event");

        // then
        assert!(error
            .to_string()
            .contains("assistant stream ended without a message stop event"));
    }

    #[test]
    fn build_assistant_message_requires_content() {
        // given
        let events = vec![AssistantEvent::MessageStop];

        // when
        let error =
            build_assistant_message(events).expect_err("assistant messages should require content");

        // then
        assert!(error
            .to_string()
            .contains("assistant stream produced no content"));
    }

    #[test]
    fn build_assistant_message_places_thinking_block_before_text_and_tool_use() {
        // given
        let events = vec![
            AssistantEvent::Thinking {
                thinking: "pondering".to_string(),
                signature: Some("sig".to_string()),
            },
            AssistantEvent::TextDelta("hello".to_string()),
            AssistantEvent::ToolUse {
                id: "tool-1".to_string(),
                name: "echo".to_string(),
                input: "payload".to_string(),
                signature: None,
            },
            AssistantEvent::MessageStop,
        ];

        // when
        let (message, _, _) = build_assistant_message(events)
            .expect("assistant message should preserve thinking, text, and tool blocks");

        // then
        assert_eq!(
            message.blocks,
            vec![
                ContentBlock::Thinking {
                    thinking: "pondering".to_string(),
                    signature: Some("sig".to_string()),
                },
                ContentBlock::Text {
                    text: "hello".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tool-1".to_string(),
                    name: "echo".to_string(),
                    input: "payload".to_string(),
                    signature: None,
                },
            ]
        );
    }

    #[test]
    fn static_tool_executor_rejects_unknown_tools() {
        // given
        let executor = StaticToolExecutor::new();

        // when
        let error = executor
            .execute("missing", "{}")
            .expect_err("unregistered tools should fail");

        // then
        assert_eq!(error.to_string(), "unknown tool: missing");
    }

    #[test]
    fn run_turn_errors_when_max_iterations_is_exceeded() {
        struct LoopingApi;

        impl ApiClient for LoopingApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: "tool-1".to_string(),
                        name: "echo".to_string(),
                        input: "payload".to_string(),
                        signature: None,
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        // given
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            LoopingApi,
            StaticToolExecutor::new().register("echo", |input| Ok(input.to_string())),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_max_iterations(1);

        // when
        let error = runtime
            .run_turn("loop", None)
            .expect_err("conversation loop should stop after the configured limit");

        // then
        assert!(error
            .to_string()
            .contains("conversation loop exceeded the maximum number of iterations"));
    }

    /// End-to-end proof that the ladder is not decoration: a stalled turn must
    /// (1) climb out of laminar, (2) have the forced orbit split injected into the
    /// system prompt the model actually receives, and (3) get the dead orbit
    /// refused at the middleware instead of executed a third time.
    #[test]
    fn repeated_identical_failure_escalates_and_refuses_the_dead_orbit() {
        struct RecordingApi {
            prompts: Arc<Mutex<Vec<String>>>,
            calls: std::cell::Cell<usize>,
        }

        impl ApiClient for RecordingApi {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.prompts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(request.system_prompt.join("\n"));
                let n = self.calls.get() + 1;
                self.calls.set(n);
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: format!("tool-{n}"),
                        name: "bash".to_string(),
                        input: "{\"command\":\"edge-tts --text hi\"}".to_string(),
                        signature: None,
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            RecordingApi {
                prompts: Arc::clone(&prompts),
                calls: std::cell::Cell::default(),
            },
            StaticToolExecutor::new().register("bash", |_input| {
                Err(ToolError::new("No such file or directory: edge-tts"))
            }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_max_iterations(4);

        // when
        let _ = runtime.run_turn("ship the tts", None);

        // then: the pressure measured off real tool results left laminar.
        assert!(
            runtime.bifurcation_regime() > Regime::Laminar,
            "four identical failing steps should walk the ladder, got:\n{}",
            runtime.bifurcation_summary()
        );

        let prompts = prompts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(prompts.len(), 4, "one API call per iteration");
        assert!(
            !prompts[0].contains("BIFURCATION"),
            "a converging first call must cost zero directive tokens"
        );
        assert!(
            prompts[2].contains("PERIOD-2 ORBIT SPLIT"),
            "the orbit-split directive must reach the system prompt, got:\n{}",
            prompts[2]
        );

        let tool_results = runtime
            .session()
            .messages
            .iter()
            .flat_map(|m| m.blocks.iter())
            .filter_map(|block| match block {
                ContentBlock::ToolResult { output, .. } => Some(output.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let refusals = tool_results
            .iter()
            .filter(|output| output.contains("DEAD ORBIT REFUSED"))
            .count();
        assert_eq!(
            refusals, 3,
            "every retry after the first failure must be refused, not executed"
        );
        assert_eq!(
            tool_results.len(),
            4,
            "exactly one real execution plus the refusals"
        );
        assert!(
            refusals < tool_results.len(),
            "the failing call must actually run once — the ladder measures, it does not guess"
        );
    }

    #[test]
    fn a_turn_that_makes_progress_stays_laminar() {
        struct ProgressingApi {
            calls: std::cell::Cell<usize>,
        }

        impl ApiClient for ProgressingApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                let n = self.calls.get() + 1;
                self.calls.set(n);
                if n > 3 {
                    return Ok(vec![
                        AssistantEvent::TextDelta("done".to_string()),
                        AssistantEvent::MessageStop,
                    ]);
                }
                Ok(vec![
                    AssistantEvent::ToolUse {
                        id: format!("tool-{n}"),
                        name: "edit_file".to_string(),
                        input: format!("{{\"path\":\"f{n}.rs\"}}"),
                        signature: None,
                    },
                    AssistantEvent::MessageStop,
                ])
            }
        }

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            ProgressingApi {
                calls: std::cell::Cell::default(),
            },
            StaticToolExecutor::new().register("edit_file", |input| Ok(format!("edited {input}"))),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_max_iterations(6);

        runtime
            .run_turn("refactor", None)
            .expect("turn should complete");

        assert_eq!(runtime.bifurcation_regime(), Regime::Laminar);
        assert!(
            runtime
                .bifurcation_summary()
                .contains("regime=laminar pressure=0.000"),
            "{}",
            runtime.bifurcation_summary()
        );
    }

    /// End-to-end proof of the Maxwell-demon runtime: the dead error is collapsed
    /// the moment the ladder crosses, a blind mutation is refused *before* the
    /// executor ever sees it, and the first read-only probe re-opens the gate.
    #[test]
    fn the_demon_prunes_the_blind_write_is_blocked_and_the_probe_unblocks_it() {
        const BLIND: &str = r#"{"path":"cfg.json","old_string":"a","new_string":"b"}"#;
        const FIXED: &str = r#"{"path":"cfg.json","old_string":"c","new_string":"d"}"#;
        const PROBE: &str = r#"{"path":"cfg.json"}"#;
        // Verbose enough to qualify as a dead-orbit exhibit, with the tail token
        // placed past the 160-char excerpt the tombstone keeps.
        let long_error = format!(
            "No such file or directory: /usr/local/lib/python3.11/site-packages/flake8/__main__.py \
             {}TRACE_FRAME_MUST_NOT_SURVIVE",
            "stack frame at 0x7ffd9f0 ".repeat(12)
        );

        struct ScriptedApi {
            prompts: Arc<Mutex<Vec<String>>>,
            calls: std::cell::Cell<usize>,
        }

        impl ApiClient for ScriptedApi {
            fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
                self.prompts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(request.system_prompt.join("\n"));
                let n = self.calls.get() + 1;
                self.calls.set(n);
                let tool = |id: &str, name: &str, input: &str| AssistantEvent::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: input.to_string(),
                    signature: None,
                };
                let stop = || AssistantEvent::MessageStop;
                match n {
                    // Two identical blind writes: the first runs and fails, the
                    // second is a closed orbit the middleware refuses.
                    1 | 2 => Ok(vec![tool(&format!("t{n}"), "edit_file", BLIND), stop()]),
                    // Same mutation, still no probe in the step → the gate blocks it.
                    3 => Ok(vec![tool("t3", "edit_file", FIXED), stop()]),
                    // Probe first, then the identical mutation: legal again.
                    4 => Ok(vec![
                        tool("t4a", "read_file", PROBE),
                        tool("t4b", "edit_file", FIXED),
                        stop(),
                    ]),
                    _ => Ok(vec![AssistantEvent::TextDelta("done".to_string()), stop()]),
                }
            }
        }

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let executed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&executed);
        let read_recorder = Arc::clone(&executed);
        let error = long_error.clone();

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            ScriptedApi {
                prompts: Arc::clone(&prompts),
                calls: std::cell::Cell::default(),
            },
            StaticToolExecutor::new()
                .register("edit_file", move |input| {
                    recorder
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!("edit_file:{input}"));
                    if input == BLIND {
                        Err(ToolError::new(error.clone()))
                    } else {
                        Ok(format!("edited {input}"))
                    }
                })
                .register("read_file", move |input| {
                    read_recorder
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!("read_file:{input}"));
                    Ok(PROBE.to_string())
                }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_max_iterations(5);

        runtime
            .run_turn("fix the config", None)
            .expect("turn should complete");

        let results = runtime
            .session()
            .messages
            .iter()
            .flat_map(|message| message.blocks.iter())
            .filter_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    output,
                    is_error,
                    ..
                } => Some((tool_use_id.clone(), output.clone(), *is_error)),
                _ => None,
            })
            .collect::<Vec<_>>();

        let tombstones = results
            .iter()
            .filter(|(_, output, _)| output.starts_with("[🗑️ Dead orbit collapsed by runtime"))
            .count();
        assert_eq!(
            tombstones,
            1,
            "the crossing collapses exactly one stale exhibit: {:?}",
            results.iter().map(|(_, o, _)| o).collect::<Vec<_>>()
        );
        let (id, tombstone, is_error) = results
            .iter()
            .find(|(_, output, _)| output.starts_with("[🗑️"))
            .expect("one tombstone");
        assert_eq!(id, "t1", "the pairing-critical id survives the rewrite");
        assert!(
            *is_error,
            "collapsing an exhibit must not disguise it as a success"
        );
        assert!(
            tombstone.contains("category=\"file_not_found\""),
            "{tombstone}"
        );
        assert!(tombstone.contains("attempts="), "{tombstone}");
        assert!(
            tombstone.contains("No such file or directory"),
            "the tombstone keeps the one line worth reading"
        );
        assert!(
            !tombstone.contains("TRACE_FRAME_MUST_NOT_SURVIVE"),
            "the tail beyond the excerpt is purged: {tombstone}"
        );

        assert!(
            results.iter().any(|(id, output, is_error)| {
                id == "t3"
                    && *is_error
                    && output.contains("[SYSTEM DIRECTIVE — BIFURCATION: PROBE GATE]")
            }),
            "the blind mutation is refused pre-execution, paired by its own id"
        );
        assert_eq!(
            executed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
            vec![
                format!("edit_file:{BLIND}"),
                format!("read_file:{PROBE}"),
                format!("edit_file:{FIXED}"),
            ],
            "the dead orbit and the blind write never reach the executor, and the probe runs first"
        );

        let prompts = prompts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(prompts.len(), 5, "one API call per iteration");
        assert!(
            !prompts[0].contains("BIFURCATION"),
            "a converging first call costs no directive tokens"
        );
        assert!(
            prompts[3].contains("PERIOD-2 ORBIT SPLIT"),
            "the escalated step is told which thinking mode the ladder forced"
        );

        let summary = runtime.bifurcation_summary();
        assert!(summary.contains("pruned_outputs=1"), "{summary}");
        assert!(summary.contains("trips=1"), "{summary}");
    }

    /// Emitting `read_file` is not the same as looking. Every probe in this
    /// trajectory errors, so none of them may discharge the contract, and the
    /// mutation that follows them is still refused — until a probe that actually
    /// returns something runs.
    #[test]
    fn probes_that_error_do_not_buy_a_blind_write() {
        const MISSING_ONE: &str = r#"{"path":"gone-1.json"}"#;
        const MISSING_TWO: &str = r#"{"path":"gone-2.json"}"#;
        const MISSING_THREE: &str = r#"{"path":"gone-3.json"}"#;
        const BLIND: &str = r#"{"path":"cfg.json","new_string":"blind"}"#;
        const FOUND: &str = r#"{"path":"cfg.json"}"#;

        struct ScriptedApi {
            calls: std::cell::Cell<usize>,
        }

        impl ApiClient for ScriptedApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                let n = self.calls.get() + 1;
                self.calls.set(n);
                let tool = |id: &str, name: &str, input: &str| AssistantEvent::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: input.to_string(),
                    signature: None,
                };
                let stop = || AssistantEvent::MessageStop;
                match n {
                    // Three reads that all fail: the second closes the orbit and
                    // crosses the first bifurcation, which arms the gate.
                    1 => Ok(vec![tool("t1", "read_file", MISSING_ONE), stop()]),
                    2 => Ok(vec![tool("t2", "read_file", MISSING_TWO), stop()]),
                    3 => Ok(vec![tool("t3", "read_file", MISSING_THREE), stop()]),
                    // The mutation after them is blind in the only way that
                    // matters: nothing was ever learned.
                    4 => Ok(vec![tool("t4", "edit_file", BLIND), stop()]),
                    // A probe that returns something, emitted ahead of it.
                    5 => Ok(vec![
                        tool("t5a", "read_file", FOUND),
                        tool("t5b", "edit_file", BLIND),
                        stop(),
                    ]),
                    _ => Ok(vec![AssistantEvent::TextDelta("done".to_string()), stop()]),
                }
            }
        }

        let executed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let read_recorder = Arc::clone(&executed);
        let edit_recorder = Arc::clone(&executed);

        let mut runtime = ConversationRuntime::new(
            Session::new(),
            ScriptedApi {
                calls: std::cell::Cell::default(),
            },
            StaticToolExecutor::new()
                .register("read_file", move |input| {
                    read_recorder
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!("read_file:{input}"));
                    if input == FOUND {
                        Ok("{ \"here\": true }".to_string())
                    } else {
                        Err(ToolError::new(
                            "No such file or directory: cfg/gone.json".to_string(),
                        ))
                    }
                })
                .register("edit_file", move |input| {
                    edit_recorder
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!("edit_file:{input}"));
                    Ok(format!("edited {input}"))
                }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        )
        .with_max_iterations(6);

        runtime
            .run_turn("fix the config", None)
            .expect("turn should complete");

        assert_eq!(
            executed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
            vec![
                format!("read_file:{MISSING_ONE}"),
                format!("read_file:{MISSING_TWO}"),
                format!("read_file:{MISSING_THREE}"),
                format!("read_file:{FOUND}"),
                format!("edit_file:{BLIND}"),
            ],
            "three failed probes must not let the write through, the successful one must"
        );

        let refused = runtime
            .session()
            .messages
            .iter()
            .flat_map(|message| message.blocks.iter())
            .filter_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    output,
                    is_error,
                    ..
                } => Some((tool_use_id.clone(), output.clone(), *is_error)),
                _ => None,
            })
            .find(|(id, _, _)| id == "t4")
            .expect("the blind write gets an answer, not silence");
        assert!(
            refused.2,
            "a gate refusal is an error the model must route around"
        );
        assert!(
            refused
                .1
                .contains("[SYSTEM DIRECTIVE — BIFURCATION: PROBE GATE]"),
            "{}",
            refused.1
        );
        assert!(
            refused.1.contains("has to return something"),
            "the refusal must teach that a failed probe does not clear it: {}",
            refused.1
        );
        assert!(
            runtime
                .bifurcation_summary()
                .contains("probe_gate=open trips=1"),
            "one trip, then the probe that returned discharges the contract: {}",
            runtime.bifurcation_summary()
        );
    }

    /// A write that leaves a broken artifact on disk is not convergence: the
    /// syntax guard's backpressure holds resistance steady instead of dividing
    /// it by α, and only the next genuinely clean write contracts the orbit.
    #[test]
    fn a_broken_write_backpressures_instead_of_looking_like_progress() {
        const SHELL_FAILURE: &str = r#"{"command":"echo tuned > /etc/hosts"}"#;
        const BROKEN: &str = r#"{"path":"cfg.json","new_string":"broken"}"#;
        const CLEAN: &str = r#"{"path":"cfg.json","new_string":"clean"}"#;

        struct ScriptedApi {
            calls: std::cell::Cell<usize>,
        }

        impl ApiClient for ScriptedApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                let n = self.calls.get() + 1;
                self.calls.set(n);
                let stop = || AssistantEvent::MessageStop;
                let text = || AssistantEvent::TextDelta("ok".to_string());
                let tool = |index: usize, name: &str, input: &str| AssistantEvent::ToolUse {
                    id: format!("t{index}"),
                    name: name.to_string(),
                    input: input.to_string(),
                    signature: None,
                };
                match n {
                    // A failing shell step buys one quarter of a bifurcation; the
                    // writes below are then measured against that resistance.
                    1 => Ok(vec![tool(1, "bash", SHELL_FAILURE), stop()]),
                    3 => Ok(vec![tool(3, "edit_file", BROKEN), stop()]),
                    5 => Ok(vec![tool(5, "edit_file", CLEAN), stop()]),
                    _ => Ok(vec![text(), stop()]),
                }
            }
        }

        let executed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&executed);
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            ScriptedApi {
                calls: std::cell::Cell::default(),
            },
            StaticToolExecutor::new()
                .register("bash", |_input| {
                    Err(ToolError::new("permission denied: /etc/hosts".to_string()))
                })
                .register("edit_file", move |input| {
                    recorder
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(input.to_string());
                    if input.contains("broken") {
                        // What the real file_ops runner returns: the write succeeded,
                        // and the local guard appends the defect in-band.
                        Ok(format!(
                            "{}: line 3: unbalanced delimiters: unexpected closing '}}' \
                             (expected '{{' to open)",
                            crate::syntax_guard::SYNTAX_BACKPRESSURE_MARKER
                        ))
                    } else {
                        Ok(r#"{"path":"cfg.json","written":true}"#.to_string())
                    }
                }),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        // One novel failure buys exactly one quarter of a bifurcation.
        runtime
            .run_turn("step one", None)
            .expect("first turn should complete");
        let after_a = runtime.bifurcation_summary();
        assert!(
            after_a.contains("regime=laminar pressure=0.250"),
            "{after_a}"
        );

        runtime
            .run_turn("step two", None)
            .expect("second turn should complete");
        let after_broken = runtime.bifurcation_summary();
        assert!(
            after_broken.contains("pressure=0.250")
                && after_broken.contains("syntax_backpressure_hits=1"),
            "a broken artifact must not discount resistance: {after_broken}"
        );
        let broken_result = runtime
            .session()
            .messages
            .iter()
            .flat_map(|message| message.blocks.iter())
            .find_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    output,
                    is_error,
                    ..
                } if tool_use_id == "t3" => Some((output.clone(), *is_error)),
                _ => None,
            })
            .expect("the broken write ran");
        assert!(
            !broken_result.1,
            "backpressure is feedback, not a failure the model must re-litigate"
        );
        assert!(
            broken_result
                .0
                .contains(crate::syntax_guard::SYNTAX_BACKPRESSURE_MARKER),
            "the guard reports the defect in the tool result itself: {}",
            broken_result.0
        );

        runtime
            .run_turn("step three", None)
            .expect("third turn should complete");
        let after_clean = runtime.bifurcation_summary();
        assert!(
            after_clean.contains("pressure=0.100") && after_clean.contains("regime=laminar"),
            "only a clean forward step contracts the orbit by α: {after_clean}"
        );
        assert_eq!(
            executed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
            vec![BROKEN, CLEAN]
        );
    }

    #[test]
    fn run_turn_propagates_api_errors() {
        struct FailingApi;

        impl ApiClient for FailingApi {
            fn stream(
                &mut self,
                _request: ApiRequest,
            ) -> Result<Vec<AssistantEvent>, RuntimeError> {
                Err(RuntimeError::new("upstream failed"))
            }
        }

        // given
        let mut runtime = ConversationRuntime::new(
            Session::new(),
            FailingApi,
            StaticToolExecutor::new(),
            PermissionPolicy::new(PermissionMode::DangerFullAccess),
            vec!["system".to_string()],
        );

        // when
        let error = runtime
            .run_turn("hello", None)
            .expect_err("API failures should propagate");

        // then
        assert_eq!(error.to_string(), "upstream failed");
    }

    #[test]
    fn test_recover_textual_tool_calls_line_based() {
        use super::recover_textual_tool_calls;
        use crate::session::state::{ContentBlock, ConversationMessage};

        let mut msg = ConversationMessage::assistant(vec![ContentBlock::Text {
            text:
                "read_file\n{\"description\": \"Read build logs\", \"path\": \"/path/to/log.txt\"}"
                    .to_string(),
        }]);

        recover_textual_tool_calls(&mut msg);

        assert_eq!(msg.blocks.len(), 2);
        if let ContentBlock::ToolUse { name, input, .. } = &msg.blocks[1] {
            assert_eq!(name, "read_file");
            assert!(input.contains("/path/to/log.txt"));
        } else {
            panic!("Expected recovered ToolUse block");
        }
    }

    #[test]
    fn test_recover_textual_tool_calls_markdown_json() {
        use super::recover_textual_tool_calls;
        use crate::session::state::{ContentBlock, ConversationMessage};

        let mut msg = ConversationMessage::assistant(vec![
            ContentBlock::Text {
                text: "```json\n{\n  \"name\": \"bash\",\n  \"arguments\": {\"command\": \"xcodebuild -version\"}\n}\n```".to_string(),
            }
        ]);

        recover_textual_tool_calls(&mut msg);

        assert_eq!(msg.blocks.len(), 2);
        if let ContentBlock::ToolUse { name, input, .. } = &msg.blocks[1] {
            assert_eq!(name, "bash");
            assert!(input.contains("xcodebuild -version"));
        } else {
            panic!("Expected recovered ToolUse block");
        }
    }
}
