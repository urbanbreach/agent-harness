use super::*;

/// Current work, independent of text retained from earlier samples in the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveTurnPhase {
    Waiting,
    WaitingFor(WaitingReason),
    Compacting,
    Retrying(u32),
    Thinking,
    Responding,
    WritingToolCall {
        tool_call_id: String,
        ordinal: usize,
        tool_name: Option<String>,
    },
    ToolRunning(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WaitingReason {
    Subagents(usize),
    TaskOutput(Vec<String>),
    TasksComplete,
    Sleep,
}

#[derive(Clone)]
pub(super) struct ProviderPhase {
    request_id: String,
    phase: LiveTurnPhase,
    started_mono_ms: u64,
    activity_before_retry: Option<LiveTurnPhase>,
    activity_before_writing: Option<LiveTurnPhase>,
    updated_at: std::time::Instant,
}

impl SessionProjection {
    pub(in crate::app) fn reported_child_phase(&self, activity: &ActivityEntry) -> LiveTurnPhase {
        let phase = self.live_turn_phase(activity).0;
        if matches!(
            phase,
            LiveTurnPhase::Waiting
                | LiveTurnPhase::Responding
                | LiveTurnPhase::Thinking
                | LiveTurnPhase::ToolRunning(_)
        ) && let Some(state) = self
            .provider_phases
            .get(&activity.request_id)
            .filter(|state| matches!(state.phase, LiveTurnPhase::WritingToolCall { .. }))
        {
            return state.phase.clone();
        }
        if phase == LiveTurnPhase::Compacting {
            return self
                .provider_phases
                .get(&activity.request_id)
                .map_or(LiveTurnPhase::Waiting, |state| state.phase.clone());
        }
        if matches!(phase, LiveTurnPhase::Retrying(_)) {
            // Retry notices update the child status immediately. The parent's
            // compact row retains the last activity until child output resumes.
            return self
                .provider_phases
                .get(&activity.request_id)
                .and_then(|state| state.activity_before_retry.clone())
                .unwrap_or(phase);
        }
        phase
    }

    pub(crate) fn live_turn_phase(&self, activity: &ActivityEntry) -> (LiveTurnPhase, u64) {
        let provider = self.provider_phases.get(&activity.request_id);
        let phase = provider.map_or(LiveTurnPhase::Waiting, |state| state.phase.clone());
        let started = provider.map_or_else(
            || {
                activity
                    .request_started_mono_ms
                    .unwrap_or(activity.first_mono_ms)
            },
            |state| state.started_mono_ms,
        );
        // The priority matches the child activity tracker: retry, compaction,
        // blocking waits, fresh argument stream, thinking, tools, response.
        if matches!(phase, LiveTurnPhase::Retrying(_)) {
            return (phase, started);
        }
        let agent = self.child_request_agents.get(&activity.request_id);
        if let Some(compaction) = agent
            .and_then(|id| self.live_compactions.get(id))
            .filter(|state| state.preview.is_some())
        {
            return (LiveTurnPhase::Compacting, compaction.started_mono_ms);
        }
        let running = activity
            .tool_calls
            .iter()
            .filter(|tool| tool.status == ToolCallDisplayStatus::Running);
        if let Some((priority, reason, started)) = running
            .clone()
            .filter_map(|tool| {
                self.blocking_wait(tool)
                    .map(|(priority, reason)| (priority, reason, tool.first_mono_ms))
            })
            .min_by_key(|(priority, _, _)| *priority)
        {
            let _ = priority;
            return (LiveTurnPhase::WaitingFor(reason), started);
        }
        let phase = if matches!(phase, LiveTurnPhase::WritingToolCall { .. })
            && provider.is_some_and(|state| {
                self.phase_now()
                    .saturating_duration_since(state.updated_at)
                    .as_secs()
                    >= 10
            }) {
            provider
                .and_then(|state| state.activity_before_writing.clone())
                .unwrap_or(LiveTurnPhase::Waiting)
        } else {
            phase
        };
        if matches!(
            phase,
            LiveTurnPhase::Thinking | LiveTurnPhase::WritingToolCall { .. }
        ) {
            return (phase, started);
        }
        if let Some(tool) = running
            .filter(|tool| !Self::suppressed_activity_tool(tool))
            .next_back()
        {
            return (
                LiveTurnPhase::ToolRunning(tool.tool_call_id.clone()),
                tool.first_mono_ms,
            );
        }
        if phase == LiveTurnPhase::Waiting {
            let children = self
                .subagents
                .rows
                .values()
                .filter(|row| {
                    !row.background
                        && row.parent_request.as_deref() == Some(activity.request_id.as_str())
                        && self
                            .subagents
                            .history
                            .records
                            .get(&row.id)
                            .is_none_or(|record| record.outcome.is_none())
                })
                .count();
            if children > 0 {
                return (
                    LiveTurnPhase::WaitingFor(WaitingReason::Subagents(children)),
                    started,
                );
            }
        }
        let started = if phase == LiveTurnPhase::Waiting {
            // Waiting begins when the last running tool finishes, even before the next request.
            activity
                .tool_calls
                .iter()
                .map(|tool| tool.last_mono_ms)
                .max()
                .unwrap_or(0)
                .max(started)
        } else {
            started
        };
        (phase, started)
    }

    pub(super) fn note_retry(
        &mut self,
        turn_id: &str,
        retry: ProviderRequestRetryMetadata,
        mono_ms: u64,
    ) {
        let Some(request) = self
            .activities
            .iter()
            .find(|activity| activity.request_id == turn_id)
            .and_then(|activity| activity.request_data.as_ref())
        else {
            return;
        };
        let request_id = request.request_id.to_string();
        self.set_provider_phase(
            turn_id,
            &request_id,
            LiveTurnPhase::Retrying(retry.attempt),
            mono_ms,
        );
        if let Some(activity) = self
            .activities
            .iter_mut()
            .find(|activity| activity.request_id == turn_id)
        {
            if let Some(request) = &mut activity.request_data {
                request.metadata.get_or_insert_with(Default::default).retry = Some(retry);
            }
            activity.status = ActivityStatus::Streaming;
            activity.error_message = None;
        }
    }

    fn set_provider_phase(
        &mut self,
        turn_id: &str,
        request_id: &str,
        phase: LiveTurnPhase,
        mono_ms: u64,
    ) {
        let activity_before_retry = matches!(phase, LiveTurnPhase::Retrying(_))
            .then(|| {
                self.activities
                    .iter()
                    .find(|activity| activity.request_id == turn_id)
                    .map(|activity| self.reported_child_phase(activity))
            })
            .flatten();
        let activity_before_writing = if matches!(phase, LiveTurnPhase::WritingToolCall { .. }) {
            self.provider_phases.get(turn_id).and_then(|state| {
                if matches!(state.phase, LiveTurnPhase::WritingToolCall { .. }) {
                    state.activity_before_writing.clone()
                } else {
                    Some(state.phase.clone())
                }
            })
        } else {
            None
        };
        let now = self.phase_now();
        let state = self
            .provider_phases
            .entry(turn_id.to_string())
            .or_insert_with(|| ProviderPhase {
                request_id: request_id.to_string(),
                phase: phase.clone(),
                started_mono_ms: mono_ms,
                activity_before_retry: activity_before_retry.clone(),
                updated_at: now,
                activity_before_writing: activity_before_writing.clone(),
            });
        state.updated_at = now;
        state.activity_before_writing = activity_before_writing;
        if state.request_id != request_id || state.phase != phase {
            let continuing_arguments = state.request_id == request_id
                && matches!(state.phase, LiveTurnPhase::WritingToolCall { .. })
                && matches!(phase, LiveTurnPhase::WritingToolCall { .. });
            state.request_id = request_id.to_string();
            state.phase = phase;
            if !continuing_arguments {
                state.started_mono_ms = mono_ms;
            }
            state.activity_before_retry = activity_before_retry;
        }
    }

    pub(super) fn update_phase_for_event(&mut self, event: &EventEnvelopeV1) {
        let (request_id, phase) =
            if let Some(fragment) = canonical_provider_fragment_for_event(event) {
                if fragment.delta.is_empty() {
                    return;
                }
                (
                    fragment.request_id,
                    match fragment.kind {
                        CanonicalProviderFragmentKind::Reasoning => LiveTurnPhase::Thinking,
                        CanonicalProviderFragmentKind::Text => LiveTurnPhase::Responding,
                    },
                )
            } else {
                match &event.payload {
                    EventV1::ProviderRequestStarted(data) => (
                        data.request_id.as_str(),
                        data.metadata
                            .as_ref()
                            .and_then(|metadata| metadata.retry)
                            .filter(|retry| retry.attempt > 0)
                            .map_or(LiveTurnPhase::Waiting, |retry| {
                                LiveTurnPhase::Retrying(retry.attempt)
                            }),
                    ),
                    EventV1::AssistantMessageFinished(data)
                        if data.parts.iter().any(
                            |part| matches!(part, AssistantPart::Text { text } if !text.is_empty()),
                        ) =>
                    {
                        (data.request_id.as_str(), LiveTurnPhase::Responding)
                    }
                    EventV1::ProviderRequestFinished(data) => {
                        (data.request_id.as_str(), LiveTurnPhase::Waiting)
                    }
                    EventV1::ToolCallRequested(_) => {
                        let turn_id = event.correlation_id.clone().or_else(|| {
                            self.activities
                                .back()
                                .map(|activity| activity.request_id.clone())
                        });
                        let Some(turn_id) = turn_id else {
                            return;
                        };
                        let request_id = self
                            .provider_phases
                            .get(&turn_id)
                            .map_or(turn_id.as_str(), |state| state.request_id.as_str())
                            .to_string();
                        self.set_provider_phase(
                            &turn_id,
                            &request_id,
                            LiveTurnPhase::Waiting,
                            event.mono_ms,
                        );
                        return;
                    }
                    _ => return,
                }
            };
        let turn_id = Self::canonical_provider_turn_id(event, request_id);
        self.set_provider_phase(turn_id, request_id, phase, event.mono_ms);
    }

    pub(super) fn update_phase_for_live_fragment(
        &mut self,
        event: &LiveEventEnvelope,
        index: usize,
    ) {
        let activity = &self.activities[index];
        let (request_id, delta, phase) = match &event.payload {
            LiveEventV1::ProviderRetrying { .. }
            | LiveEventV1::CompactionProgress { .. }
            | LiveEventV1::RuntimeWarning { .. }
            | LiveEventV1::SubagentProgress(_)
            | LiveEventV1::EvalProgress { .. } => return,
            LiveEventV1::ProviderReasoningDelta { request_id, delta } => {
                (request_id, delta, LiveTurnPhase::Thinking)
            }
            LiveEventV1::ProviderTextDelta { request_id, delta } => {
                (request_id, delta, LiveTurnPhase::Responding)
            }
            LiveEventV1::ProviderToolInputDelta {
                request_id,
                tool_call_id,
                tool_name,
                delta,
            } => {
                let seen = self
                    .transient_assistants
                    .get(request_id.as_str())
                    .map(|state| &state.tool_call_ids);
                let ordinal = seen.map_or(1, |ids| {
                    activity
                        .tool_calls
                        .iter()
                        .filter(|tool| ids.contains(&tool.tool_call_id))
                        .position(|tool| tool.tool_call_id == tool_call_id.as_str())
                        .unwrap_or(ids.len())
                        + 1
                });
                (
                    request_id,
                    delta,
                    LiveTurnPhase::WritingToolCall {
                        tool_call_id: tool_call_id.to_string(),
                        ordinal,
                        tool_name: tool_name.clone().or_else(|| {
                            activity
                                .tool_calls
                                .iter()
                                .find(|tool| tool.tool_call_id == tool_call_id.as_str())
                                .filter(|tool| tool.tool_id != "tool")
                                .map(|tool| tool.tool_id.clone())
                        }),
                    },
                )
            }
        };
        if delta.is_empty()
            && !matches!(
                &phase,
                LiveTurnPhase::WritingToolCall {
                    tool_name: Some(_),
                    ..
                }
            )
        {
            return;
        }
        let turn_id = activity.request_id.clone();
        self.set_provider_phase(&turn_id, request_id.as_str(), phase, event.mono_ms);
    }
}

impl SessionProjection {
    pub(super) fn phase_now(&self) -> std::time::Instant {
        self.phase_clock
            .as_ref()
            .map_or_else(std::time::Instant::now, |clock| clock())
    }

    fn suppressed_activity_tool(tool: &ToolCallEntry) -> bool {
        matches!(
            tool.effective_tool_id(),
            "get_command_or_subagent_output"
                | "get_task_output"
                | "get_task_or_subagent_output"
                | "wait_commands_or_subagents"
                | "wait_tasks"
                | "wait_tasks_or_subagents"
                | "background_output"
                | "kill_task"
                | "kill_command_or_subagent"
                | "background_cancel"
                | "Await"
                | "AwaitShell"
                | "sleep"
                | "wait"
                | "spawn_subagent"
                | "task"
                | "agent.spawn"
        )
    }

    fn blocking_wait(&self, tool: &ToolCallEntry) -> Option<(u8, WaitingReason)> {
        let args =
            serde_json::from_str::<serde_json::Value>(&tool.args_summary).unwrap_or_default();
        match tool.effective_tool_id() {
            "background_output"
            | "get_command_or_subagent_output"
            | "get_task_output"
            | "get_task_or_subagent_output"
                if args
                    .get("timeout_ms")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|timeout| timeout > 0)
                    || args.get("block").and_then(serde_json::Value::as_bool) == Some(true) =>
            {
                let mut ids = Vec::new();
                for id in args
                    .get("task_ids")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    if !ids.iter().any(|seen| seen == id) {
                        ids.push(id.to_owned());
                    }
                }
                if ids.is_empty()
                    && let Some(id) = args
                        .get("task_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                {
                    ids.push(id.to_owned());
                }
                Some((0, WaitingReason::TaskOutput(ids)))
            }
            "wait_commands_or_subagents" | "wait_tasks" | "wait_tasks_or_subagents" => {
                Some((1, WaitingReason::TasksComplete))
            }
            "Await" | "AwaitShell" | "sleep" | "wait" => Some((2, WaitingReason::Sleep)),
            "spawn_subagent" | "task" | "agent.spawn"
                if tool_call_is_foreground_child_wait(tool) =>
            {
                Some((3, WaitingReason::Subagents(1)))
            }
            _ => None,
        }
    }

    pub(crate) fn waiting_label(&self, reason: &WaitingReason, request_id: &str) -> String {
        use super::super::subagents::activity::{clamp_subject, subject_label};
        let owner = self.child_request_agents.get(request_id);
        match reason {
            WaitingReason::Subagents(count) => {
                if *count > 1 {
                    format!("Waiting for {count} subagents…")
                } else {
                    "Waiting for subagent…".into()
                }
            }
            WaitingReason::TaskOutput(ids) => {
                let subject = ids.iter().find_map(|id| self.wait_task_subject(id, owner));
                subject.map_or_else(
                    || "Waiting on task output…".into(),
                    |subject| {
                        if ids.len() > 1 {
                            let suffix = format!(" + {} more", ids.len() - 1);
                            let budget = 40usize.saturating_sub(suffix.chars().count()).max(8);
                            let base: String =
                                clamp_subject(&subject).chars().take(budget).collect();
                            subject_label(&format!("{base}{suffix}"))
                        } else {
                            subject_label(&subject)
                        }
                    },
                )
            }
            WaitingReason::TasksComplete => "Waiting on tasks…".into(),
            WaitingReason::Sleep => "Sleeping…".into(),
        }
    }
    fn wait_task_subject(&self, id: &str, owner: Option<&String>) -> Option<String> {
        use super::super::subagents::activity::{clamp_subject, tool_activity};
        let tool = self
            .activities
            .iter()
            .filter(|activity| self.child_request_agents.get(&activity.request_id) == owner)
            .flat_map(|activity| &activity.tool_calls)
            .find(|tool| {
                tool.output_json
                    .as_ref()
                    .and_then(|v| v.get("task_id"))
                    .and_then(serde_json::Value::as_str)
                    == Some(id)
            });
        if let Some(tool) = tool {
            let (command, description) = tool_activity(tool);
            if description.is_some() {
                return description;
            }
            let command = command.lines().map(str::trim).find(|line| !line.is_empty());
            if let Some(command) = command.filter(|command| command.chars().count() <= 40) {
                return Some(command.to_owned());
            }
        }
        self.subagents
            .rows
            .get(id)
            .filter(|row| {
                owner.map_or_else(
                    || !self.child_agent_ids.contains(&row.spawner),
                    |owner| owner == &row.spawner,
                )
            })
            .filter(|row| !row.description.trim().is_empty())
            .map(|row| clamp_subject(&row.description))
    }
}
