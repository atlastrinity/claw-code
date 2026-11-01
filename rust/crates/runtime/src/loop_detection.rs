use std::sync::Mutex;

use crate::fractal::{BifurcationEngine, Refusal};
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

    /// Tool-use ids whose call never reached the executor because the runtime
    /// itself refused it (dead orbit, probe gate, older loop directive).
    ///
    /// These emissions must not feed the byte-equality counter: a mutation
    /// blocked *before* running leaves the world untouched, and counting it as
    /// an execution would punish the agent twice — once by the gate, then again
    /// when it retries the same call after a legitimate read-only probe.
    #[must_use]
    fn refused_ids(&self) -> std::collections::HashSet<String> {
        let start = self
            .session
            .messages
            .iter()
            .rposition(|message| matches!(message.role, MessageRole::User))
            .map_or(0, |index| index + 1);
        self.session
            .messages
            .iter()
            .skip(start)
            .flat_map(|message| message.blocks.iter())
            .filter_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    output,
                    ..
                } if output.starts_with("[SYSTEM DIRECTIVE") => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// The calls of one assistant turn, as a sorted multiset of
    /// `tool|operative-input`, for every turn of the current human request.
    ///
    /// The caption is stripped because a reworded `description` is the same
    /// action; the set is sorted because a small model that re-orders an
    /// equivalent batch is not making a new attempt either.
    fn turn_shapes(&self) -> Vec<Vec<String>> {
        let start = self
            .session
            .messages
            .iter()
            .rposition(|message| matches!(message.role, MessageRole::User))
            .map_or(0, |index| index + 1);
        self.session.messages[start..]
            .iter()
            .filter(|message| matches!(message.role, MessageRole::Assistant))
            .map(|message| {
                let mut calls: Vec<String> = message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolUse { name, input, .. } => {
                            Some(format!("{name}|{}", BifurcationEngine::operative_input(input)))
                        }
                        _ => None,
                    })
                    .collect();
                calls.sort();
                calls
            })
            .filter(|calls| !calls.is_empty())
            .collect()
    }

    /// How many turns the current batch has already lapped, when the tail of the
    /// turn history is one closed cycle.
    ///
    /// Repetition collapse does not look like a repeated call: the model re-emits
    /// the whole block — plan, execute, plan, execute — so the turns alternate
    /// between two or three shapes and every per-call counter sees a fresh
    /// attempt. The turn is the unit that actually repeats.
    fn lap_cycle(shapes: &[Vec<String>]) -> Option<(usize, usize)> {
        let turns = shapes.len();
        for period in 1..=3usize {
            if turns < period * 3 {
                continue;
            }
            // A lap of one single call belongs to the per-call gates: the failure
            // ledger refuses it by name and the byte counter catches it with its
            // own directive. This gate is for the block that repeats as a block.
            if shapes[turns - period..].iter().map(|lap| lap.len()).sum::<usize>() < 2 {
                continue;
            }
            let last = &shapes[turns - period..];
            let mut laps = 1;
            while (laps + 1) * period <= turns {
                let start = turns - (laps + 1) * period;
                if shapes[start..start + period] != *last {
                    break;
                }
                laps += 1;
            }
            if laps >= 3 {
                return Some((period, laps));
            }
        }
        None
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
        // 0. Whole-turn gate: the same block of calls, lapped three times.
        let shapes = self.turn_shapes();
        if let Some((period, laps)) = Self::lap_cycle(&shapes) {
            let mut denied_messages: Vec<ConversationMessage> = Vec::new();
            {
                let mut engine = self
                    .bifurcation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for call in &ctx.calls {
                    let tool =
                        resolve_effective_tool_name(&call.request.tool_name, &call.request.input);
                    engine.note_refusal(&Refusal {
                        tool: tool.to_string(),
                        category: "lap_cycle".to_string(),
                        attempts: laps,
                        epoch: 0,
                    });
                    denied_messages.push(ConversationMessage::tool_result(
                        call.request.tool_use_id.clone(),
                        call.request.tool_name.clone(),
                        format!(
                            "[SYSTEM DIRECTIVE — RUN LOOP STOPPED]\n\
                             {laps} turns in a row have submitted the same block of tool calls \
                             (a {period}-turn cycle, unchanged). Nothing in this batch ran, \
                             because it can only produce the results the previous laps already got.\n\
                             Pick exactly one: \
                             (1) act on a different object and say in the call what you changed — \
                             a new path, a new command, or a read-only probe first; \
                             (2) mark the blocked task 'failed' with the reason and move to the \
                             next sibling; \
                             (3) stop, and tell the user plainly what is blocked and what you \
                             tried. Re-sending this batch is the fourth lap."
                        ),
                        true,
                    ));
                }
            }
            tracing::warn!(
                period,
                laps,
                calls = denied_messages.len(),
                "run loop stopped: identical turn cycle"
            );
            // Nothing runs, so the executor is never entered — the denied
            // results are the whole outcome of this step.
            return ToolCallOutcome {
                messages: denied_messages,
            };
        }

        let mut allowed_calls = Vec::new();
        let mut denied_messages = Vec::new();
        let refused_ids = self.refused_ids();

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
            let operative = BifurcationEngine::operative_input(input);

            // Scan backwards through session messages until the last User message
            for message in self.session.messages.iter().rev() {
                if matches!(message.role, MessageRole::User) {
                    break;
                }

                if matches!(message.role, MessageRole::Assistant) {
                    for block in &message.blocks {
                        if let ContentBlock::ToolUse {
                            id,
                            name,
                            input: past_input,
                            ..
                        } = block
                        {
                            if refused_ids.contains(id) {
                                continue;
                            }
                            // Compare the action, not the caption: the same command
                            // with a new `description` is one more lap of the orbit.
                            if name == tool_name
                                && BifurcationEngine::operative_input(past_input) == operative
                            {
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
                let reason = "[SYSTEM DIRECTIVE - LOOP DETECTED]\nYou have executed this same action (same arguments — a reworded `description` does not make it a new attempt) recently in this turn.\nYou are caught in a repetitive loop. Analyze your actions, stop repeating the same cyclical events, try a completely different approach.\n\
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(calls: &[&str]) -> Vec<String> {
        let mut calls: Vec<String> = calls.iter().map(|call| (*call).to_string()).collect();
        calls.sort();
        calls
    }

    fn cycle(period: usize, laps: usize) -> Vec<Vec<String>> {
        let block: Vec<Vec<String>> = (0..period)
            .map(|turn| shape(&[&format!("bash|command turn {turn}")]))
            .collect();
        let mut shapes = Vec::new();
        for _ in 0..laps {
            shapes.extend(block.iter().cloned());
        }
        shapes
    }

    #[test]
    fn a_lapped_block_of_calls_is_a_run_loop() {
        let shapes = vec![
            shape(&["TaskGraph|add", "bash|swift build"]),
            shape(&["TaskGraph|add", "bash|swift build"]),
            shape(&["TaskGraph|add", "bash|swift build"]),
        ];
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&shapes), Some((1, 3)));
    }

    #[test]
    fn one_repeated_single_call_is_left_to_the_per_call_gates() {
        // The failure ledger and the byte counter already refuse this by name,
        // with the error they measured; a second gate would only blur the signal.
        let shapes = cycle(1, 4);
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&shapes), None);
    }

    #[test]
    fn an_alternating_plan_execute_plan_is_a_run_loop_too() {
        // The live shape: the model never repeats one call, it repeats the pair.
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&cycle(2, 3)), Some((2, 3)));
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&cycle(3, 3)), Some((3, 3)));
    }

    #[test]
    fn two_laps_is_not_yet_a_loop() {
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&cycle(1, 2)), None);
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&cycle(2, 2)), None);
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&cycle(2, 1)), None);
    }

    #[test]
    fn work_that_changes_is_not_a_loop() {
        let shapes: Vec<Vec<String>> = (0..6)
            .map(|turn| shape(&[&format!("bash|swift build step {turn}")]))
            .collect();
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&shapes), None);

        // A cycle broken by one different turn resets: the model changed approach.
        let mut shapes = cycle(2, 3);
        shapes.push(shape(&["bash|swift build"]));
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&shapes), None);
    }

    #[test]
    fn a_fourth_lap_is_still_reported_as_the_cycle_it_is() {
        assert_eq!(LoopDetectionMiddleware::lap_cycle(&cycle(2, 4)), Some((2, 4)));
    }
}
