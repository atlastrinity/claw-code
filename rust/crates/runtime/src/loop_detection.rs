use std::sync::Mutex;

use crate::fractal::BifurcationEngine;
use crate::middleware::{ToolCallContext, ToolCallOutcome, TurnMiddleware};
use crate::security::permissions::PermissionPrompter;
use crate::session::error_tracker::resolve_effective_tool_name;
use crate::session::state::{ContentBlock, ConversationMessage, MessageRole, Session};

/// Refuses retries that cannot succeed, before they cost a tool execution.
///
/// Two independent rules run here:
///
/// 1. the ladder's semantic gate — a call that already failed with this exact
///    input is refused while the mutation epoch has not moved, i.e. nothing that
///    could change the outcome has succeeded in between. Denied retries are fed
///    back into the ladder via [`BifurcationEngine::note_refusal`], so a blocked
///    loop still escalates the thinking mode;
/// 2. the older byte-equality counter, kept because it also catches repeats of
///    calls that *succeeded* (re-reading the same file forever), which the
///    failure ledger deliberately ignores.
pub struct LoopDetectionMiddleware<'a> {
    pub session: &'a Session,
    pub max_identical_calls: usize,
    pub bifurcation: &'a Mutex<BifurcationEngine>,
}

impl<'a> LoopDetectionMiddleware<'a> {
    #[must_use]
    pub fn new(session: &'a Session, bifurcation: &'a Mutex<BifurcationEngine>) -> Self {
        Self {
            session,
            max_identical_calls: 2, // Allow 2 identical calls, deny on the 3rd
            bifurcation,
        }
    }
}

impl<'a, 'p> TurnMiddleware<'a, 'p> for LoopDetectionMiddleware<'a> {
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

        for call_state in ctx.calls {
            let tool_name = &call_state.request.tool_name;
            let input = &call_state.request.input;

            // 1. Semantic gate: a dead orbit, not merely a repeated string.
            let effective_name = resolve_effective_tool_name(tool_name, input);
            let key = BifurcationEngine::attempt_key(&effective_name, input);
            let refusal = {
                let mut engine = self
                    .bifurcation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match engine.would_refuse(&key) {
                    // The blocked retry is trajectory evidence: the ladder must
                    // see the orbit close even though nothing executed.
                    Some(refusal) => {
                        engine.note_refusal(&refusal);
                        Some(refusal)
                    }
                    None => None,
                }
            };
            if let Some(refusal) = refusal {
                tracing::warn!(
                    tool = %refusal.tool,
                    category = %refusal.category,
                    attempts = refusal.attempts,
                    epoch = refusal.epoch,
                    "dead orbit refused"
                );
                denied_messages.push(ConversationMessage::tool_result(
                    call_state.request.tool_use_id.clone(),
                    tool_name.clone(),
                    refusal.to_directive(),
                    true,
                ));
                continue;
            }

            let mut identical_count = 0;

            // Scan backwards through session messages until the last User message
            for message in self.session.messages.iter().rev() {
                if matches!(message.role, MessageRole::User) {
                    break;
                }

                if matches!(message.role, MessageRole::Assistant) {
                    for block in &message.blocks {
                        if let ContentBlock::ToolUse {
                            name,
                            input: past_input,
                            ..
                        } = block
                        {
                            if name == tool_name && past_input == input {
                                identical_count += 1;
                            }
                            // Reset counter if a successful TaskGraph call happened between attempts
                            // This means the agent changed the graph state, so retrying is legitimate
                            if name == "TaskGraph" {
                                identical_count = 0;
                            }
                        }
                    }
                }
            }

            if identical_count >= self.max_identical_calls {
                let reason = "[SYSTEM DIRECTIVE - LOOP DETECTED]\nYou have executed this exact same tool with the exact same input recently in this turn.\nYou are caught in a repetitive loop. Analyze your actions, stop repeating the same cyclical events, try a completely different approach.\n\
                \nTaskGraph RECOVERY DIRECTIVE:\n\
                1. If this task keeps failing, DO NOT delete it.\n\
                2. Mark the current active task as 'failed' using TaskGraph operation: 'update_status'.\n\
                3. Create a new recovery sub-task or an alternative approach using TaskGraph operation: 'add'.\n\
                4. Set the new recovery task to 'in_progress' and proceed.".to_string();

                denied_messages.push(ConversationMessage::tool_result(
                    call_state.request.tool_use_id.clone(),
                    tool_name.clone(),
                    reason,
                    true, // Mark as an error to trigger failure handling
                ));
            } else {
                allowed_calls.push(call_state);
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

        // Add the denied messages
        outcome.messages.extend(denied_messages);
        outcome
    }
}
