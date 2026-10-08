//! Coordinator guidance inside one worker turn.
//!
//! The worker calls these hooks at three points: when the model stops without tool calls,
//! after each tool batch, and when the stream guard stops a response. Every reminder is
//! recorded through the coordinator before it enters context.
use super::*;
use crate::coord::context::Context;
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use std::collections::VecDeque;

mod output_contract;

/// Repetition units are at most three calls long, and a turn stops at twice the threshold.
const MAX_LOOP_PERIOD: usize = 3;

#[derive(Default)]
pub(in crate::coord) struct Guidance {
    todo_reminders: u32,
    acted_since_todo_reminder: bool,
    calls: VecDeque<Call>,
    loop_reminded: Option<Vec<Call>>,
    stream_retries: u32,
    pub(super) output_attempts: u32,
}

impl Guidance {
    /// New input from the user or another agent changes what the agent is doing.
    pub(super) fn input_arrived(&mut self) {
        self.calls.clear();
        self.loop_reminded = None;
    }
}

impl Worker {
    /// Records a reminder for this turn and places it in context.
    pub(super) async fn remind(
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
        messages.push(
            CompletionMessage::text(MessageRole::User, text),
            seq,
            Some(&self.turn.id),
        );
        Ok(())
    }

    /// Places waiting subagent messages, steering, and reminders in context before the
    /// next request. Returns true when anything arrived.
    pub(super) async fn take_turn_inputs(
        &mut self,
        messages: &mut Context,
        steering: bool,
    ) -> Result<bool, CoordinatorError> {
        let inputs = self
            .handle
            .drain_turn_inputs(self.actor.clone(), self.turn.id.clone(), steering)
            .await?;
        if inputs.is_empty() {
            return Ok(false);
        }
        self.guidance.input_arrived();
        for (seq, text) in inputs {
            messages.push(
                CompletionMessage::text(MessageRole::User, text),
                seq,
                Some(&self.turn.id),
            );
        }
        Ok(true)
    }

    /// The model answered without tool calls. Returns true when the turn should continue.
    pub(super) async fn continue_after_stop(
        &mut self,
        text: &str,
        messages: &mut Context,
    ) -> Result<bool, CoordinatorError> {
        // Inputs arriving during the final provider request belong to this turn,
        // including one-shot CLI turns which stop the runtime after completion.
        if self.take_turn_inputs(messages, true).await? {
            return Ok(true);
        }
        if let Some(body) = self.output_contract_reminder(text).await? {
            self.remind(RuntimeReminderKind::OutputContract, body, None, messages)
                .await?;
            return Ok(true);
        }
        let settings = &self.behavior.todo_continuation;
        if !settings.enabled
            || self.guidance.todo_reminders >= settings.max_reminders
            || (self.guidance.todo_reminders > 0 && !self.guidance.acted_since_todo_reminder)
        {
            return Ok(false);
        }
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let open: Vec<_> = self
            .handle
            .root_todo_items(agent)
            .await?
            .into_iter()
            .filter(crate::proj::TodoItem::is_open)
            .collect();
        if open.is_empty() {
            return Ok(false);
        }
        self.guidance.todo_reminders += 1;
        self.guidance.acted_since_todo_reminder = false;
        let items = open
            .iter()
            .map(|item| format!("- [{}] {}", item.status, item.content))
            .collect::<Vec<_>>()
            .join("\n");
        let body = format!(
            "Your todo list still has open items:\n{items}\n\nContinue with the next open item now. When an item is done, mark it completed in the todo list; if it is blocked or no longer needed, mark it cancelled and say why in your final message. End the turn only when every item is completed or cancelled, or when you need input that only the user can give."
        );
        self.remind(RuntimeReminderKind::TodoContinuation, body, None, messages)
            .await?;
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

    async fn loop_guard(
        &mut self,
        calls: &[AssistantToolCall],
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        let settings = &self.behavior.loop_guard;
        if !settings.enabled || settings.threshold < 2 {
            return Ok(());
        }
        // Room for the stop count of the longest cycle; config validation bounds the threshold.
        let window = MAX_LOOP_PERIOD * 2 * settings.threshold as usize;
        for call in calls {
            if settings
                .exempt_tools
                .iter()
                .any(|tool| tool == &call.function_name)
            {
                // Waits and questions can change what the next identical call returns.
                self.guidance.calls.clear();
                continue;
            }
            if self.guidance.calls.len() >= window {
                self.guidance.calls.pop_front();
            }
            self.guidance.calls.push_back(Call::of(call));
        }
        let Some(repeat) = repetition(&self.guidance.calls) else {
            return Ok(());
        };
        let threshold = settings.threshold as usize;
        if repeat.count >= threshold * 2 {
            return Err(CoordinatorError::Invalid(format!(
                "loop guard stopped the turn: {} repeated {} times after a reminder",
                repeat.describe(),
                repeat.count
            )));
        }
        if repeat.count < threshold || self.guidance.loop_reminded.as_ref() == Some(&repeat.unit) {
            return Ok(());
        }
        self.guidance.loop_reminded = Some(repeat.unit.clone());
        let remaining = threshold * 2 - repeat.count;
        let body = format!(
            "You have made {} {} times in a row. Repeating it will not produce a different result. Use what the earlier results already show, try a materially different approach, or stop and report what is blocking you. If it repeats {remaining} more times, Harness will stop this turn.",
            repeat.describe(),
            repeat.count
        );
        self.remind(RuntimeReminderKind::LoopGuard, body, None, messages)
            .await
    }
}

impl CoordinatorHandle {
    /// Todo items for a root agent; subagents never inherit the session list.
    async fn root_todo_items(
        &self,
        agent: String,
    ) -> Result<Vec<crate::proj::TodoItem>, CoordinatorError> {
        self.call(move |s| Ok(s.root_todo_items(&agent))).await
    }
}

impl crate::coord::runtime::Runtime {
    pub(in crate::coord) fn root_todo_items(&self, agent: &str) -> Vec<crate::proj::TodoItem> {
        if self
            .agents
            .get(agent)
            .is_some_and(|agent| agent.info.parent_agent_id.is_none())
        {
            self.todos.items()
        } else {
            Vec::new()
        }
    }
}

/// Tool name plus a digest of its canonical arguments, so long arguments are not retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct Call {
    name: String,
    arguments: u64,
}

