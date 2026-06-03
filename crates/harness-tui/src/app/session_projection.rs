use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use harness_core::event::{
    ActorKind, BackgroundTaskNotificationEvent, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ResolvedToolIdentity, UserMessageSubmittedEvent,
};

use super::permissions::PendingPermission;
use super::{
    mark_activity_event, merge_orchestration_task_completion_metadata,
    merge_orchestration_task_event, merge_resolved_tool_identity, merge_tool_call_metadata,
    new_streaming_activity_entry, task_child_request_id_from_output,
    task_child_session_id_from_output, task_completed_updates_assistant_transcript,
    ActiveContextUsage, ActivityCacheUsage, ActivityEntry, ActivityStatus, ActivityUsage, AppState,
    CompactionState, CompactionStatus, CompactionUsageMetrics, Focus, MemoryCaps,
    NewStreamingActivityEntryArgs, OrchestrationOwnerLabels, OrchestrationSummary,
    OrchestrationTaskRow, OrchestrationTaskState, ToolCallDisplayStatus, ToolCallEntry,
    TOOL_OUTPUT_DISPLAY_MAX_CHARS,
};
use crate::text::non_empty_preserved_string;
use crate::view_model;

#[derive(Default)]
pub struct SessionProjection {
    pub(crate) events: Vec<EventEnvelopeV1>,
    pub(crate) activities: VecDeque<ActivityEntry>,
    pub(crate) active_context_usage: Option<ActiveContextUsage>,
    pub(crate) compaction_status: Option<CompactionStatus>,
    pub(crate) compaction_usage_metrics: CompactionUsageMetrics,
    pub(crate) memory_caps: MemoryCaps,
    pub(crate) events_trimmed_count: usize,
    pub(crate) transcript_trimmed_count: usize,
    orchestration_tasks: BTreeMap<String, OrchestrationTaskRow>,
    agent_profiles: BTreeMap<String, String>,
    child_agent_ids: BTreeSet<String>,
    child_request_agents: BTreeMap<String, String>,
    seen_seqs: BTreeSet<u64>,
    pub(crate) pending_permissions: BTreeMap<String, PendingPermission>,
    pub(crate) run_terminal_seen: bool,
}

impl SessionProjection {
    pub(crate) fn reset(&mut self) {
        self.events.clear();
        self.activities.clear();
        self.active_context_usage = None;
        self.compaction_status = None;
        self.compaction_usage_metrics = CompactionUsageMetrics::default();
        self.orchestration_tasks.clear();
        self.agent_profiles.clear();
        self.child_agent_ids.clear();
        self.child_request_agents.clear();
        self.seen_seqs.clear();
        self.pending_permissions.clear();
        self.run_terminal_seen = false;
        self.events_trimmed_count = 0;
        self.transcript_trimmed_count = 0;
    }

    pub(crate) fn has_seen_seq(&self, seq: u64) -> bool {
        self.seen_seqs.contains(&seq)
    }

    fn profile_label_for_event(&self, event: &EventEnvelopeV1) -> String {
        if let EventV1::BackgroundTaskNotification(data) = &event.payload {
            if let Some(profile) = data
                .parent_agent_id
                .as_deref()
                .and_then(|agent_id| self.agent_profiles.get(agent_id))
            {
                return profile.clone();
            }
        }

        event
            .actor
            .agent_id
            .as_deref()
            .and_then(|agent_id| self.agent_profiles.get(agent_id))
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    }

    pub(crate) fn ingest_event(&mut self, event: EventEnvelopeV1, historical: bool) -> usize {
        self.seen_seqs.insert(event.seq);
        self.update_derived_state_for_event(&event, historical);
        self.events.push(event);
        self.enforce_event_memory_cap()
    }

    fn find_tool_call_mut(&mut self, tool_call_id: &str) -> Option<&mut ToolCallEntry> {
        for activity in &mut self.activities {
            if let Some(tool_call) = activity
                .tool_calls
                .iter_mut()
                .find(|tc| tc.tool_call_id == tool_call_id)
            {
                return Some(tool_call);
            }
        }
        None
    }

    fn activity_index_for_request(&self, request_id: &str) -> Option<usize> {
        self.activities
            .iter()
            .position(|activity| activity.request_id == request_id)
    }

    fn local_prompt_echo_index_for_message(&self, text: &str) -> Option<usize> {
        self.activities.iter().rposition(|activity| {
            activity.request_id.is_empty()
                && activity
                    .user_message
                    .as_ref()
                    .is_some_and(|message| message.text == text)
        })
    }

    fn last_local_prompt_echo_index(&self) -> Option<usize> {
        self.activities
            .iter()
            .rposition(|activity| activity.request_id.is_empty())
    }

    fn adopt_local_prompt_echo_at(
        &mut self,
        index: usize,
        request_id: &str,
        seq: u64,
    ) -> Option<usize> {
        let entry = self.activities.get_mut(index)?;
        if !entry.request_id.is_empty() {
            return None;
        }

        entry.request_id = request_id.to_string();
        if entry.first_seq == 0 {
            entry.first_seq = seq;
        }
        entry.last_seq = seq;
        Some(index)
    }

    fn adopt_local_prompt_echo(&mut self, request_id: &str, seq: u64) -> Option<usize> {
        let index = self.last_local_prompt_echo_index()?;
        self.adopt_local_prompt_echo_at(index, request_id, seq)
    }

    fn activity_index_or_local_echo(&mut self, request_id: &str, seq: u64) -> Option<usize> {
        self.activity_index_for_request(request_id)
            .or_else(|| self.adopt_local_prompt_echo(request_id, seq))
    }

