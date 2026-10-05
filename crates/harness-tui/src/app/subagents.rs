//! Event-derived child presentation. The core fold owns attempt reconciliation.
use std::collections::BTreeMap;
pub(crate) mod activity;

use harness_core::{
    event::{EventEnvelopeV1, EventV1},
    subagent::{SubagentHistory, SubagentTerminalOutcome, SubagentTransitionKind},
};

use super::{ActivityStatus, AppState};

#[derive(Debug, Clone)]
pub(crate) struct SubagentRow {
    pub id: String,
    pub spawner: String,
    pub parent_tool: String,
    pub parent_request: Option<String>,
    pub first_seq: u64,
    pub label: String,
    pub description: String,
    pub display_description: String,
    pub model: String,
    pub meta: String,
    pub background: bool,
    pub resumed: bool,
    pub forked: bool,
    pub started_ms: u64,
}

impl SubagentRow {
    pub(crate) fn context_badge(&self) -> &'static str {
        if self.resumed {
            "resumed"
        } else if self.forked {
            "forked"
        } else {
            ""
        }
    }
}

#[derive(Default)]
pub(crate) struct SubagentPresentation {
    pub history: SubagentHistory,
    pub rows: BTreeMap<String, SubagentRow>,
    pub last_mono_ms: u64,
    pub observed_at: Option<std::time::Instant>,
    pub progress: BTreeMap<String, harness_core::event::SubagentProgressEvent>,
    pub attempts: BTreeMap<(String, u64), SubagentAttempt>,
}

pub(crate) struct SubagentAttempt {
    pub row: SubagentRow,
    pub first_seq: u64,
    pub finished_seq: Option<u64>,
    pub outcome: Option<SubagentTerminalOutcome>,
    pub duration_ms: Option<u64>,
    pub request_id: Option<String>,
    pub error: Option<String>,
}

impl SubagentPresentation {
    pub(super) fn apply_progress(
        &mut self,
        progress: &harness_core::event::SubagentProgressEvent,
        mono_ms: u64,
    ) {
        let Some(record) = self.history.records.get(&progress.child_id) else {
            return;
        };
        if record.outcome.is_some()
            || record.generation != progress.generation
            || record.lifecycle.current_attempt_id() != Some(progress.attempt_id.as_str())
        {
            return;
        }
        self.last_mono_ms = self.last_mono_ms.max(mono_ms);
        self.progress
            .insert(progress.child_id.clone(), progress.clone());
    }

