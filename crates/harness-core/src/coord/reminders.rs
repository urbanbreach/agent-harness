//! Model-only guidance the coordinator adds to a running turn.
//!
//! Every reminder is appended as a durable `RuntimeReminder` before the worker places it in
//! context, so resume, replay, and the behavior census see exactly what the model saw.
use super::runtime::JobKind;
use super::*;
use std::path::PathBuf;

/// Guidance queued for an agent's running turn, emitted when the worker next drains inputs.
#[derive(Debug, Clone)]
pub(super) struct PendingReminder {
    pub kind: RuntimeReminderKind,
    pub body: String,
    pub source: Option<String>,
}

/// A user message sent into an agent's running turn.
#[derive(Debug, Clone)]
pub(super) struct Steering {
    pub id: String,
    pub text: String,
    /// Journal position of the acceptance, which orders it against queued prompts.
    pub seq: u64,
}

/// Wraps reminder text in the tag models treat as runtime guidance, escaping nested tags
/// so quoted tool output cannot close the wrapper early.
pub(super) fn wrap(text: &str) -> String {
    let text =
        ["system-reminder", "system_reminder"]
            .into_iter()
            .fold(text.to_owned(), |text, tag| {
                text.replace(&format!("</{tag}"), &format!("<\\/{tag}"))
                    .replace(&format!("<{tag}"), &format!("<\\{tag}"))
            });
    format!("<system-reminder>\n{text}\n</system-reminder>")
}

impl Runtime {
    /// Records model-only guidance for a turn and returns its sequence and exact text.
    pub(super) fn emit_reminder(
        &mut self,
        agent: &str,
        request: &str,
        kind: RuntimeReminderKind,
        body: &str,
        source: Option<String>,
    ) -> Result<(u64, String), CoordinatorError> {
        let event = self.emit(
            EventActor::new(ActorKind::Worker, Some(agent.into())),
            Some(request.into()),
            EventV1::RuntimeReminder(RuntimeReminderEvent {
                request_id: request.to_owned().into(),
                kind,
                text: wrap(body),
                source,
            }),
        )?;
        let EventV1::RuntimeReminder(recorded) = &event.payload else {
            return Err(CoordinatorError::Invalid(
                "runtime reminder was recorded as another event".into(),
            ));
        };
        Ok((event.seq, recorded.text.clone()))
    }

    /// Queues guidance for the agent's next drain; returns false when the agent is idle.
    pub(super) fn queue_reminder(&mut self, agent: &str, reminder: PendingReminder) -> bool {
        if !self.agents.get(agent).is_some_and(|a| a.busy) {
            return false;
        }
        self.pending_reminders
            .entry(agent.to_owned())
            .or_default()
            .push(reminder);
        true
    }

    pub(super) fn drain_pending_reminders(
        &mut self,
        agent: &str,
        request: &str,
    ) -> Result<Vec<(u64, String)>, CoordinatorError> {
        let Some(pending) = self.pending_reminders.remove(agent) else {
            return Ok(Vec::new());
        };
        let mut emitted = Vec::with_capacity(pending.len());
        for reminder in pending {
            if reminder.kind == RuntimeReminderKind::CommandCompleted
                && (!self.config.behavior.command_notifications.enabled
                    || reminder
                        .source
                        .as_deref()
                        .is_none_or(|id| !self.command_notice_unobserved(agent, id)))
            {
                continue;
            }
            emitted.push(self.emit_reminder(
                agent,
                request,
                reminder.kind,
                &reminder.body,
                reminder.source,
            )?);
        }
        self.sync_command_notices(agent);
        Ok(emitted)
    }

    /// Folds one appended or replayed event into reminder-derived state; never emits.
    pub(super) fn apply_guidance_state(&mut self, event: &EventEnvelopeV1) {
        self.apply_command_notice_state(event);
        self.apply_instruction_state(event);
    }

    pub(super) fn rebuild_instruction_state(&mut self, events: &[EventEnvelopeV1]) {
        self.instructions_seen.clear();
        self.pending_reminders.values_mut().for_each(|pending| {
            pending.retain(|reminder| reminder.kind != RuntimeReminderKind::DirectoryInstructions);
        });
        for event in crate::conversation_rewind::active_events(events).iter() {
            self.apply_instruction_state(event);
        }
    }

    fn apply_instruction_state(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::RuntimeReminder(reminder)
                if reminder.kind == RuntimeReminderKind::DirectoryInstructions =>
            {
                if let (Some(agent), Some(source)) =
                    (event.actor.agent_id.as_ref(), reminder.source.as_ref())
                {
                    self.instructions_seen
                        .entry(agent.clone())
                        .or_default()
                        .insert(PathBuf::from(source));
                }
            }
            EventV1::SessionCompaction(compaction) => {
                self.instructions_seen.remove(&compaction.agent_id);
            }
            _ => {}
        }
    }
}

impl CoordinatorHandle {
    /// Records a reminder for the worker's own running turn.
    pub(super) async fn emit_turn_reminder(
        &self,
        actor: EventActor,
        request_id: String,
        kind: RuntimeReminderKind,
        body: String,
        source: Option<String>,
    ) -> Result<(u64, String), CoordinatorError> {
        self.call(move |s| {
            s.check_task(&request_id)?;
            let job = &s.running[&request_id];
            if job.actor != actor || !matches!(job.kind, JobKind::Turn { .. }) {
                return Err(CoordinatorError::PermissionDenied(
                    "reminders require the current worker".into(),
                ));
            }
            let agent = actor.agent_id.clone().ok_or_else(|| {
                CoordinatorError::PermissionDenied("reminders require an agent".into())
            })?;
            s.emit_reminder(&agent, &request_id, kind, &body, source)
        })
        .await
    }

    /// The session's current todo items.
    pub(super) async fn todo_items(&self) -> Result<Vec<crate::proj::TodoItem>, CoordinatorError> {
        self.call(|s| Ok(s.todos.items())).await
    }
}
