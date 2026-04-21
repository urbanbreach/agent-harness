use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use harness_core::event::{ActorKind, EventEnvelopeV1, EventV1, ResolvedToolIdentity};

use super::permissions::PendingPermission;
use super::{
    json_string_field, mark_activity_event, merge_orchestration_task_completion_metadata,
    merge_orchestration_task_event, merge_resolved_tool_identity, merge_tool_call_metadata,
    new_streaming_activity_entry, task_completed_updates_assistant_transcript, ActivityEntry,
    ActivityStatus, ActivityUsage, AppState, Focus, MemoryCaps, NewStreamingActivityEntryArgs,
    OrchestrationOwnerLabels, OrchestrationSummary, OrchestrationTaskRow, OrchestrationTaskState,
    ToolCallDisplayStatus, ToolCallEntry, TOOL_OUTPUT_DISPLAY_MAX_CHARS,
};
use crate::view_model;

#[derive(Default)]
pub struct SessionProjection {
    pub(crate) events: Vec<EventEnvelopeV1>,
    pub(crate) activities: VecDeque<ActivityEntry>,
    pub(crate) memory_caps: MemoryCaps,
    pub(crate) events_trimmed_count: usize,
    pub(crate) transcript_trimmed_count: usize,
    orchestration_tasks: BTreeMap<String, OrchestrationTaskRow>,
    agent_profiles: BTreeMap<String, String>,
    seen_seqs: BTreeSet<u64>,
    pub(crate) pending_permissions: BTreeMap<String, PendingPermission>,
    pub(crate) run_terminal_seen: bool,
}

impl SessionProjection {
    pub(crate) fn reset(&mut self) {
        self.events.clear();
        self.activities.clear();
        self.orchestration_tasks.clear();
        self.agent_profiles.clear();
        self.seen_seqs.clear();
        self.pending_permissions.clear();
        self.run_terminal_seen = false;
        self.events_trimmed_count = 0;
        self.transcript_trimmed_count = 0;
    }

    pub(crate) fn has_seen_seq(&self, seq: u64) -> bool {
        self.seen_seqs.contains(&seq)
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

    fn adopt_local_prompt_echo(&mut self, request_id: &str, seq: u64) -> Option<usize> {
        let last_index = self.activities.len().checked_sub(1)?;
        let entry = self.activities.get_mut(last_index)?;
        if !entry.request_id.is_empty() {
            return None;
        }

        entry.request_id = request_id.to_string();
        if entry.first_seq == 0 {
            entry.first_seq = seq;
        }
        entry.last_seq = seq;
        Some(last_index)
    }

    fn activity_index_or_local_echo(&mut self, request_id: &str, seq: u64) -> Option<usize> {
        self.activity_index_for_request(request_id)
            .or_else(|| self.adopt_local_prompt_echo(request_id, seq))
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
                    permission.resolved_decision = Some(decision);
                    permission.resolution_reason = reason.map(str::to_owned);
                    permission.last_seq = seq;
                    activity.last_seq = seq;
                    return;
                }
            }