    fn canonical_provider_turn_id<'a>(
        event: &'a EventEnvelopeV1,
        provider_request_id: &'a str,
    ) -> &'a str {
        event
            .correlation_id
            .as_deref()
            .unwrap_or(provider_request_id)
    }

    fn note_child_agent_request(&mut self, event: &EventEnvelopeV1, request_id: &str) {
        let Some(agent_id) = event
            .actor
            .agent_id
            .as_deref()
            .or_else(|| event.stream_key.as_deref()?.strip_prefix("agent:"))
        else {
            return;
        };
        if self.child_agent_ids.contains(agent_id) {
            self.child_request_agents
                .insert(request_id.to_string(), agent_id.to_string());
        }
    }

    fn ensure_background_notification_activity(
        &mut self,
        event: &EventEnvelopeV1,
        data: &BackgroundTaskNotificationEvent,
    ) {
        let request_id = data
            .delivered_turn_request_id
            .as_deref()
            .unwrap_or(data.child_request_id.as_str());
        if self.activity_index_for_request(request_id).is_some() {
            return;
        }

        let text = background_task_notification_text(data);
        let status = if data.delivered_turn_request_id.is_some() {
            ActivityStatus::Queued
        } else {
            ActivityStatus::Done
        };

        self.activities.push_back(new_streaming_activity_entry(
            NewStreamingActivityEntryArgs {
                request_id: request_id.to_string(),
                profile_label: self.profile_label_for_event(event),
                model_id: String::new(),
                provider_id: String::new(),
                user_message: Some(UserMessageSubmittedEvent {
                    request_id: request_id.to_string(),
                    text,
                }),
                user_timestamp: event.ts.clone(),
                request_data: None,
                transcript_text: String::new(),
                first_seq: event.seq,
                first_mono_ms: event.mono_ms,
            },
        ));
        if let Some(entry) = self.activities.back_mut() {
            entry.status = status;
        }
    }

    pub(crate) fn delegated_child_request_ids_for_parent_view<'a>(
        &'a self,
        current_session_id: Option<&str>,
    ) -> BTreeSet<&'a str> {
        self.child_request_agents
            .iter()
            .filter_map(|(request_id, agent_id)| {
                (current_session_id != Some(agent_id.as_str())).then_some(request_id.as_str())
            })
            .collect()
    }

    fn activity_index_for_provider_event(
        &mut self,
        event: &EventEnvelopeV1,
        provider_request_id: &str,
    ) -> Option<usize> {
        let turn_id = Self::canonical_provider_turn_id(event, provider_request_id);
        self.activity_index_for_request(turn_id)
            .or_else(|| self.activity_index_for_request(provider_request_id))
            .or_else(|| self.adopt_local_prompt_echo(turn_id, event.seq))
    }

    fn activity_index_for_user_message(
        &mut self,
        data: &harness_core::event::UserMessageSubmittedEvent,
        seq: u64,
    ) -> Option<usize> {
        if let Some(index) = self.activity_index_for_request(&data.request_id) {
            self.remove_duplicate_local_prompt_echo(&data.text, index);
            return self.activity_index_for_request(&data.request_id);
        }

        self.local_prompt_echo_index_for_message(&data.text)
            .and_then(|index| self.adopt_local_prompt_echo_at(index, &data.request_id, seq))
            .or_else(|| self.adopt_local_prompt_echo(&data.request_id, seq))
    }

    fn remove_duplicate_local_prompt_echo(&mut self, text: &str, keep_index: usize) {
        let Some(index) = self.local_prompt_echo_index_for_message(text) else {
            return;
        };
        if index != keep_index {
            self.activities.remove(index);
        }
    }

    fn attach_permission_request(&mut self, event: &EventEnvelopeV1) {
        let EventV1::PermissionRequested(data) = &event.payload else {
            return;
        };

        let permission_entry = super::PermissionEntry {
            permission_id: data.permission_id.clone(),
            kind: data.kind.clone(),
            tool_call_id: data.tool_call_id.clone(),
            summary: data.summary.clone(),
            request_digest: data.request_digest.clone(),
            timeout_ms: data.timeout_ms,
            default_decision: data.default_decision,
            resolved_decision: None,
            resolution_reason: None,
            first_seq: event.seq,
            last_seq: event.seq,
        };

        if let Some(tool_call_id) = data.tool_call_id.as_deref() {
            if let Some(tool_entry) = self.find_tool_call_mut(tool_call_id) {
                tool_entry.permissions.push(permission_entry);
                tool_entry.sync_display_status();
                return;
            }
        }

        let found_by_correlation = event.correlation_id.as_deref().and_then(|request_id| {
            self.activities
                .iter()
                .position(|activity| activity.request_id == request_id)
        });

        if let Some(idx) = found_by_correlation {
            if let Some(activity) = self.activities.get_mut(idx) {
                activity.permissions.push(permission_entry);
                activity.last_seq = event.seq;
            }
        } else if let Some(activity) = self.activities.back_mut() {
            activity.permissions.push(permission_entry);
            activity.last_seq = event.seq;
        }
    }

    fn update_permission_resolution(
        &mut self,
        permission_id: &str,
        decision: harness_core::event::PermissionDecision,
        reason: Option<&str>,
        seq: u64,
    ) {
        for activity in &mut self.activities {
            for permission in &mut activity.permissions {
                if permission.permission_id == permission_id {
                    permission.mark_resolved(decision, reason, seq);
                    activity.last_seq = seq;
                    return;
                }
            }

            for tool_call in &mut activity.tool_calls {
                for permission in &mut tool_call.permissions {
                    if permission.permission_id == permission_id {
                        permission.mark_resolved(decision, reason, seq);
                        tool_call.sync_display_status();
                        tool_call.last_seq = seq;
                        activity.last_seq = seq;
                        return;
                    }
                }
            }
        }
    }

    fn orchestration_task_row_mut(
        &mut self,
        event: &EventEnvelopeV1,
        task_id: &str,
    ) -> &mut OrchestrationTaskRow {
        let row = self
            .orchestration_tasks
            .entry(task_id.to_string())
            .or_insert_with(|| OrchestrationTaskRow {
                task_id: task_id.to_string(),
                queue_key: None,
                state: OrchestrationTaskState::Queued,
                warning: None,
                owner_kind: event.actor.kind,
                owner_agent_id: event.actor.agent_id.clone(),
                request_id: event.correlation_id.clone(),
                parent_tool_call_id: None,
                parent_request_id: None,
                child_session_id: event.actor.agent_id.clone(),
                child_request_id: event.correlation_id.clone(),
                result_summary: None,
                child_tool_call_count: 0,
                current_child_tool_title: None,
                timing_elapsed_ms: None,
                first_seq: event.seq,
                last_seq: event.seq,
                first_mono_ms: event.mono_ms,
                last_mono_ms: event.mono_ms,
                first_timestamp: event.ts.clone(),
                last_timestamp: event.ts.clone(),
            });

        row.owner_kind = event.actor.kind;
        if let Some(agent_id) = event.actor.agent_id.as_ref() {
            row.owner_agent_id = Some(agent_id.clone());
        }
        row
    }

    fn update_orchestration_task<F>(&mut self, event: &EventEnvelopeV1, task_id: &str, update: F)
    where
        F: FnOnce(&mut OrchestrationTaskRow),
    {
        {
            let row = self.orchestration_task_row_mut(event, task_id);
            merge_orchestration_task_event(row, event);
            update(row);
        }
        self.enforce_orchestration_retention();
    }

    fn note_child_task_tool_call(
        &mut self,
        event: &EventEnvelopeV1,
        data: &harness_core::event::ToolCallRequestedEvent,
    ) {
        let Some(request_id) = event.correlation_id.as_deref() else {
            return;
        };

        for row in self.orchestration_tasks.values_mut() {
            if row.effective_child_request_id() == Some(request_id) {
                row.child_tool_call_count = row.child_tool_call_count.saturating_add(1);
                row.current_child_tool_title = Self::child_tool_call_title(data);
                row.last_seq = event.seq;
                row.last_mono_ms = event.mono_ms;
                row.last_timestamp = event.ts.clone();
            }
        }
    }

    fn child_tool_call_title(data: &harness_core::event::ToolCallRequestedEvent) -> Option<String> {
        let args = serde_json::from_str::<serde_json::Value>(&data.args_summary).ok();
        let arg = |keys: &[&str]| {
            keys.iter().find_map(|key| {
                args.as_ref()
                    .and_then(|value| value.get(*key))
                    .and_then(serde_json::Value::as_str)
                    .and_then(non_empty_preserved_string)
            })
        };
        let title = match data.tool_id.as_str() {
            "fs.read" | "read" => format!("Read {}", arg(&["filePath", "path"])?),
            "fs.grep" | "grep" => format!("Grep \"{}\"", arg(&["pattern", "query"])?),
            "fs.glob" | "glob" => format!("Glob \"{}\"", arg(&["pattern"])?),
            "fs.ls" | "list" => {
                format!("List {}", arg(&["path"]).unwrap_or_else(|| ".".to_string()))
            }
            "shell.run" | "bash" => {
                arg(&["description", "command", "cmd"]).map(|value| format!("Shell {value}"))?
            }
            "edit.hashline_apply" | "edit" => format!("Edit {}", arg(&["filePath", "path"])?),
            "fs.write" | "write" => format!("Write {}", arg(&["filePath", "path"])?),
            other => titlecase_word(other),
        };
        Some(title)
    }

    fn has_active_turn_task_for_request(&self, request_id: &str) -> bool {
        self.orchestration_tasks.values().any(|row| {
            !row.state.is_terminal()
                && Self::task_row_is_turn_level(row)
                && (row.effective_child_request_id() == Some(request_id)
                    || row.request_id.as_deref() == Some(request_id))
        })
    }

    pub(super) fn active_turn_task_ids(&self) -> Vec<&str> {
        self.orchestration_tasks
            .values()
            .filter(|row| !row.state.is_terminal() && Self::task_row_is_turn_level(row))
            .map(|row| row.task_id.as_str())
            .collect()
    }

    pub(super) fn active_turn_task_id(&self) -> Option<&str> {
        self.orchestration_tasks
            .values()
            .filter(|row| !row.state.is_terminal() && Self::task_row_is_turn_level(row))
            .max_by_key(|row| (row.last_seq, row.first_seq))
            .map(|row| row.task_id.as_str())
    }

    fn task_row_is_turn_level(row: &OrchestrationTaskRow) -> bool {
        row.queue_key
            .as_deref()
            .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
    }

    fn task_scope_is_turn_level(scope: harness_core::event::TaskTerminalScope) -> bool {
        matches!(scope, harness_core::event::TaskTerminalScope::AgentTurn)
    }

    fn is_turn_level_task_completion(
        &self,
        task_id: &str,
        data: &harness_core::event::TaskCompletedEvent,
    ) -> bool {
        self.orchestration_tasks
            .get(task_id)
            .map(Self::task_row_is_turn_level)
            .or_else(|| {
                data.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope)
                    .map(Self::task_scope_is_turn_level)
            })
            .unwrap_or_else(|| task_completed_updates_assistant_transcript(data))
    }

    fn is_turn_level_task_cancellation(
        &self,
        task_id: &str,
        data: &harness_core::event::TaskCancelledEvent,
    ) -> bool {
        self.orchestration_tasks
            .get(task_id)
            .map(Self::task_row_is_turn_level)
            .or_else(|| data.task_scope.map(Self::task_scope_is_turn_level))
            .unwrap_or(false)
    }

    pub(crate) fn transcript_task_row_for_tool_call(
        &self,
        tool_call: &ToolCallEntry,
    ) -> Option<OrchestrationTaskRow> {
        let child_request_id = tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.clone())
            .or_else(|| task_child_request_id_from_output(tool_call.output_json.as_ref()));
        let child_session_id = tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.clone())
            .or_else(|| task_child_session_id_from_output(tool_call.output_json.as_ref()));

        self.orchestration_tasks
            .values()
            .filter_map(|row| {
                let mut score = 0u8;
                if row.parent_tool_call_id.as_deref() == Some(tool_call.tool_call_id.as_str()) {
                    score += 8;
                }
                if child_request_id
                    .as_deref()
                    .is_some_and(|request_id| row.effective_child_request_id() == Some(request_id))
                {
                    score += 4;
                }
                if child_session_id
                    .as_deref()
                    .is_some_and(|session_id| row.effective_child_session_id() == Some(session_id))
                {
                    score += 2;
                }
                (score > 0).then_some((score, !row.state.is_terminal(), row.last_seq, row.clone()))
            })
            .max_by_key(|(score, active, last_seq, _)| (*score, *active, *last_seq))
            .map(|(_, _, _, row)| row)
    }

    fn enforce_orchestration_retention(&mut self) {
        let mut terminal_rows = self
            .orchestration_tasks
            .iter()
            .filter(|(_, row)| row.state.is_terminal())
            .map(|(task_id, row)| (task_id.clone(), row.last_seq))
            .collect::<Vec<_>>();

        if terminal_rows.len() <= 5 {
            return;
        }

        terminal_rows.sort_by_key(|(task_id, last_seq)| (Reverse(*last_seq), task_id.clone()));
        for (task_id, _) in terminal_rows.into_iter().skip(5) {
            self.orchestration_tasks.remove(&task_id);
        }
    }

    pub fn orchestration_summary(&self) -> OrchestrationSummary {
        let mut summary = OrchestrationSummary::default();
        let mut active_agents = BTreeSet::new();

        for row in self.orchestration_tasks.values() {
            if row.state.is_terminal() {
                continue;
            }

            if row.owner_kind == ActorKind::Worker {
                if let Some(agent_id) = row.owner_agent_id.as_deref() {
                    active_agents.insert(agent_id);
                }
            }

            match row.state {
                OrchestrationTaskState::Queued => summary.queued += 1,
                OrchestrationTaskState::Running => summary.running += 1,
                OrchestrationTaskState::Stale => summary.stale += 1,
                OrchestrationTaskState::Completed
                | OrchestrationTaskState::Cancelled
                | OrchestrationTaskState::Failed
                | OrchestrationTaskState::TimedOut
                | OrchestrationTaskState::LateResult => {}
            }
        }

        summary.active_agents = active_agents.len();
        summary
    }

    pub fn orchestration_latest_warning(&self) -> Option<&str> {
        self.orchestration_tasks
            .values()
            .filter_map(|row| {
                row.warning
                    .as_ref()
                    .map(|warning| (row.last_seq, warning.as_str()))
            })
            .max_by_key(|(last_seq, _)| *last_seq)
            .map(|(_, warning)| warning)
    }

    pub fn orchestration_visible_rows(&self) -> Vec<OrchestrationTaskRow> {
        let mut rows = self
            .orchestration_tasks
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| {
            (
                row.state.is_terminal(),
                row.state.sort_rank(),
                Reverse(row.last_seq),
                row.task_id.clone(),
            )
        });
        rows
    }

    pub fn orchestration_owner_labels(
        &self,
        row: &OrchestrationTaskRow,
    ) -> OrchestrationOwnerLabels {
        match row.owner_kind {
            ActorKind::Worker => {
                let label = row
                    .owner_agent_id
                    .clone()
                    .unwrap_or_else(|| "worker".to_string());
                let profile = self
                    .agent_profiles
                    .get(label.as_str())
                    .cloned()
                    .unwrap_or_else(|| "n/a".to_string());
                OrchestrationOwnerLabels { label, profile }
            }
            ActorKind::Supervisor => OrchestrationOwnerLabels {
                label: "supervisor".to_string(),
                profile: "n/a".to_string(),
            },
            ActorKind::System | ActorKind::User => OrchestrationOwnerLabels {
                label: "system".to_string(),
                profile: "n/a".to_string(),
            },
        }
    }

    fn update_derived_state_for_event(&mut self, event: &EventEnvelopeV1, historical: bool) {
        match &event.payload {
            EventV1::PermissionRequested(data) => {
                self.pending_permissions.insert(
                    data.permission_id.clone(),
                    PendingPermission {
                        seq: event.seq,
                        kind: data.kind.clone(),
                        summary: data.summary.clone(),
                        request_digest: data.request_digest.clone(),
                        timeout_ms: data.timeout_ms,
                        default_decision: data.default_decision,
                        tool_call_id: data.tool_call_id.clone(),
                    },
                );
                self.attach_permission_request(event);
            }
            EventV1::PermissionResolved(data) => {
                self.pending_permissions.remove(&data.permission_id);
                self.update_permission_resolution(
                    &data.permission_id,
                    data.decision,
                    data.reason.as_deref(),
                    event.seq,
                );
            }
            EventV1::RunFinished(_) => {
                if !historical {
                    self.run_terminal_seen = true;
                }
            }
            EventV1::RunFailed(data) => {
                if !historical {
                    self.run_terminal_seen = true;
                }
                if let Some(entry) = self.activities.back_mut() {
                    entry.status = ActivityStatus::Error;
                    entry.error_message = Some(data.error.clone());
                }
            }
            EventV1::AgentSpawned(data) => {
                self.agent_profiles
                    .insert(data.agent_id.clone(), data.profile.clone());
                if data.parent_agent_id.is_some() {
                    self.child_agent_ids.insert(data.agent_id.clone());
                }
            }
            EventV1::UserMessageSubmitted(data) => {
                self.note_child_agent_request(event, &data.request_id);
                if let Some(index) = self.activity_index_for_user_message(data, event.seq) {
                    let status = if self.activities.iter().any(|activity| {
                        activity.status == ActivityStatus::Streaming
                            && activity.request_id != data.request_id
                    }) {
                        ActivityStatus::Queued
                    } else {
                        ActivityStatus::Streaming
                    };
                    if let Some(entry) = self.activities.get_mut(index) {
                        if !matches!(entry.status, ActivityStatus::Done | ActivityStatus::Error)
                            && !activity_is_background_notification_reminder(entry)
                        {
                            entry.status = status;
                        }
                        entry.user_message = Some(data.clone());
                        entry.user_timestamp = event.ts.clone();
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    let status = if self
                        .activities
                        .iter()
                        .any(|activity| activity.status == ActivityStatus::Streaming)
                    {
                        ActivityStatus::Queued
                    } else {
                        ActivityStatus::Streaming
                    };
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
                            profile_label: self.profile_label_for_event(event),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: Some(data.clone()),
                            user_timestamp: event.ts.clone(),
                            request_data: None,
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                    if let Some(entry) = self.activities.back_mut() {
                        entry.status = status;
                    }
                }
            }
            EventV1::ProviderRequestStarted(data) => {
                self.note_child_agent_request(event, &data.request_id);
                let turn_id = Self::canonical_provider_turn_id(event, &data.request_id);
                self.note_child_agent_request(event, turn_id);
                for row in self.orchestration_tasks.values_mut() {
                    if row.effective_child_request_id() == Some(turn_id)
                        || row.effective_child_request_id() == Some(data.request_id.as_str())
                    {
                        if row.result_summary.is_none() {
                            row.result_summary = non_empty_preserved_string(&data.prompt_summary);
                        }
                        row.last_seq = event.seq;
                        row.last_mono_ms = event.mono_ms;
                        row.last_timestamp = event.ts.clone();
                    }
                }
                if let Some(index) = self.activity_index_for_provider_event(event, &data.request_id)
                {
                    let profile_label = self.profile_label_for_event(event);
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        if entry.profile_label.is_empty() {
                            entry.profile_label = profile_label;
                        }
                        entry.model_id = data.model_id.clone();
                        entry.provider_id = data.provider_id.clone();
                        entry.request_data = Some(data.clone());
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: turn_id.to_string(),
                            profile_label: self.profile_label_for_event(event),
                            model_id: data.model_id.clone(),
                            provider_id: data.provider_id.clone(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: Some(data.clone()),
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                }
            }
            EventV1::ProviderStreamDelta(data) => {
                self.note_child_agent_request(event, &data.request_id);
                let turn_id = Self::canonical_provider_turn_id(event, &data.request_id);
                self.note_child_agent_request(event, turn_id);
                if let Some(index) = self.activity_index_for_provider_event(event, &data.request_id)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.transcript_text.push_str(&data.delta);
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: turn_id.to_string(),
                            profile_label: self.profile_label_for_event(event),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: None,
                            transcript_text: data.delta.clone(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                }
                self.enforce_transcript_memory_cap();
            }
            EventV1::ProviderReasoningDelta(data) => {
                self.note_child_agent_request(event, &data.request_id);
                let turn_id = Self::canonical_provider_turn_id(event, &data.request_id);
                self.note_child_agent_request(event, turn_id);
                if let Some(index) = self.activity_index_for_provider_event(event, &data.request_id)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.thinking_text.push_str(&data.delta);
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: turn_id.to_string(),
                            profile_label: self.profile_label_for_event(event),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: None,
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                    if let Some(entry) = self.activities.back_mut() {
                        entry.thinking_text = data.delta.clone();
                    }
                }
                self.enforce_transcript_memory_cap();
            }
            EventV1::ProviderRequestFinished(data) => {
                self.note_child_agent_request(event, &data.request_id);
                let turn_id = Self::canonical_provider_turn_id(event, &data.request_id);
                self.note_child_agent_request(event, turn_id);
                let provider_error_detail = provider_error_detail(data);
                if let Some(index) = self.activity_index_for_provider_event(event, &data.request_id)
                {
                    let should_mark_done = !self.has_active_turn_task_for_request(turn_id);
                    if let Some(entry) = self.activities.get_mut(index) {
                        if entry.tool_calls.is_empty()
                            && entry.transcript_text.is_empty()
                            && !entry.thinking_text.is_empty()
                        {
                            entry.transcript_text = std::mem::take(&mut entry.thinking_text);
                        }
                        if let Some(error_detail) = provider_error_detail {
                            entry.status = ActivityStatus::Error;
                            entry.error_message = Some(error_detail);
                        } else if should_mark_done {
                            entry.status = ActivityStatus::Done;
                        }
                        entry.usage = data.usage.as_ref().map(|usage| ActivityUsage {
                            prompt_tokens: usage.prompt_tokens,
                            completion_tokens: usage.completion_tokens,
                            total_tokens: usage.total_tokens,
                        });
                        entry.cache_usage = data.metadata.as_ref().and_then(|metadata| {
                            match (metadata.cache_read_tokens, metadata.cache_write_tokens) {
                                (None, None) => None,
                                (read_tokens, write_tokens) => Some(ActivityCacheUsage {
                                    read_tokens: read_tokens.unwrap_or(0),
                                    write_tokens: write_tokens.unwrap_or(0),
                                }),
                            }
                        });
                        if let Some(usage) = data.usage.as_ref() {
                            self.active_context_usage = Some(ActiveContextUsage::estimate(
                                usage.prompt_tokens.saturating_add(usage.completion_tokens),
                            ));
                        }
                        entry.last_seq = event.seq;
                        entry.last_mono_ms = event.mono_ms;
                    }
                }
            }
            EventV1::CompactionApplied(data) => {
                self.active_context_usage = Some(
                    data.tokens_after_estimate
                        .map(ActiveContextUsage::estimate)
                        .unwrap_or_else(ActiveContextUsage::compacted_pending_refresh),
                );
                self.compaction_usage_metrics.completed_count = self
                    .compaction_usage_metrics
                    .completed_count
                    .saturating_add(1);
                self.compaction_usage_metrics.summary_tokens_estimate = self
                    .compaction_usage_metrics
                    .summary_tokens_estimate
                    .saturating_add(u64::from(data.summary_tokens_estimate.unwrap_or(0)));
                self.compaction_usage_metrics.reduction_tokens_estimate = self
                    .compaction_usage_metrics
                    .reduction_tokens_estimate
                    .saturating_add(u64::from(data.reduction_tokens_estimate.unwrap_or(0)));
                self.compaction_usage_metrics.last_tokens_before_estimate =
                    data.tokens_before_estimate;
                self.compaction_usage_metrics.last_tokens_after_estimate =
                    data.tokens_after_estimate;
                self.compaction_usage_metrics
                    .last_reduction_percent_estimate = data.reduction_percent_estimate;
                self.compaction_status = Some(CompactionStatus {
                    agent_id: data.agent_id.clone(),
                    checkpoint_id: Some(data.checkpoint_id.clone()),
                    trigger_reason: "applied".to_string(),
                    state: CompactionState::Applied,
                    message: data
                        .tokens_after_estimate
                        .map(|tokens| format!("compaction applied · active ctx ~{tokens}"))
                        .unwrap_or_else(|| "compaction applied · refresh pending".to_string()),
                });
            }
            EventV1::CompactionRequested(data) => {
                self.compaction_status = Some(CompactionStatus {
                    agent_id: data.agent_id.clone(),
                    checkpoint_id: Some(data.checkpoint_id.clone()),
                    trigger_reason: data.trigger_reason.clone(),
                    state: CompactionState::Requested,
                    message: format!("compaction requested · {}", data.trigger_reason),
                });
            }
            EventV1::CompactionWritten(data) => {
                let source_label = data.summary_source.as_ref().and_then(|source| {
                    source
                        .deterministic_fallback
                        .then_some(" · deterministic fallback")
                });
                self.compaction_status = Some(CompactionStatus {
                    agent_id: data.agent_id.clone(),
                    checkpoint_id: Some(data.checkpoint_id.clone()),
                    trigger_reason: data.trigger_reason.clone(),
                    state: CompactionState::Written,
                    message: format!(
                        "compaction checkpoint written{} · {} bytes",
                        source_label.unwrap_or_default(),
                        data.artifact_bytes,
                    ),
                });
            }
            EventV1::CompactionFailed(data) => {
                self.compaction_status = Some(CompactionStatus {
                    agent_id: data.agent_id.clone(),
                    checkpoint_id: data.checkpoint_id.clone(),
                    trigger_reason: data.trigger_reason.clone(),
                    state: CompactionState::Failed,
                    message: format!("compaction failed · {}", data.reason),
                });
            }
            EventV1::TaskCompleted(data) => {
                let should_mark_done = self.is_turn_level_task_completion(&data.task_id, data);
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Completed;
                    row.warning = None;
                    row.result_summary = Some(data.result_summary.clone());
                    merge_orchestration_task_completion_metadata(row, data.metadata.as_ref());
                });

                if let Some(request_id) = event.correlation_id.as_deref() {
                    if let Some(index) = self.activity_index_or_local_echo(request_id, event.seq) {
                        if let Some(entry) = self.activities.get_mut(index) {
                            if should_mark_done {
                                entry.status = ActivityStatus::Done;
                            }
                            if should_mark_done && entry.transcript_text.is_empty() {
                                if let Some(result_summary) =
                                    non_empty_preserved_string(&data.result_summary)
                                {
                                    entry.transcript_text = result_summary;
                                }
                            }
                            entry.last_seq = event.seq;
                        }
                    }
                }
            }
            EventV1::TaskScheduled(data) => {
                if let Some(request_id) = event.correlation_id.as_deref() {
                    self.note_child_agent_request(event, request_id);
                }
                if data.state == harness_core::event::TaskScheduleState::Queued {
                    if let Some(request_id) = event.correlation_id.as_deref() {
                        if let Some(index) =
                            self.activity_index_or_local_echo(request_id, event.seq)
                        {
                            if let Some(entry) = self.activities.get_mut(index) {
                                if !matches!(
                                    entry.status,
                                    ActivityStatus::Done | ActivityStatus::Error
                                ) {
                                    entry.status = ActivityStatus::Queued;
                                    mark_activity_event(entry, event.seq, event.mono_ms);
                                }
                            }
                        }
                    }
                }
                self.update_orchestration_task(event, &data.task_id, |row| {
                    if let Some(queue_key) = data.queue_key.as_ref() {
                        row.queue_key = Some(queue_key.clone());
                    }
                    row.warning = None;
                    if row.child_request_id.is_none() {
                        row.child_request_id = event.correlation_id.clone();
                    }
                    row.state = match data.state {
                        harness_core::event::TaskScheduleState::Queued => {
                            OrchestrationTaskState::Queued
                        }
                        harness_core::event::TaskScheduleState::Started => {
                            OrchestrationTaskState::Running
                        }
                    };
                });
            }
            EventV1::TaskCancelled(data) => {
                let should_mark_error = self.is_turn_level_task_cancellation(&data.task_id, data);
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Cancelled;
                    row.warning = non_empty_preserved_string(&data.reason);
                });

                if should_mark_error {
                    if let Some(request_id) = event.correlation_id.as_deref() {
                        if let Some(index) = self.activity_index_for_request(request_id) {
                            if let Some(entry) = self.activities.get_mut(index) {
                                entry.status = ActivityStatus::Error;
                                let reason = non_empty_preserved_string(&data.reason);
                                entry.error_message = match (reason, entry.error_message.take()) {
                                    (Some(reason), Some(existing))
                                        if !existing.contains(&reason) =>
                                    {
                                        Some(format!("{reason} · {existing}"))
                                    }
                                    (Some(reason), _) => Some(reason),
                                    (None, existing) => existing,
                                };
                                mark_activity_event(entry, event.seq, event.mono_ms);
                            }
                        }
                    }
                }
            }
            EventV1::TaskResultLate(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::LateResult;
                    row.warning = Some("late result after stale cancellation".to_string());
                });
            }
            EventV1::BackgroundTaskNotification(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.child_session_id = Some(data.child_session_id.clone());
                    row.child_request_id = Some(data.child_request_id.clone());
                    row.result_summary = non_empty_preserved_string(&data.summary);
                    row.state = match data.status {
                        harness_core::event::BackgroundTaskNotificationStatus::Completed => {
                            OrchestrationTaskState::Completed
                        }
                        harness_core::event::BackgroundTaskNotificationStatus::Cancelled => {
                            OrchestrationTaskState::Cancelled
                        }
                        harness_core::event::BackgroundTaskNotificationStatus::Failed => {
                            OrchestrationTaskState::Failed
                        }
                        harness_core::event::BackgroundTaskNotificationStatus::TimedOut => {
                            OrchestrationTaskState::TimedOut
                        }
                    };
                    row.warning = match data.status {
                        harness_core::event::BackgroundTaskNotificationStatus::Completed => None,
                        _ => Some(data.status.as_str().replace('_', " ")),
                    };
                });
                self.ensure_background_notification_activity(event, data);
            }
            EventV1::TeamCreated(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.queue_key = Some("team".to_string());
                    row.result_summary = Some(format!(
                        "team {} · {} member(s)",
                        data.spec.name,
                        data.spec.members.len()
                    ));
                    row.warning = None;
                });
            }
            EventV1::TeamMemberSpawned(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary =
                        Some(format!("{} active as {}", data.member_name, data.profile));
                    row.warning = None;
                });
            }
            EventV1::TeamMessageSent(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary = data
                        .message
                        .summary
                        .clone()
                        .or_else(|| Some(format!("message {}", data.message.message_id)));
                    row.warning = None;
                });
            }
            EventV1::TeamTaskCreated(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary = Some(format!("task created · {}", data.task.task_id));
                    row.warning = None;
                });
            }
            EventV1::TeamTaskUpdated(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary =
                        Some(format!("task {} · {}", data.task_id, data.status.as_str()));
                    row.warning = None;
                });
            }
            EventV1::TeamShutdownRequested(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary = Some(format!("shutdown requested · {}", data.member_name));
                    row.warning = None;
                });
            }
            EventV1::TeamShutdownApproved(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary = Some(format!("shutdown approved · {}", data.member_name));
                    row.warning = None;
                });
            }
            EventV1::TeamShutdownRejected(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Running;
                    row.result_summary = Some(format!("shutdown rejected · {}", data.member_name));
                    row.warning = Some(data.reason.clone());
                });
            }
            EventV1::TeamDeleted(data) => {
                let row_id = format!("team:{}", data.team_run_id);
                self.update_orchestration_task(event, &row_id, |row| {
                    row.state = OrchestrationTaskState::Completed;
                    row.result_summary = Some("team deleted".to_string());
                    row.warning = None;
                });
            }
            EventV1::StaleDetected(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Stale;
                    row.warning = Some(format!("stale for {} ms", data.stale_for_ms));
                });
            }
            EventV1::ToolCallRequested(data) => {
                let target_corr_id = event.correlation_id.clone();
                if let Some(request_id) = target_corr_id.as_deref() {
                    self.note_child_agent_request(event, request_id);
                }
                let use_back = self
                    .activities
                    .back()
                    .is_none_or(|entry| target_corr_id.is_none() || entry.request_id.is_empty());

                let entry = if use_back {
                    self.activities.back_mut()
                } else if let Some(corr) = &target_corr_id {
                    self.activities
                        .iter_mut()
                        .find(|activity| &activity.request_id == corr)
                } else {
                    None
                };

                if let Some(entry) = entry {
                    if entry.tool_calls.is_empty()
                        && entry.thinking_text.is_empty()
                        && !entry.transcript_text.is_empty()
                    {
                        entry.thinking_text = std::mem::take(&mut entry.transcript_text);
                    }
                    let tool_entry = ToolCallEntry {
                        tool_call_id: data.tool_call_id.clone(),
                        tool_id: data.tool_id.clone(),
                        canonical_tool_id: None,
                        alias_source_tool_id: None,
                        resolved_tool_identity: None,
                        args_summary: data.args_summary.clone(),
                        args_digest: data.args_digest.clone(),
                        lifecycle_state: Some(harness_core::event::ToolCallLifecycleState::Pending),
                        status: ToolCallDisplayStatus::Queued,
                        output_summary: None,
                        output_digest: None,
                        output_json: None,
                        truncated_output: None,
                        edit: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing_elapsed_ms: None,
                        permissions: Vec::new(),
                        first_seq: event.seq,
                        last_seq: event.seq,
                        first_mono_ms: event.mono_ms,
                        last_mono_ms: event.mono_ms,
                        first_timestamp: event.ts.clone(),
                        last_timestamp: event.ts.clone(),
                    };
                    let mut tool_entry = tool_entry;
                    merge_resolved_tool_identity(
                        &mut tool_entry,
                        ResolvedToolIdentity::from_tool_call(
                            Some(data.tool_id.as_str()),
                            data.metadata.as_ref(),
                        ),
                    );
                    merge_tool_call_metadata(&mut tool_entry, data.metadata.as_ref());
                    tool_entry.sync_display_status();
                    entry.tool_calls.push(tool_entry);
                    entry.last_seq = event.seq;
                }
                self.note_child_task_tool_call(event, data);
            }
            EventV1::ToolCallStarted(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(&data.tool_call_id) {
                    tool_entry.lifecycle_state =
                        Some(harness_core::event::ToolCallLifecycleState::Running);
                    tool_entry.sync_display_status();
                    tool_entry.last_seq = event.seq;
                    tool_entry.last_mono_ms = event.mono_ms;
                    tool_entry.last_timestamp = event.ts.clone();
                }
            }
            EventV1::ToolCallFinished(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(&data.tool_call_id) {
                    tool_entry.lifecycle_state = Some(
                        harness_core::event::ToolCallLifecycleState::from_finish_status(
                            data.status,
                        ),
                    );
                    tool_entry.output_summary = data.output_summary.clone();
                    tool_entry.output_digest = data.output_digest.clone();
                    tool_entry.output_json = data.output_json.clone();
                    if let Some(summary) = &data.output_summary {
                        let display_text =
                            if summary.chars().count() > TOOL_OUTPUT_DISPLAY_MAX_CHARS {
                                let truncated: String = summary
                                    .chars()
                                    .take(TOOL_OUTPUT_DISPLAY_MAX_CHARS)
                                    .collect();
                                format!("{}…", truncated)
                            } else {
                                summary.clone()
                            };
                        tool_entry.truncated_output = Some(display_text);
                    }
                    merge_resolved_tool_identity(
                        tool_entry,
                        ResolvedToolIdentity::from_tool_call(
                            Some(tool_entry.tool_id.as_str()),
                            data.metadata.as_ref(),
                        ),
                    );
                    merge_tool_call_metadata(tool_entry, data.metadata.as_ref());
                    tool_entry.sync_display_status();
                    tool_entry.last_seq = event.seq;
                    tool_entry.last_mono_ms = event.mono_ms;
                    tool_entry.last_timestamp = event.ts.clone();
                }
            }
            EventV1::EditProposed(data) => {
                if let Some(tool_entry) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|tool_call_id| self.find_tool_call_mut(tool_call_id))
                {
                    tool_entry.edit = Some(super::EditEntry {
                        edit_id: data.edit_id.clone(),
                        path: data.path.clone(),
                        status: super::EditDisplayStatus::Proposed,
                        summary: Some(data.summary.clone()),
                        patch_digest: Some(data.patch_digest.clone()),
                        new_file_digest: None,
                        diff_rel_path: None,
                        diff_digest: None,
                        rejection_reason: None,
                    });
                    tool_entry.last_seq = event.seq;
                }
            }
            EventV1::EditApplied(data) => {
                if let Some(tool_entry) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|tool_call_id| self.find_tool_call_mut(tool_call_id))
                {
                    let summary = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.summary.clone());
                    let patch_digest = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.patch_digest.clone());
                    tool_entry.edit = Some(super::EditEntry {
                        edit_id: data.edit_id.clone(),
                        path: data.path.clone(),
                        status: super::EditDisplayStatus::Applied,
                        summary,
                        patch_digest,
                        new_file_digest: Some(data.new_file_digest.clone()),
                        diff_rel_path: data.diff_rel_path.clone(),
                        diff_digest: data.diff_digest.clone(),
                        rejection_reason: None,
                    });
                    tool_entry.last_seq = event.seq;
                }
            }
            EventV1::EditRejected(data) => {
                if let Some(tool_entry) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|tool_call_id| self.find_tool_call_mut(tool_call_id))
                {
                    let summary = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.summary.clone());
                    let patch_digest = tool_entry
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.patch_digest.clone());
                    tool_entry.edit = Some(super::EditEntry {
                        edit_id: data.edit_id.clone(),
                        path: data.path.clone(),
                        status: super::EditDisplayStatus::Rejected,
                        summary,
                        patch_digest,
                        new_file_digest: None,
                        diff_rel_path: None,
                        diff_digest: None,
                        rejection_reason: Some(data.reason.clone()),
                    });
                    tool_entry.last_seq = event.seq;
                }
            }
            _ => {}
        }
    }

    fn enforce_event_memory_cap(&mut self) -> usize {
        let max_events = self.memory_caps.max_events;
        if self.events.len() > max_events {
            let to_remove = self.events.len() - max_events;
            self.events.drain(0..to_remove);
            self.events_trimmed_count += to_remove;
            to_remove
        } else {
            0
        }
    }

    fn enforce_transcript_memory_cap(&mut self) {
        let max_chars = self.memory_caps.max_transcript_chars;
        let total_chars: usize = self
            .activities
            .iter()
            .map(|activity| activity.thinking_text.len() + activity.transcript_text.len())
            .sum();
        if total_chars > max_chars {
            let excess = total_chars - max_chars;
            let mut trimmed = 0;
            while trimmed < excess && !self.activities.is_empty() {
                if let Some(first) = self.activities.front_mut() {
                    for chunk in [&mut first.thinking_text, &mut first.transcript_text] {
                        if trimmed >= excess {
                            break;
                        }
                        if chunk.len() <= excess - trimmed {
                            trimmed += chunk.len();
                            chunk.clear();
                        } else {
                            let to_trim = excess - trimmed;
                            *chunk = chunk.split_off(to_trim);
                            trimmed = excess;
                        }
                    }
                    if trimmed >= excess {
                        break;
                    }
                }
                if trimmed < excess {
                    self.activities.pop_front();
                }
            }
            self.transcript_trimmed_count += trimmed;
        }
    }
}

