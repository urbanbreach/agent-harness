use super::*;
use crate::coord::reminders::PendingReminder;

const DETAIL_LIMIT: usize = 16;
const SUMMARY_LIMIT: usize = 64;

#[derive(Default)]
pub(in crate::coord) struct CommandNoticeBacklog {
    // Overflow retains only identity/status, never another output body. One bounded
    // summary drains up to 64 identities per request, so even large bursts are bounded.
    pub pending: BTreeMap<String, (String, Option<String>)>,
    pub wake_attempted: bool,
}

impl CommandNoticeBacklog {
    fn remove_source(&mut self, source: &str) {
        if source.starts_with('[') {
            for id in notice_ids(source) {
                self.pending.remove(&id);
            }
        } else {
            self.pending.remove(source);
        }
    }

    fn extend(&mut self, other: Self) {
        let mut detailed = self
            .pending
            .values()
            .filter(|(_, body)| body.is_some())
            .take(DETAIL_LIMIT)
            .count();
        for (id, (status, body)) in other.pending {
            let body = if detailed < DETAIL_LIMIT { body } else { None };
            detailed += usize::from(body.is_some());
            self.pending.insert(id, (status, body));
        }
    }
}

fn notice_ids(source: &str) -> Vec<String> {
    serde_json::from_str(source).unwrap_or_default()
}

impl Runtime {
    pub(in crate::coord) fn command_notice_unobserved(&self, agent: &str, source: &str) -> bool {
        self.command_notices.get(agent).is_some_and(|backlog| {
            if source.starts_with('[') {
                notice_ids(source)
                    .iter()
                    .any(|id| backlog.pending.contains_key(id))
            } else {
                backlog.pending.contains_key(source)
            }
        })
    }

    pub(in crate::coord) fn apply_command_notice_state(&mut self, event: &EventEnvelopeV1) {
        let Some(agent) = event.actor.agent_id.as_deref() else {
            return;
        };
        match &event.payload {
            EventV1::CommandNotice(CommandNoticeEvent::Queued {
                task_id,
                status,
                body,
            }) => {
                for (owner, backlog) in &mut self.command_notices {
                    if owner != agent {
                        backlog.pending.remove(task_id);
                    }
                }
                let backlog = self.command_notices.entry(agent.to_owned()).or_default();
                let detailed = backlog
                    .pending
                    .values()
                    .filter(|(_, body)| body.is_some())
                    .take(DETAIL_LIMIT)
                    .count();
                backlog.pending.insert(
                    task_id.clone(),
                    (
                        status.clone(),
                        (detailed < DETAIL_LIMIT).then(|| body.clone()),
                    ),
                );
                backlog.wake_attempted = false;
                self.sync_command_notices(agent);
            }
            EventV1::CommandNotice(CommandNoticeEvent::Observed { task_id }) => {
                self.forget_command_notice(agent, task_id);
            }
            EventV1::CommandNotice(CommandNoticeEvent::WakeAttempted) => {
                self.command_notices
                    .entry(agent.to_owned())
                    .or_default()
                    .wake_attempted = true;
            }
            EventV1::RuntimeReminder(reminder)
                if reminder.kind == RuntimeReminderKind::CommandCompleted =>
            {
                if let Some(source) = &reminder.source
                    && let Some(backlog) = self.command_notices.get_mut(agent)
                {
                    backlog.remove_source(source);
                }
            }
            _ => {}
        }
    }

    pub(in crate::coord) fn sync_command_notices(&mut self, agent: &str) {
        let pending = self.pending_reminders.entry(agent.to_owned()).or_default();
        pending.retain(|reminder| reminder.kind != RuntimeReminderKind::CommandCompleted);
        if !self.config.behavior.command_notifications.enabled {
            return;
        }
        let Some(backlog) = self.command_notices.get(agent) else {
            return;
        };
        let mut overflow_ids = Vec::new();
        let mut summary = String::from("More background commands finished:\n");
        for (id, (status, body)) in &backlog.pending {
            if let Some(body) = body {
                pending.push(PendingReminder {
                    kind: RuntimeReminderKind::CommandCompleted,
                    body: body.clone(),
                    source: Some(id.clone()),
                });
            } else if overflow_ids.len() < SUMMARY_LIMIT {
                overflow_ids.push(id.as_str());
                summary.push_str(&format!("- {id}: {status}\n"));
            }
        }
        if !overflow_ids.is_empty() {
            summary.push_str("Use get_command_or_subagent_output to inspect output and log paths. Further completions will be reported in the next request.");
            pending.push(PendingReminder {
                kind: RuntimeReminderKind::CommandCompleted,
                body: summary,
                source: Some(serde_json::json!(overflow_ids).to_string()),
            });
        }
    }