impl Call {
    fn of(call: &AssistantToolCall) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        match serde_json::from_str::<Value>(&call.arguments_json) {
            Ok(value) => canonical_json(value).hash(&mut hasher),
            Err(_) => call.arguments_json.hash(&mut hasher),
        }
        Self {
            name: call.function_name.clone(),
            arguments: hasher.finish(),
        }
    }
}

fn canonical_json(mut value: Value) -> String {
    value.sort_all_objects();
    value.to_string()
}

struct Repetition {
    /// Calls in the repeated unit, oldest first.
    unit: Vec<Call>,
    count: usize,
}

impl Repetition {
    fn describe(&self) -> String {
        match self.unit.as_slice() {
            [call] => format!("the same `{}` call with identical arguments", call.name),
            _ => format!(
                "the same sequence of {} calls ({})",
                self.unit.len(),
                self.unit
                    .iter()
                    .map(|call| format!("`{}`", call.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The longest trailing run of an identical call, or of a repeated cycle of two or three
/// distinct calls, counted in repetitions of the unit.
fn repetition(calls: &VecDeque<Call>) -> Option<Repetition> {
    let calls: Vec<&Call> = calls.iter().collect();
    let mut best: Option<Repetition> = None;
    for period in 1..=MAX_LOOP_PERIOD {
        if calls.len() < period * 2 {
            continue;
        }
        let unit = &calls[calls.len() - period..];
        if period > 1 && unit.iter().all(|call| *call == unit[0]) {
            continue;
        }
        let mut count = 1;
        while calls.len() >= period * (count + 1)
            && calls[calls.len() - period * (count + 1)..calls.len() - period * count] == *unit
        {
            count += 1;
        }
        if count >= 2 && best.as_ref().is_none_or(|b| count > b.count) {
            best = Some(Repetition {
                unit: unit.iter().map(|call| (*call).clone()).collect(),
                count,
            });
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(names: &[&str]) -> VecDeque<Call> {
        names
            .iter()
            .map(|name| Call {
                name: (*name).into(),
                arguments: 0,
            })
            .collect()
    }

    #[test]
    fn repetition_counts_identical_calls_and_cycles_but_not_progress() {
        for (sequence, expected) in [
            (&["read", "read", "read"][..], Some((1, 3))),
            (&["grep", "read", "read", "read", "read"][..], Some((1, 4))),
            (
                &["read", "edit", "read", "edit", "read", "edit"][..],
                Some((2, 3)),
            ),
            (&["a", "b", "c", "a", "b", "c"][..], Some((3, 2))),
            (&["read", "edit", "bash", "read"][..], None),
            (&["read"][..], None),
        ] {
            let found = repetition(&calls(sequence)).map(|r| (r.unit.len(), r.count));
            assert_eq!(found, expected, "{sequence:?}");
        }
    }
}