            for tool_call in &mut activity.tool_calls {
                for permission in &mut tool_call.permissions {
                    if permission.permission_id == permission_id {
                        permission.resolved_decision = Some(decision);
                        permission.resolution_reason = reason.map(str::to_owned);
                        permission.last_seq = seq;
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

    fn note_child_task_tool_call(&mut self, event: &EventEnvelopeV1) {
        let Some(request_id) = event.correlation_id.as_deref() else {
            return;
        };

        for row in self.orchestration_tasks.values_mut() {
            if row.effective_child_request_id() == Some(request_id) {
                row.child_tool_call_count = row.child_tool_call_count.saturating_add(1);
                row.last_seq = event.seq;
                row.last_mono_ms = event.mono_ms;
                row.last_timestamp = event.ts.clone();
            }
        }
    }

    fn has_active_turn_task_for_request(&self, request_id: &str) -> bool {
        self.orchestration_tasks.values().any(|row| {
            !row.state.is_terminal()
                && Self::task_row_is_turn_level(row)
                && (row.effective_child_request_id() == Some(request_id)
                    || row.request_id.as_deref() == Some(request_id))
        })
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
            .or_else(|| {
                json_string_field(
                    tool_call.output_json.as_ref(),
                    &["child_request_id", "request_id"],
                )
            });
        let child_session_id = tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.clone())
            .or_else(|| {
                json_string_field(
                    tool_call.output_json.as_ref(),
                    &["child_session_id", "session_id"],
                )
            });

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
            }
            EventV1::UserMessageSubmitted(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.user_message = Some(data.clone());
                        entry.user_timestamp = event.ts.clone();
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
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
                }
            }
            EventV1::ProviderRequestStarted(data) => {
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.model_id = data.model_id.clone();
                        entry.provider_id = data.provider_id.clone();
                        entry.request_data = Some(data.clone());
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
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
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.transcript_text.push_str(&data.delta);
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
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
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.thinking_text.push_str(&data.delta);
                        mark_activity_event(entry, event.seq, event.mono_ms);
                    }
                } else {
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.clone(),
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
                if let Some(index) = self.activity_index_or_local_echo(&data.request_id, event.seq)
                {
                    let should_mark_done = !self.has_active_turn_task_for_request(&data.request_id);
                    if let Some(entry) = self.activities.get_mut(index) {
                        if entry.tool_calls.is_empty()
                            && entry.transcript_text.is_empty()
                            && !entry.thinking_text.is_empty()
                        {
                            entry.transcript_text = std::mem::take(&mut entry.thinking_text);
                        }
                        if should_mark_done {
                            entry.status = ActivityStatus::Done;
                        }
                        entry.usage = data.usage.as_ref().map(|usage| ActivityUsage {
                            prompt_tokens: usage.prompt_tokens,
                            completion_tokens: usage.completion_tokens,
                            total_tokens: usage.total_tokens,
                        });
                        entry.last_seq = event.seq;
                        entry.last_mono_ms = event.mono_ms;
                    }
                }
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
                            if should_mark_done
                                && entry.transcript_text.is_empty()
                                && !data.result_summary.trim().is_empty()
                            {
                                entry.transcript_text = data.result_summary.clone();
                            }
                            entry.last_seq = event.seq;
                        }
                    }
                }
            }
            EventV1::TaskScheduled(data) => {
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
                    row.warning = (!data.reason.trim().is_empty()).then(|| data.reason.clone());
                });

                if should_mark_error {
                    if let Some(request_id) = event.correlation_id.as_deref() {
                        if let Some(index) = self.activity_index_for_request(request_id) {
                            if let Some(entry) = self.activities.get_mut(index) {
                                entry.status = ActivityStatus::Error;
                                entry.error_message =
                                    (!data.reason.trim().is_empty()).then(|| data.reason.clone());
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
            EventV1::StaleDetected(data) => {
                self.update_orchestration_task(event, &data.task_id, |row| {
                    row.state = OrchestrationTaskState::Stale;
                    row.warning = Some(format!("stale for {} ms", data.stale_for_ms));
                });
            }
            EventV1::ToolCallRequested(data) => {
                let target_corr_id = event.correlation_id.clone();
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
                self.note_child_task_tool_call(event);
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
            return view_model::control_dock_view_model(
                view_model::ControlDockInput::ReplayReadOnly {
                    runtime_context,
                    runtime_state,
                    primary_summary: grammar.primary_summary,
                    composer_body: "Replay is read-only.".to_string(),
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
            summary_segment: grammar.summary_segment,
            composer_body,
            composer_disclosure: String::new(),
            composer_focused: self.focus == Focus::Prompt,
        })
    }

    pub(crate) fn operator_rail_has_sections(&self) -> bool {
        if self.startup_shell_visible() {
            return false;
        }

        let has_session_title = self.activities.iter().any(|activity| {
            activity
                .user_message
                .as_ref()
                .map(|message| message.text.trim())
                .is_some_and(|text| !text.is_empty())
        });
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

        has_session_title || has_usage || has_modified_files || has_integrations || has_lsp
    }
}
