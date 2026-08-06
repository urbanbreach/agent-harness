// allow: SIZE_OK — TUI app state (session projection + interaction)
use crate::UnwrapOrAbort;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use harness_core::event::{
    ActorKind, BackgroundTaskNotificationEvent, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestRetryMetadata, ResolvedToolIdentity,
    UserMessageSubmittedEvent,
};

use super::permissions::PendingPermission;
use super::{
    mark_activity_event, merge_orchestration_task_completion_metadata,
    merge_orchestration_task_event, merge_resolved_tool_identity, merge_tool_call_metadata,
    new_streaming_activity_entry, task_child_request_id_from_output,
    task_child_session_id_from_output, task_completed_updates_assistant_transcript,
    ActiveContextUsage, ActivityCacheUsage, ActivityEntry, ActivityStatus, ActivityUsage, AppState,
    CompactionState, CompactionStatus, CompactionUsageMetrics, EditDisplayStatus, EditEntry, Focus,
    MemoryCaps, NewStreamingActivityEntryArgs, OrchestrationOwnerLabels, OrchestrationSummary,
    OrchestrationTaskRow, OrchestrationTaskState, ToolCallDisplayStatus, ToolCallEntry,
    TOOL_OUTPUT_DISPLAY_MAX_CHARS,
};
use crate::text::{has_trimmed_content, non_empty_preserved_string};
use crate::view_model;

#[path = "session_projection/background_notification.rs"]
mod background_notification;
mod event_ingest;