fn provider_error_detail(data: &ProviderRequestFinishedEvent) -> Option<String> {
    if !data.finish_reason.eq_ignore_ascii_case("error") {
        return None;
    }

    let metadata = data.metadata.as_ref()?;
    let category = metadata.provider_error_category?;
    let remediation = metadata
        .provider_error_remediation
        .as_deref()
        .and_then(non_empty_preserved_string);

    Some(match remediation {
        Some(remediation) => format!("{} · {remediation}", category.as_str()),
        None => category.as_str().to_string(),
    })
}

fn titlecase_word(value: &str) -> String {
    let mut label = String::with_capacity(value.len());
    let mut previous_was_word = false;
    for ch in value.chars() {
        let is_word = ch.is_ascii_alphanumeric() || ch == '_';
        if is_word && !previous_was_word {
            label.extend(ch.to_uppercase());
        } else {
            label.push(ch);
        }
        previous_was_word = is_word;
    }
    label
}

fn background_task_notification_text(data: &BackgroundTaskNotificationEvent) -> String {
    let status = data.status.as_str();
    let task_id = background_notification_safe_field(&data.task_id);
    let child_request_id = background_notification_safe_field(&data.child_request_id);
    let child_session_id = background_notification_safe_field(&data.child_session_id);

    format!(
        "<system-reminder>\n[BACKGROUND TASK {}]\nID: {}\nRequest ID: {}\nStatus: {}\n\nBackground task {}. Use background_output(request_id=\"{}\") for full details or task(session_id=\"{}\") to continue analysis from the child session.\n</system-reminder>",
        status.to_ascii_uppercase(),
        task_id,
        child_request_id,
        status,
        status.replace('_', " "),
        child_request_id,
        child_session_id,
    )
}

