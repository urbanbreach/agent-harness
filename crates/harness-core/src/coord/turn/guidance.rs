//! Coordinator guidance inside one worker turn.
//!
//! The worker calls these hooks at three points: when the model stops without tool calls,
//! after each tool batch, and when the stream guard stops a response. Every reminder is
//! recorded through the coordinator before it enters context.
use super::*;
use crate::coord::context::Context;
use crate::coord::reminders::{note_kind, TurnInput};
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use std::collections::VecDeque;

mod loops;
mod output_contract;
mod todo;

use loops::Call;

#[derive(Default)]
pub(in crate::coord) struct Guidance {
    todo_reminders: u32,
    acted_since_todo_reminder: bool,
    calls: VecDeque<Call>,
    loop_reminded: Option<Vec<Call>>,
    stream_retries: u32,
    pub(super) output_attempts: u32,
    /// Consecutive `length` stops that delivered nothing.
    pub(in crate::coord) length_recoveries: u32,
    /// Bumped when input reaches the turn, so a scheduled synthetic continuation can tell
    /// that the user or another agent changed what the agent is doing.
    pub(in crate::coord) input_generation: u64,
    /// The model the footer status was last published for.
    pub(in crate::coord) status_model: String,
    /// The operator's model, settings and target while a context promotion has this turn
    /// on a larger-context model. The promotion never outlives the turn.
    pub(in crate::coord) operator_model:
        Option<(String, AgentModelSettings, Option<ResolvedModelTarget>)>,
}

impl Guidance {
    /// New input from the user or another agent changes what the agent is doing.
    pub(super) fn input_arrived(&mut self) {
        self.calls.clear();
        self.loop_reminded = None;
        self.length_recoveries = 0;
        self.input_generation = self.input_generation.wrapping_add(1);
    }
}

/// Places a user-role note in context with its provenance.
pub(in crate::coord) fn push_input(
    messages: &mut Context,
    turn: &str,
    input: TurnInput,
    at_ms: Option<u64>,
) {
    messages.push_timed(
        CompletionMessage::text(MessageRole::User, input.text),
        input.seq,
        Some(turn),
        at_ms,
    );
    if let Some(entry) = messages.entries.last_mut() {
        entry.origin = input.origin;
        entry.note_kind = input.note;
    }
}

impl Worker {
    /// Records a reminder for this turn and places it in context.
    pub(in crate::coord) async fn remind(
        &self,
        kind: RuntimeReminderKind,
        body: String,
        source: Option<String>,
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        let (seq, text) = self
            .handle
            .emit_turn_reminder(self.actor.clone(), self.turn.id.clone(), kind, body, source)
            .await?;
        let at_ms = self.event_ms().await?;
        let input = TurnInput {
            seq,
            text,
            origin: harness_compaction::Origin::Host,
            note: Some(note_kind(kind)),
        };
        push_input(messages, &self.turn.id, input, at_ms);
        Ok(())
    }

    /// Places waiting subagent messages, steering, and reminders in context before the
    /// next request. Returns true when anything arrived.
    pub(super) async fn take_turn_inputs(
        &mut self,
        messages: &mut Context,
        steering: bool,
    ) -> Result<bool, CoordinatorError> {
        // The notes window reminder is an aside: it is not input and never changes the result.
        self.drain_notes_reminder(messages).await?;
        let inputs = self
            .handle
            .drain_turn_inputs(self.actor.clone(), self.turn.id.clone(), steering)
            .await?;
        if inputs.is_empty() {
            return Ok(false);
        }
        self.guidance.input_arrived();
        let at_ms = self.event_ms().await?;
        for input in inputs {
            push_input(messages, &self.turn.id, input, at_ms);
        }
        Ok(true)
    }

    /// Runs after a tool batch whose results are already in context. Directory
    /// instructions are queued by the coordinator at tool completion instead, so calls
    /// made from inside eval and by subagents are covered too.
    pub(super) async fn after_tools(
        &mut self,
        calls: &[AssistantToolCall],
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        self.guidance.acted_since_todo_reminder = true;
        self.loop_guard(calls, messages).await
    }

    /// The stream guard stopped a response: record a correction so the worker retries,
    /// or fail the turn with the guard error once retries are exhausted.
    pub(super) async fn retry_after_stream_guard(
        &mut self,
        failure: super::super::StreamGuardFailure,
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        let settings = &self.behavior.stream_guard;
        if !settings.enabled || self.guidance.stream_retries >= settings.max_retries {
            return Err(CoordinatorError::StreamGuard(failure));
        }
        self.guidance.stream_retries += 1;
        let reason = &failure.reason;
        let body = format!(
            "Your previous response was stopped while it streamed because {reason}. Do not repeat that text. Continue from where the work stands: make the next tool call if work remains, or give a short final answer."
        );
        self.remind(RuntimeReminderKind::StreamGuard, body, None, messages)
            .await?;
        // The retry is a new request, so input that arrived meanwhile joins it.
        self.take_turn_inputs(messages, true).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_input_resets_length_recovery_and_advances_the_generation() {
        let mut guidance = Guidance {
            length_recoveries: 2,
            ..Default::default()
        };
        guidance.input_arrived();
        assert_eq!(guidance.length_recoveries, 0);
        assert_eq!(guidance.input_generation, 1);
    }
}