use self::background_notification::{
    activity_is_background_notification_reminder, background_task_notification_text,
};
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
    fallback_profile_label: String,
    seen_seqs: BTreeSet<u64>,
    pub(crate) pending_permissions: BTreeMap<String, PendingPermission>,
    pub(crate) run_terminal_seen: bool,
    pub(crate) pending_status_notice: Option<String>,
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
        self.pending_status_notice = None;
    }

    pub(crate) fn set_fallback_profile_label(&mut self, profile: impl Into<String>) {
        self.fallback_profile_label = profile.into();
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
            .or_else(|| self.profile_label_for_request(event.correlation_id.as_deref()))
            .or_else(|| self.single_root_profile_label())
            .or_else(|| self.single_profile_label())
            .or_else(|| {
                has_trimmed_content(&self.fallback_profile_label)
                    .then(|| self.fallback_profile_label.clone())
            })
            .unwrap_or_else(|| "default".to_string())
    }

    fn profile_label_for_request(&self, request_id: Option<&str>) -> Option<String> {
        let agent_id = self.child_request_agents.get(request_id?)?;
        self.agent_profiles.get(agent_id).cloned()
    }

    fn single_root_profile_label(&self) -> Option<String> {
        let mut labels = self
            .agent_profiles
            .iter()
            .filter(|(agent_id, _)| !self.child_agent_ids.contains(*agent_id))
            .map(|(_, profile)| profile);
        let label = labels.next()?;
        labels.next().is_none().then(|| label.clone())
    }

    fn single_profile_label(&self) -> Option<String> {
        let mut labels = self.agent_profiles.values();
        let label = labels.next()?;
        labels.next().is_none().then(|| label.clone())
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

    fn has_tool_call(&self, tool_call_id: &str) -> bool {
        self.activities.iter().any(|activity| {
            activity
                .tool_calls
                .iter()
                .any(|tool_call| tool_call.tool_call_id == tool_call_id)
        })
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
                    request_id: request_id.into(),
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
        if let Some(index) = self.activity_index_for_request(data.request_id.as_str()) {
            self.remove_duplicate_local_prompt_echo(&data.text, index);
            return self.activity_index_for_request(data.request_id.as_str());
        }

        self.local_prompt_echo_index_for_message(&data.text)
            .and_then(|index| self.adopt_local_prompt_echo_at(index, data.request_id.as_str(), seq))
            .or_else(|| self.adopt_local_prompt_echo(data.request_id.as_str(), seq))
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
            tool_call_id: data.tool_call_id.as_ref().map(|id| id.to_string()),
            summary: data.summary.clone(),
            request_digest: data.request_digest.clone(),
            timeout_ms: data.timeout_ms,
            default_decision: data.default_decision,
            resolved_decision: None,
            resolution_reason: None,
            first_seq: event.seq,
            last_seq: event.seq,
        };

        if let Some(tool_call_id) = data.tool_call_id.as_ref().map(|id| id.as_str()) {
            if let Some(tool_entry) = self.find_tool_call_mut(tool_call_id) {
                tool_entry.permissions.push(permission_entry);
                tool_entry.sync_display_status();
                return;
            }
            if is_question_permission_kind(&permission_entry.kind) {
                let activity_index = event
                    .correlation_id
                    .as_deref()
                    .and_then(|request_id| {
                        self.activities
                            .iter()
                            .position(|activity| activity.request_id == request_id)
                    })
                    .or_else(|| self.activities.len().checked_sub(1));
                if let Some(activity_index) = activity_index {
                    if self.push_orphan_question_tool_call(
                        activity_index,
                        tool_call_id,
                        permission_entry,
                        event.seq,
                        event.mono_ms,
                        event.ts.clone(),
                    ) {
                        return;
                    }
                }
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

    fn ensure_orphan_question_tool_calls(&mut self) {
        let Some(latest_index) = self.activities.len().checked_sub(1) else {
            return;
        };

        let pending: Vec<(String, super::PermissionEntry)> = self
            .pending_permissions
            .iter()
            .filter(|(_, pending)| is_question_permission_kind(&pending.kind))
            .filter_map(|(permission_id, pending)| {
                let tool_call_id = pending.tool_call_id.as_deref()?;
                Some((
                    tool_call_id.to_string(),
                    super::PermissionEntry {
                        permission_id: permission_id.clone(),
                        kind: pending.kind.clone(),
                        tool_call_id: Some(tool_call_id.to_string()),
                        summary: pending.summary.clone(),
                        request_digest: pending.request_digest.clone(),
                        timeout_ms: pending.timeout_ms,
                        default_decision: pending.default_decision,
                        resolved_decision: None,
                        resolution_reason: None,
                        first_seq: pending.seq,
                        last_seq: pending.seq,
                    },
                ))
            })
            .collect();

        for (tool_call_id, permission_entry) in pending {
            if self.activities.get(latest_index).is_some_and(|activity| {
                activity
                    .tool_calls
                    .iter()
                    .any(|tool_call| tool_call.tool_call_id == tool_call_id)
            }) {
                continue;
            }

            let mut relocated: Option<ToolCallEntry> = None;
            for (index, activity) in self.activities.iter_mut().enumerate() {
                if index == latest_index {
                    continue;
                }
                if let Some(pos) = activity
                    .tool_calls
                    .iter()
                    .position(|tool_call| tool_call.tool_call_id == tool_call_id)
                {
                    relocated = Some(activity.tool_calls.remove(pos));
                    break;
                }
            }

            if let Some(tool_entry) = relocated {
                if let Some(activity) = self.activities.get_mut(latest_index) {
                    activity.tool_calls.push(tool_entry);
                    activity.last_seq = activity.last_seq.max(permission_entry.last_seq);
                }
                continue;
            }

            let seq = permission_entry.first_seq;
            let _ = self.push_orphan_question_tool_call(
                latest_index,
                &tool_call_id,
                permission_entry,
                seq,
                0,
                None,
            );
        }
    }

    fn push_orphan_question_tool_call(
        &mut self,
        activity_index: usize,
        tool_call_id: &str,
        permission_entry: super::PermissionEntry,
        seq: u64,
        mono_ms: u64,
        timestamp: Option<String>,
    ) -> bool {
        if self.has_tool_call(tool_call_id) {
            return false;
        }
        let Some(activity) = self.activities.get_mut(activity_index) else {
            return false;
        };

        let mut tool_entry = ToolCallEntry {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "user.question".to_string(),
            canonical_tool_id: Some("user.question".to_string()),
            alias_source_tool_id: None,
            resolved_tool_identity: None,
            args_summary: permission_entry.summary.clone(),
            args_digest: permission_entry.request_digest.clone(),
            lifecycle_state: Some(harness_core::event::ToolCallLifecycleState::Pending),
            status: ToolCallDisplayStatus::PendingPermission,
            output_summary: None,
            output_digest: None,
            output_json: None,
            truncated_output: None,
            edit: None,
            lineage: None,
            artifact_refs: Vec::new(),
            timing_elapsed_ms: None,
            permissions: vec![permission_entry],
            first_seq: seq,
            last_seq: seq,
            first_mono_ms: mono_ms,
            last_mono_ms: mono_ms,
            first_timestamp: timestamp.clone(),
            last_timestamp: timestamp,
        };
        tool_entry.sync_display_status();
        activity.tool_calls.push(tool_entry);
        activity.last_seq = activity.last_seq.max(seq);
        // Advance turn mono so Waiting footer can pack elapsed duration.
        if activity.first_mono_ms == 0 {
            activity.first_mono_ms = mono_ms;
        }
        activity.last_mono_ms = activity.last_mono_ms.max(mono_ms);
        true
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
                row.warning = None;
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

    pub(crate) fn active_background_task_count(&self) -> usize {
        self.orchestration_tasks
            .values()
            .filter(|row| !row.state.is_terminal() && !Self::task_row_is_turn_level(row))
            .count()
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

fn provider_retry_detail(retry: ProviderRequestRetryMetadata) -> String {
    let category = retry
        .category
        .map(|category| format!(" · {}", category.as_str()))
        .unwrap_or_default();
    format!("Retrying (attempt {}){category}", retry.attempt)
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

fn is_question_permission_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("question")
        || kind.eq_ignore_ascii_case("ask")
        || kind.eq_ignore_ascii_case("ask_user")
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

    pub(crate) fn active_retry_metadata(&self) -> Option<ProviderRequestRetryMetadata> {
        self.activities.iter().rev().find_map(|entry| {
            if entry.status != ActivityStatus::Streaming {
                return None;
            }
            let metadata = entry.request_data.as_ref()?.metadata.as_ref()?;
            metadata.retry
        })
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
            let composer_text = self.composer_render_text();
            let composer_body = if composer_text.is_empty() {
                runtime_state.composer_hint.clone()
            } else {
                composer_text
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

        let composer_text = self.composer_render_text();
        let composer_body = if composer_text.is_empty() {
            String::new()
        } else {
            composer_text
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn active_retry_metadata_returns_some_for_streaming_retry() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.activities.push_back(ActivityEntry {
            request_id: "req_retry".to_string(),
            profile_label: "build".to_string(),
            model_id: "gpt-5.4".to_string(),
            provider_id: "default".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: Some(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_retry".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4".to_string(),
                prompt_summary: "prompt summary".to_string(),
                request_digest: "digest-retry".to_string(),
                metadata: Some(harness_core::event::ProviderRequestStartedMetadata {
                    retry: Some(harness_core::event::ProviderRequestRetryMetadata {
                        attempt: 3,
                        max_attempts: 4,
                        delay_ms: None,
                        category: None,
                    }),
                    ..harness_core::event::ProviderRequestStartedMetadata::default()
                }),
            }),
            thinking_text: String::new(),
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: String::new(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            request_started_mono_ms: None,
            revision: 0,
        });

        // act
        let retry = app.active_retry_metadata();

        // assert
        let retry = retry.unwrap_or_abort();
        assert_eq!(retry.attempt, 3);
        assert_eq!(retry.max_attempts, 4);
    }

    #[test]
    fn active_retry_metadata_returns_none_for_non_streaming_activity() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.activities.push_back(ActivityEntry {
            request_id: "req_done".to_string(),
            profile_label: "build".to_string(),
            model_id: "gpt-5.4".to_string(),
            provider_id: "default".to_string(),
            status: ActivityStatus::Done,
            user_message: None,
            user_timestamp: None,
            request_data: Some(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_done".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4".to_string(),
                prompt_summary: "prompt summary".to_string(),
                request_digest: "digest-done".to_string(),
                metadata: Some(harness_core::event::ProviderRequestStartedMetadata {
                    retry: Some(harness_core::event::ProviderRequestRetryMetadata {
                        attempt: 1,
                        max_attempts: 3,
                        delay_ms: None,
                        category: None,
                    }),
                    ..harness_core::event::ProviderRequestStartedMetadata::default()
                }),
            }),
            thinking_text: String::new(),
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: String::new(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            request_started_mono_ms: None,
            revision: 0,
        });

        // act
        let retry = app.active_retry_metadata();

        // assert
        assert!(retry.is_none());
    }
}