    pub(super) fn apply(&mut self, event: &EventEnvelopeV1) {
        self.last_mono_ms = self.last_mono_ms.max(event.mono_ms);
        let accepted = match &event.payload {
            EventV1::SubagentTransition(data) => self.history.apply_transition(data, event.mono_ms),
            _ => {
                self.history.apply(event);
                false
            }
        };
        match &event.payload {
            EventV1::SubagentTransition(data) if accepted && data.outcome.is_some() => {
                self.progress.remove(&data.child_id.0);
            }
            EventV1::NativeSubagentRegistered(data) if data.payload_version == 1 => {
                if self
                    .rows
                    .get(&data.child_id)
                    .is_some_and(|row| row.first_seq >= event.seq)
                {
                    return;
                }
                let (tag, description) = data
                    .description
                    .strip_prefix('[')
                    .and_then(|rest| rest.split_once(']'))
                    .filter(|(tag, _)| !tag.trim().is_empty())
                    .map_or((None, data.description.as_str()), |(tag, description)| {
                        (Some(tag.trim()), description.trim_start())
                    });
                let label = [data.persona.as_deref(), data.role.as_deref(), tag]
                    .into_iter()
                    .flatten()
                    .map(str::trim)
                    .find(|part| !part.is_empty())
                    .unwrap_or("subagent");
                let mut chars = label.chars();
                let label = chars.next().map_or(String::new(), |first| {
                    first.to_uppercase().chain(chars).collect()
                });
                let mut parts = Vec::new();
                for part in [data.persona.as_deref(), data.role.as_deref()]
                    .into_iter()
                    .flatten()
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                {
                    if !parts
                        .iter()
                        .any(|existing: &&str| existing.eq_ignore_ascii_case(part))
                    {
                        parts.push(part);
                    }
                }
                if !data.model.trim().is_empty() {
                    parts.push(data.model.trim());
                }
                let meta = if parts.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", parts.join(" · "))
                };
                self.rows.insert(
                    data.child_id.clone(),
                    SubagentRow {
                        id: data.child_id.clone(),
                        spawner: data.spawner.clone(),
                        parent_tool: data.parent_tool.clone(),
                        parent_request: data.parent_request.clone(),
                        first_seq: event.seq,
                        label,
                        description: data.description.clone(),
                        display_description: description.into(),
                        model: data.model.clone(),
                        meta,
                        background: data.background,
                        resumed: data.source.is_some(),
                        forked: data.fork_context,
                        started_ms: event.mono_ms,
                    },
                );
            }
            EventV1::SubagentTransition(data)
                if accepted && data.transition == SubagentTransitionKind::Spawned =>
            {
                self.progress.remove(&data.child_id.0);
                if let Some(row) = self.rows.get_mut(&data.child_id.0)
                    && self.history.records.get(&row.id).is_some_and(|record| {
                        record.generation == data.generation
                            && record.lifecycle.current_attempt_id() == data.attempt_id.as_deref()
                    })
                {
                    row.started_ms = event.mono_ms;
                    row.first_seq = event.seq;
                    row.background = data.metadata.notification_route.background;
                    row.resumed |= data.generation > 1;
                }
            }
            EventV1::BackgroundTaskNotification(data) => {
                if let Some(attempt) = self.attempts.values_mut().find(|attempt| {
                    attempt.row.id == data.child_session_id.as_str()
                        && attempt.request_id.as_deref() == Some(data.child_request_id.as_str())
                }) && data.status
                    == harness_core::event::BackgroundTaskNotificationStatus::Failed
                {
                    attempt.error = (!data.summary.is_empty()).then(|| data.summary.clone());
                }
            }
            _ => {}
        }
        if let EventV1::SubagentTransition(data) = &event.payload
            && let Some((row, record)) = self.rows.get(&data.child_id.0).zip(
                self.history.records.get(&data.child_id.0).filter(|record| {
                    accepted
                        && record.generation == data.generation
                        && record.lifecycle.has_current_attempt()
                }),
            )
        {
            let attempt = self
                .attempts
                .entry((row.id.clone(), record.generation))
                .or_insert_with(|| SubagentAttempt {
                    row: row.clone(),
                    first_seq: event.seq,
                    finished_seq: None,
                    outcome: None,
                    duration_ms: None,
                    request_id: record.lifecycle.current_attempt_id().map(str::to_owned),
                    error: None,
                });
            if record.lifecycle.is_finished() && record.outcome.is_some() {
                attempt.finished_seq.get_or_insert(event.seq);
                attempt.outcome = record.outcome;
                attempt.duration_ms = record.accounting.map(|accounting| accounting.duration_ms);
            }
        }
    }
}

impl AppState {
    pub(crate) fn is_native_subagent_target(
        &self,
        target: &crate::ui::TranscriptMouseTarget,
    ) -> bool {
        use crate::ui::TranscriptMouseTarget;
        match target {
            TranscriptMouseTarget::SubagentSession { session_id } => {
                self.subagents.rows.contains_key(session_id)
            }
            TranscriptMouseTarget::ToolGroup { tool_call_ids } => tool_call_ids.iter().any(|id| {
                self.native_subagent_task(id).is_some()
                    || id.starts_with("background-notification:")
                    || id.starts_with("subagent-started:")
            }),
            _ => false,
        }
    }

    pub(crate) fn native_subagent_selected(&self) -> bool {
        self.selected_transcript_entry()
            .and_then(|entry| entry.target)
            .is_some_and(|target| self.is_native_subagent_target(&target))
    }
}

pub(crate) struct SubagentHeader {
    pub label: String,
    pub description: String,
    pub model: String,
    pub cwd: String,
    pub status: ActivityStatus,
    pub elapsed_ms: u64,
    pub badge: &'static str,
    pub activity: String,
    pub phase_elapsed_ms: u64,
    pub cancelling: bool,
}

