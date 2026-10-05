use super::*;

mod completion;
mod disposal;
mod reminder_text;
mod reminders;
mod restore;

#[derive(Serialize)]
struct NativeMetadata<'a> {
    schema_version: u16,
    subagent_id: &'a str,
    parent_session_id: &'a str,
    spawner: &'a str,
    subagent_type: &'a str,
    description: &'a str,
    status: &'a str,
    started: &'a str,
    ended: Option<&'a str>,
    tool_calls: u32,
    turns: u32,
    duration_ms: u64,
    error: Option<&'a str>,
    cwd: String,
    worktree_path: Option<String>,
    snapshot_ref: Option<&'a str>,
    finalized_state: Option<&'a FinalizedAgentStateReferenceV1>,
}

impl Runtime {
    pub(super) fn write_native_projection(
        &self,
        agent: &str,
        terminal: bool,
    ) -> Result<(), CoordinatorError> {
        let Some(child) = self.native_subagents.get(agent) else {
            return Ok(());
        };
        // A wake has no new Prepared publication. Its prior projection remains
        // intact until actual Started admission has accepted the incarnation.
        if child.displaced.is_some() && child.phase != NativePhase::Running {
            return Ok(());
        }
        let snapshot = child
            .pending_terminal
            .clone()
            .unwrap_or_else(|| child.updates.borrow().clone());
        let record = self.subagent_history.records.get(agent);
        let accounting = record.and_then(|record| record.accounting.as_ref());
        let metadata = NativeMetadata {
            schema_version: 1,
            subagent_id: agent,
            parent_session_id: self.info()?.run_id.as_str(),
            spawner: &child.registration.spawner,
            subagent_type: &child.registration.subagent_type,
            description: &child.registration.description,
            status: if terminal {
                &snapshot.result.status
            } else {
                "running"
            },
            started: &child.started,
            ended: child.ended.as_deref(),
            tool_calls: accounting.map_or(0, |a| a.tool_calls),
            turns: accounting.map_or(0, |a| a.turns),
            duration_ms: self.clock.mono_ms().saturating_sub(child.started_ms),
            error: snapshot.error.as_deref(),
            cwd: child.cwd.to_string_lossy().into_owned(),
            worktree_path: child
                .worktree
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            snapshot_ref: child.snapshot_ref.as_deref(),
            finalized_state: self.agents.get(agent).and_then(|a| a.finalized.as_ref()),
        };
        let directory = self.info()?.run_dir.join("subagents").join(agent);
        crate::store::create_private_dir(&directory)?;
        let mut metadata = serde_json::to_value(metadata)?;
        crate::redact::redact_in_place(self.redactor.as_ref(), &mut metadata);
        let bytes = serde_json::to_vec_pretty(&metadata)?;
        crate::store::write_private_atomic(&directory.join("meta.json"), &bytes)?;
        if terminal
            && let Some(output) = snapshot
                .completed
                .as_ref()
                .filter(|output| !output.output.is_empty())
        {
            let mut output = serde_json::json!({
                "schema_version": 1, "subagent_id": agent,
                "output": output.output, "tool_calls": output.tool_calls,
                "turns": output.turns, "duration_ms": output.duration_ms,
            });
            crate::redact::redact_in_place(self.redactor.as_ref(), &mut output);
            crate::store::write_private_atomic(
                &directory.join("output.json"),
                &serde_json::to_vec(&output)?,
            )?;
        }
        Ok(())
    }