    pub(in crate::coord) fn restore_command_notices(&mut self) {
        let agents: Vec<_> = self.command_notices.keys().cloned().collect();
        for agent in agents {
            if let Some(parent) = self.native_command_parent(&agent).map(str::to_owned) {
                if let Some(child) = self.command_notices.remove(&agent) {
                    self.command_notices
                        .entry(parent.clone())
                        .or_default()
                        .extend(child);
                }
                self.pending_reminders.remove(&agent);
                self.sync_command_notices(&parent);
            } else {
                self.sync_command_notices(&agent);
            }
        }
    }

    pub(super) fn notify_command_completed(&mut self, id: &str) -> Result<(), CoordinatorError> {
        if !self.config.behavior.command_notifications.enabled {
            return Ok(());
        }
        let Some(command) = self.commands.get(id) else {
            return Ok(());
        };
        let snapshot = &command.snapshot;
        let Some(owner) = snapshot.owner_agent_id.clone() else {
            return Ok(());
        };
        let owner = self
            .native_command_parent(&owner)
            .map(str::to_owned)
            .unwrap_or(owner);
        if snapshot.result.status == "cancelled" || !self.command_notice_allowed(&owner) {
            return Ok(());
        }
        let result = &snapshot.result;
        let status = match result.status.as_str() {
            "timed_out" => "timed out",
            other => other,
        };
        let exit = result
            .exit_code
            .map_or_else(String::new, |code| format!("\nExit code: {code}"));
        let body = format!(
            "Background command finished.\nTask id: {id}\nDescription: {}\nStatus: {status}{exit}\nDuration: {:.3} seconds\nOutput tail:\n{}\nFull output log: {}",
            snapshot.description.as_deref().unwrap_or(&result.command), result.duration_secs,
            command.output_tail.as_deref().unwrap_or_default(), result.output_file,
        );
        let status = result.status.clone();
        if let Some(command) = self.commands.get_mut(id)
            && command.snapshot.owner_agent_id.as_deref() != Some(owner.as_str())
        {
            command.snapshot.owner_agent_id = Some(owner.clone());
            command.updates.send_replace(command.snapshot.clone());
        }
        self.emit(
            EventActor::new(ActorKind::Worker, Some(owner.clone())),
            Some(id.to_owned()),
            EventV1::CommandNotice(CommandNoticeEvent::Queued {
                task_id: id.to_owned(),
                status,
                body,
            }),
        )?;
        if !self.agents.get(&owner).is_some_and(|agent| agent.busy) {
            self.start_next(&owner)?;
        }
        Ok(())
    }

    fn command_notice_allowed(&self, agent: &str) -> bool {
        self.stopping.is_none()
            && self.rewind.is_none()
            && self.fault.is_none()
            && self.agents.contains_key(agent)
            && self.native_command_parent(agent).is_none()
            && !self.killed_agents.contains(agent)
            && !self.stopped_sessions.contains(agent)
            && self
                .info()
                .is_ok_and(|run| !self.stopped_sessions.contains(run.run_id.as_str()))
    }

    pub(in crate::coord) fn queue_command_wake(
        &mut self,
        agent: &str,
    ) -> Result<bool, CoordinatorError> {
        if !self.config.behavior.command_notifications.enabled
            || !self.config.behavior.command_notifications.wake_idle
            || !self.command_notice_allowed(agent)
            || !self
                .agents
                .get(agent)
                .is_some_and(|state| !state.busy && state.queue.is_empty())
            || !self
                .command_notices
                .get(agent)
                .is_some_and(|backlog| !backlog.wake_attempted && !backlog.pending.is_empty())
            || self.check_turn_capacity(agent).is_err()
        {
            return Ok(false);
        }
        // Persist admission before hooks run. A rejected wake does not retry unchanged
        // notices; the next explicit turn or a new completion can admit them again.
        self.emit(
            EventActor::new(ActorKind::Worker, Some(agent.to_owned())),
            None,
            EventV1::CommandNotice(CommandNoticeEvent::WakeAttempted),
        )?;
        self.queue_turn(
            EventActor::new(ActorKind::Worker, Some(agent.to_owned())),
            agent,
            super::super::prompt::Prompt {
                reminder_wake: true,
                ..Default::default()
            },
            None,
            None,
            None,
        )?;
        Ok(true)
    }

    pub(in crate::coord) fn transfer_command_notices(
        &mut self,
        child: &str,
        parent: &str,
    ) -> Result<(), CoordinatorError> {
        let Some(backlog) = self.command_notices.remove(child) else {
            return Ok(());
        };
        self.pending_reminders.remove(child);
        for (id, (status, body)) in backlog.pending {
            self.emit(EventActor::new(ActorKind::Worker, Some(parent.to_owned())), Some(id.clone()),
                EventV1::CommandNotice(CommandNoticeEvent::Queued { task_id: id, status: status.clone(),
                    body: body.unwrap_or_else(|| format!("Background command finished with status {status}. Poll its output for details.")) }))?;
        }
        if !self.agents.get(parent).is_some_and(|agent| agent.busy) {
            self.start_next(parent)?;
        }
        Ok(())
    }
}