impl super::SessionProjection {
    pub(crate) fn native_subagent_task(
        &self,
        tool_id: &str,
    ) -> Option<super::OrchestrationTaskRow> {
        use super::OrchestrationTaskState as State;
        let row = self
            .subagents
            .rows
            .values()
            .find(|row| row.parent_tool == tool_id)?;
        let record = self.subagents.history.records.get(&row.id);
        let state = match record.and_then(|record| record.outcome) {
            Some(
                SubagentTerminalOutcome::Completed | SubagentTerminalOutcome::StationarityEnded,
            ) => State::Completed,
            Some(
                SubagentTerminalOutcome::Cancelled | SubagentTerminalOutcome::RemovedFromQueue,
            ) => State::Cancelled,
            Some(_) => State::Failed,
            None if record.is_some_and(|record| record.lifecycle.has_current_attempt()) => {
                State::Running
            }
            None => State::Queued,
        };
        let request = record
            .and_then(|record| record.lifecycle.current_attempt_id())
            .map(str::to_owned);
        Some(super::OrchestrationTaskRow {
            task_id: row.id.clone(),
            queue_key: Some("subagent".into()),
            state,
            warning: None,
            owner_kind: harness_core::event::ActorKind::Worker,
            owner_agent_id: Some(row.id.clone()),
            request_id: request.clone(),
            parent_tool_call_id: Some(row.parent_tool.clone()),
            parent_request_id: row.parent_request.clone(),
            child_session_id: Some(row.id.clone()),
            child_request_id: request.clone(),
            result_summary: None,
            child_tool_call_count: record
                .and_then(|record| record.accounting)
                .map(|accounting| accounting.tool_calls as usize)
                .or_else(|| {
                    self.subagents
                        .progress
                        .get(&row.id)
                        .map(|progress| progress.tool_call_count as usize)
                })
                .unwrap_or(0),
            current_child_tool_title: self
                .activities
                .iter()
                .find(|activity| Some(activity.request_id.as_str()) == request.as_deref())
                .map(|activity| self.reported_child_activity(activity)),
            timing_elapsed_ms: record
                .and_then(|record| record.accounting)
                .map(|accounting| accounting.duration_ms),
            first_seq: row.first_seq,
            last_seq: row.first_seq,
            first_mono_ms: row.started_ms,
            last_mono_ms: self.subagents.last_mono_ms,
            first_timestamp: None,
            last_timestamp: None,
        })
    }
}

impl AppState {
    pub(crate) fn subagent_elapsed_ms(&self, row: &SubagentRow) -> u64 {
        if let Some(accounting) = self
            .subagents
            .history
            .records
            .get(&row.id)
            .and_then(|record| record.accounting)
        {
            return accounting.duration_ms;
        }
        self.subagent_presentation_ms()
            .saturating_sub(row.started_ms)
    }

    fn subagent_presentation_ms(&self) -> u64 {
        let extra = if self.presentation_is_live() {
            self.subagents.observed_at.map_or(0, |at| {
                u64::try_from(self.now().saturating_duration_since(at).as_millis())
                    .unwrap_or(u64::MAX)
            })
        } else {
            0
        };
        self.subagents.last_mono_ms.saturating_add(extra)
    }

    pub(crate) fn subagent_header(&self) -> Option<SubagentHeader> {
        let info = self.current_subagent_session_info()?;
        let id = self.current_session_id()?;
        let state = &self.projection.subagents;
        let row = state.rows.get(id);
        let record = state.history.records.get(id);
        let status =
            record.and_then(|record| record.outcome).map_or_else(
                || {
                    if row.is_some() {
                        return ActivityStatus::Streaming;
                    }
                    self.activities
                        .back()
                        .map_or(ActivityStatus::Queued, |activity| activity.status)
                },
                |outcome| match outcome {
                    SubagentTerminalOutcome::Completed
                    | SubagentTerminalOutcome::StationarityEnded => ActivityStatus::Done,
                    SubagentTerminalOutcome::Cancelled
                    | SubagentTerminalOutcome::RemovedFromQueue => ActivityStatus::Error,
                    _ => ActivityStatus::Error,
                },
            );
        let cwd = record
            .and_then(|record| record.metadata.as_ref())
            .map(|metadata| metadata.context.effective_cwd.clone())
            .or_else(|| {
                self.events().find_map(|event| match &event.payload {
                    EventV1::RunStarted(data) => Some(data.workspace_root.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default();
        let cancelled_at = self.inspected_child_cancel_started();
        Some(SubagentHeader {
            label: row.map_or(info.label, |row| row.label.clone()),
            description: row.map_or(info.title, |row| row.display_description.clone()),
            model: row.map_or_else(
                || {
                    self.activities
                        .back()
                        .map(|activity| activity.model_id.clone())
                        .unwrap_or_default()
                },
                |row| row.model.clone(),
            ),
            cwd,
            status,
            elapsed_ms: row.map_or(0, |row| self.subagent_elapsed_ms(row)),
            cancelling: cancelled_at.is_some(),
            activity: if cancelled_at.is_some() {
                "Waiting".into()
            } else {
                self.activities.back().map_or_else(
                    || "Waiting for response…".into(),
                    |activity| self.child_activity(activity),
                )
            },
            phase_elapsed_ms: cancelled_at.map_or_else(
                || {
                    self.activities.back().map_or(0, |activity| {
                        self.subagent_presentation_ms()
                            .saturating_sub(self.live_turn_phase(activity).1)
                    })
                },
                |started| {
                    u64::try_from(self.now().saturating_duration_since(started).as_millis())
                        .unwrap_or(u64::MAX)
                },
            ),
            badge: row.map_or("", SubagentRow::context_badge),
        })
    }
}

pub(crate) fn duration_label(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 10 {
        format!("{}.{:01}s", seconds, (ms % 1000) / 100)
    } else if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    }
}