    fn native_terminal_publication(
        &mut self,
        agent: &str,
        terminal: &EventEnvelopeV1,
    ) -> Result<(), CoordinatorError> {
        let child = &self.native_subagents[agent];
        let snapshot = child.updates.borrow().clone();
        let direct = child.registration.spawner == child.registration.root_agent;
        let should_wake = child.registration.background
            && direct
            && child.registration.parent_request.is_some()
            && !child.consumed
            && child.waiters.is_empty()
            && !child.explicitly_killed
            && snapshot.result.status != "cancelled"
            && self.stopping.is_none()
            && self.rewind.is_none()
            && self.fault.is_none()
            && !self
                .stopped_sessions
                .contains(&child.registration.root_agent);
        let parent = child.registration.spawner.clone();
        let request = child.request.clone().unwrap_or_default();
        let description = child.registration.description.clone();
        let summary = snapshot.completed.as_ref().map_or_else(
            || snapshot.error.clone().unwrap_or_default(),
            |output| output.output.clone(),
        );
        let wake_text = self.native_completion_reminder(agent, &parent);
        let reserved = if should_wake {
            Some(self.id("turn")?)
        } else {
            None
        };
        self.emit(
            EventActor::new(ActorKind::Worker, Some(parent.clone())),
            Some(request.clone()),
            EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
                parent_session_id: if direct {
                    self.info()?.run_id.to_string().into()
                } else {
                    parent.clone().into()
                },
                parent_agent_id: Some(parent.clone()),
                child_session_id: agent.into(),
                child_request_id: request.clone(),
                task_id: request.clone().into(),
                description,
                status: match snapshot.result.status.as_str() {
                    "completed" => BackgroundTaskNotificationStatus::Completed,
                    "failed" => BackgroundTaskNotificationStatus::Failed,
                    _ => BackgroundTaskNotificationStatus::Cancelled,
                },
                summary: summary.clone(),
                terminal_event_id: terminal.event_id.clone(),
                terminal_task_id: request.clone(),
                delivered_turn_request_id: reserved.clone(),
            }),
        )?;
        if let Some(reserved) = reserved {
            self.queue_turn(
                EventActor::new(ActorKind::Worker, Some(parent.clone())),
                &parent,
                super::super::prompt::Prompt {
                    text: wake_text,
                    reserved_id: Some(reserved),
                    child_completion: Some(request),
                    ..Default::default()
                },
                None,
                None,
                None,
            )?;
        }
        self.emit(
            EventActor::new(ActorKind::Worker, Some(agent.into())),
            self.agents[agent].attempt.clone(),
            EventV1::NativeSubagentReceipt(Box::new(NativeSubagentReceipt {
                payload_version: 1,
                child_id: agent.into(),
                attempt_id: self.agents[agent].attempt.clone(),
                generation: self.agents[agent].generation,
                kind: "terminal_published".into(),
                waiter_id: None,
            })),
        )?;
        let buffered_for = if direct
            || self
                .native_subagents
                .get(&parent)
                .is_some_and(|spawner| spawner.phase == NativePhase::Running)
        {
            Some(parent)
        } else {
            None
        };
        if let Some(child) = self.native_subagents.get_mut(agent) {
            child.terminal_published = true;
            child.buffered_for = buffered_for;
        }
        let mut buffered: Vec<_> = self
            .native_subagents
            .iter()
            .filter(|(_, child)| child.buffered_for.is_some())
            .map(|(id, child)| (child.completion_age, id.clone()))
            .collect();
        buffered.sort();
        let excess = buffered.len().saturating_sub(256);
        for (_, id) in buffered.into_iter().take(excess) {
            if let Some(child) = self.native_subagents.get_mut(&id) {
                child.buffered_for = None;
            }
        }
        Ok(())
    }

    fn route_native_survivors(&mut self, parent: &str) -> Result<(), CoordinatorError> {
        let ids: Vec<_> = self
            .native_subagents
            .iter()
            .filter(|(_, child)| {
                child.registration.spawner == parent && child.phase != NativePhase::Terminal
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            if let Some(child) = self.native_subagents.get_mut(&id) {
                child.routed_to_root = true;
                child.buffered_for = None;
            }
            if let Some(attempt) = self.agents[&id].attempt.clone() {
                let transition = self.agent_transition(
                    &id,
                    &attempt,
                    self.agents[&id].generation,
                    SubagentTransitionKind::Routed,
                    None,
                    None,
                    None,
                )?;
                let _ = self.commit_subagent_transition(transition)?;
            }
        }
        for child in self.native_subagents.values_mut() {
            if child.buffered_for.as_deref() == Some(parent) {
                child.buffered_for = None;
            }
        }
        Ok(())
    }

    fn evict_native_completed(&mut self) {
        let mut terminal: Vec<_> = self
            .native_subagents
            .iter()
            .filter(|(_, child)| child.phase == NativePhase::Terminal)
            .map(|(id, child)| (child.completion_age, id.clone()))
            .collect();
        terminal.sort();
        let excess = terminal.len().saturating_sub(1024);
        for (_, id) in terminal.into_iter().take(excess) {
            self.native_subagents.remove(&id);
        }
    }
}

pub(super) fn completed_body(output: &SpawnSubagentOutput) -> String {
    let id = &output.subagent_id;
    let mut body = format!(
        "{}\n\n<subagent_meta>id={id}, tool_calls={}, turns={}, duration_ms={}</subagent_meta>",
        output.output, output.tool_calls, output.turns, output.duration_ms
    );
    if let Some(path) = &output.worktree_path {
        body.push_str(&format!("\n<worktree_path>{path}</worktree_path>"));
    }
    body.push_str(&format!("\n\n<subagent_result>\nsubagent_id: {id}\nTo continue this subagent's conversation, use resume_from=\"{id}\".\n</subagent_result>"));
    body
}
