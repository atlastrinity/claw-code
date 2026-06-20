use crate::middleware::{ToolCallContext, ToolCallOutcome, TurnMiddleware};
use crate::permissions::PermissionPrompter;
use crate::session::{ContentBlock, ConversationMessage, MessageRole, Session};

pub struct LoopDetectionMiddleware<'a> {
    pub session: &'a Session,
    pub max_identical_calls: usize,
}

impl<'a> LoopDetectionMiddleware<'a> {
    #[must_use]
    pub fn new(session: &'a Session) -> Self {
        Self {
            session,
            max_identical_calls: 2, // Allow 2 identical calls, deny on the 3rd
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
                        }
                    }
                }
            }

            if identical_count >= self.max_identical_calls {
                let reason = "[SYSTEM DIRECTIVE - LOOP DETECTED]\nYou have executed this exact same tool with the exact same input recently in this turn.\nYou are caught in a repetitive loop. Analyze your actions, stop repeating the same cyclical events, try a completely different approach.".to_string();

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