fn background_notification_safe_field(value: &str) -> String {
    const MAX_CHARS: usize = 120;

    let mut sanitized = String::new();
    for character in value.chars() {
        sanitized.push(if character.is_control() || character == '\t' {
            ' '
        } else {
            character
        });
    }

    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }

    let mut capped = String::new();
    for (index, character) in trimmed.chars().enumerate() {
        if index == MAX_CHARS {
            capped.push('…');
            break;
        }
        capped.push(character);
    }
    capped
}

fn activity_is_background_notification_reminder(activity: &ActivityEntry) -> bool {
    activity
        .user_message
        .as_ref()
        .is_some_and(|message| message.text.contains("[BACKGROUND TASK "))
}

impl AppState {
    pub fn runtime_context_primary_summary(&self) -> String {
        self.control_dock_view_model().primary_summary
    }

    pub fn runtime_context_summary_segment_text(&self) -> Option<String> {
        self.control_dock_view_model()
            .summary_segment
            .map(|segment| segment.text)
    }

    pub fn runtime_context_provider_display(&self) -> Option<String> {
        self.control_dock_view_model().runtime_context
    }

    pub(crate) fn cache_status_summary_segment(
        &self,
    ) -> Option<view_model::ControlDockSummarySegment> {
        let cache = self
            .projection
            .activities
            .iter()
            .rev()
            .find_map(|entry| entry.cache_usage)?;

        Some(view_model::ControlDockSummarySegment {
            kind: view_model::ControlDockSummarySegmentKind::Orchestration,
            text: format!(
                "cache read {} · write {}",
                cache.read_tokens, cache.write_tokens
            ),
            tone: view_model::ControlDockSummaryTone::Secondary,
        })
    }

    pub(crate) fn control_dock_view_model(&self) -> view_model::ControlDockViewModel {
        let runtime_state = self.runtime_state();
        let grammar = view_model::runtime_context_grammar(view_model::RuntimeContextGrammarInput {
            label: self.runtime_context_label(),
            identity: self.runtime_context_identity(),
            next_turn_identity: self.next_turn_identity(),
        });
        let runtime_context = self.runtime_provider_context();

        if self.startup_shell_visible() {
            let composer_body = if self.prompt_buffer.is_empty() {
                runtime_state.composer_hint.clone()
            } else {
                self.prompt_buffer.clone()
            };
            return view_model::control_dock_view_model(view_model::ControlDockInput::Startup {
                runtime_context,
                runtime_state,
                primary_summary: grammar.primary_summary,
                composer_body,
                composer_disclosure: String::new(),
                composer_focused: self.focus == Focus::Prompt,
            });
        }

        if self.replay_mode {
            let composer_body = if runtime_state.kind == crate::app::RuntimeStateKind::Failure {
                runtime_state
                    .detail
                    .as_deref()
                    .filter(|detail| !detail.trim().is_empty())
                    .map(|detail| {
                        format!("Replay is read-only · {} · {detail}", runtime_state.summary)
                    })
                    .unwrap_or_else(|| format!("Replay is read-only · {}", runtime_state.summary))
            } else {
                "Replay is read-only.".to_string()
            };

            return view_model::control_dock_view_model(
                view_model::ControlDockInput::ReplayReadOnly {
                    runtime_context,
                    runtime_state,
                    primary_summary: grammar.primary_summary,
                    composer_body,
                    composer_disclosure: String::new(),
                    composer_focused: self.focus == Focus::Prompt,
                },
            );
        }

        let composer_body = if self.prompt_buffer.is_empty() {
            String::new()
        } else {
            self.prompt_buffer.clone()
        };
        view_model::control_dock_view_model(view_model::ControlDockInput::Live {
            runtime_context,
            runtime_state,
            primary_summary: grammar.primary_summary,
            summary_segment: self
                .cache_status_summary_segment()
                .or(grammar.summary_segment),
            composer_body,
            composer_disclosure: String::new(),
            composer_focused: self.focus == Focus::Prompt,
        })
    }

    pub(crate) fn operator_rail_has_sections(&self) -> bool {
        if self.startup_shell_visible() {
            return false;
        }

        let has_generated_session_title = self
            .events
            .iter()
            .any(|event| matches!(event.payload, EventV1::SessionTitleUpdated(_)));
        let has_user_message_title = self.activities.iter().any(|activity| {
            activity
                .user_message
                .as_ref()
                .map(|message| message.text.trim())
                .is_some_and(|text| !text.is_empty())
        });
        let title_generation_pending = !self.replay_mode
            && self
                .activities
                .iter()
                .any(|activity| activity.user_message.is_some())
            && !self
                .events
                .iter()
                .any(|event| matches!(event.payload, EventV1::ProviderRequestStarted(_)));
        let has_usage = self
            .activities
            .iter()
            .any(|activity| activity.usage.is_some());
        let has_modified_files = self
            .events
            .iter()
            .any(|event| matches!(event.payload, EventV1::EditApplied(_)));
        let has_integrations = harness_core::config::registered_integrations_config().is_some();
        let lsp = harness_core::config::registered_lsp_config();
        let has_lsp = lsp.disabled || !lsp.servers.is_empty();

        has_generated_session_title
            || has_user_message_title
            || title_generation_pending
            || has_usage
            || !self.orchestration_visible_rows().is_empty()
            || has_modified_files
            || has_integrations
            || has_lsp
    }
}
