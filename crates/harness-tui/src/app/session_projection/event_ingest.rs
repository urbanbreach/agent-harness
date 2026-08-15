// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::*;

impl SessionProjection {
    #[allow(
        deprecated,
        reason = "deprecated event variants kept for backward compatibility with existing session logs"
    )]
    pub(super) fn update_derived_state_for_event(
        &mut self,
        event: &EventEnvelopeV1,
        historical: bool,
    ) {
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
                        tool_call_id: data.tool_call_id.as_ref().map(|id| id.to_string()),
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
                self.note_child_agent_request(event, data.request_id.as_str());
                if let Some(index) = self.activity_index_for_user_message(data, event.seq) {
                    let status = if self
                        .has_other_streaming_activity_in_request_scope(data.request_id.as_str())
                    {
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
                        .has_other_streaming_activity_in_request_scope(data.request_id.as_str())
                    {
                        ActivityStatus::Queued
                    } else {
                        ActivityStatus::Streaming
                    };
                    self.activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: data.request_id.to_string(),
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
                self.ensure_orphan_question_tool_calls();
            }
            EventV1::ProviderRequestStarted(data) => {
                self.note_child_agent_request(event, data.request_id.as_str());
                let turn_id = Self::canonical_provider_turn_id(event, data.request_id.as_str());
                self.note_child_agent_request(event, turn_id);
                for row in self.orchestration_tasks.values_mut() {
                    if row.effective_child_request_id() == Some(turn_id)
                        || row.effective_child_request_id() == Some(data.request_id.as_str())
                    {
                        if row.result_summary.is_none() {
                            row.result_summary = non_empty_preserved_string(&data.prompt_summary);
                        }
                        row.warning = data
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.retry)
                            .map(provider_retry_detail);
                        row.last_seq = event.seq;
                        row.last_mono_ms = event.mono_ms;
                        row.last_timestamp = event.ts.clone();
                    }
                }
                if let Some(index) =
                    self.activity_index_for_provider_event(event, data.request_id.as_str())
                {
                    let profile_label = self.profile_label_for_event(event);
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        if entry.profile_label.is_empty() {
                            entry.profile_label = profile_label;
                        }
                        if !entry.model_id.is_empty() && entry.model_id != data.model_id {
                            self.pending_status_notice = Some(format!(
                                "provider fallback: {} → {}",
                                entry.model_id, data.model_id
                            ));
                        }
                        entry.model_id = data.model_id.clone();
                        entry.provider_id = data.provider_id.clone();
                        entry.request_data = Some(data.clone());
                        entry.request_started_mono_ms = Some(event.mono_ms);
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
                    if let Some(entry) = self.activities.back_mut() {
                        entry.request_started_mono_ms = Some(event.mono_ms);
                    }
                }
                self.ensure_orphan_question_tool_calls();
            }
            EventV1::ProviderStreamDelta(data) => {
                self.note_child_agent_request(event, data.request_id.as_str());
                let turn_id = Self::canonical_provider_turn_id(event, data.request_id.as_str());
                self.note_child_agent_request(event, turn_id);
                if let Some(index) =
                    self.activity_index_for_provider_event(event, data.request_id.as_str())
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        if entry.transcript_text.is_empty() && entry.tool_calls.is_empty() {
                            entry.finish_thinking_mono(event.mono_ms);
                        }
                        if entry.first_delta_mono_ms.is_none() {
                            entry.first_delta_mono_ms = Some(event.mono_ms);
                        }
                        entry.transcript_text.push_str(&data.delta);
                        entry.bump_revision();
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
                self.note_child_agent_request(event, data.request_id.as_str());
                let turn_id = Self::canonical_provider_turn_id(event, data.request_id.as_str());
                self.note_child_agent_request(event, turn_id);
                if let Some(index) =
                    self.activity_index_for_provider_event(event, data.request_id.as_str())
                {
                    if let Some(entry) = self.activities.get_mut(index) {
                        entry.status = ActivityStatus::Streaming;
                        entry.thinking_text.push_str(&data.delta);
                        entry.note_thinking_mono(event.mono_ms);
                        entry.bump_revision();
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
                        entry.note_thinking_mono(event.mono_ms);
                        entry.bump_revision();
                    }
                }
                self.enforce_transcript_memory_cap();
            }
            EventV1::ProviderRequestFinished(data) => {
                self.note_child_agent_request(event, data.request_id.as_str());
                let turn_id = Self::canonical_provider_turn_id(event, data.request_id.as_str());
                self.note_child_agent_request(event, turn_id);
                let provider_error_detail = provider_error_detail(data);
                if let Some(index) =
                    self.activity_index_for_provider_event(event, data.request_id.as_str())
                {
                    let should_mark_done = !self.has_active_turn_task_for_request(turn_id);
                    if let Some(entry) = self.activities.get_mut(index) {
                        if entry.transcript_text.is_empty() && entry.tool_calls.is_empty() {
                            entry.finish_thinking_mono(event.mono_ms);
                        }
                        if let Some(error_detail) = provider_error_detail {
                            entry.status = ActivityStatus::Error;
                            entry.error_message = Some(error_detail);
                        } else if should_mark_done {
                            entry.status = ActivityStatus::Done;
                        }
                        if let Some(usage) = data.usage.as_ref() {
                            entry.usage = Some(ActivityUsage {
                                prompt_tokens: usage.prompt_tokens,
                                completion_tokens: usage.completion_tokens,
                                total_tokens: usage.total_tokens,
                            });
                        }
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
                            // Context breadcrumb uses prompt/context fill when reported;
                            // turn status (⇣Nk) uses activity.usage.total_tokens separately.
                            // Keep the latest provider usage visible while an enclosing turn
                            // task remains active, matching Grok's live header behavior.
                            let context_tokens = if usage.prompt_tokens > 0 {
                                usage.prompt_tokens
                            } else {
                                usage.total_tokens
                            };
                            self.active_context_usage =
                                Some(ActiveContextUsage::estimate(context_tokens));
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
            EventV1::SessionCompaction(data) => {
                self.compaction_usage_metrics.completed_count = self
                    .compaction_usage_metrics
                    .completed_count
                    .saturating_add(1);
                self.compaction_usage_metrics.last_tokens_before_estimate =
                    Some(data.tokens_before);
                self.compaction_status = Some(CompactionStatus {
                    agent_id: data.agent_id.clone(),
                    checkpoint_id: None,
                    trigger_reason: data.trigger_reason.clone(),
                    state: CompactionState::Applied,
                    message: format!(
                        "session compacted · {} tokens before · {}",
                        data.tokens_before, data.trigger_reason
                    ),
                });
            }
            EventV1::BranchSummary(data) => {
                self.compaction_status = Some(CompactionStatus {
                    agent_id: data.agent_id.clone(),
                    checkpoint_id: None,
                    trigger_reason: "branch_summary".to_string(),
                    state: CompactionState::Applied,
                    message: "branch summary generated".to_string(),
                });
            }
            EventV1::TaskCompleted(data) => {
                let should_mark_done =
                    self.is_turn_level_task_completion(data.task_id.as_str(), data);
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    row.state = OrchestrationTaskState::Completed;
                    row.warning = None;
                    row.result_summary = Some(data.result_summary.clone());
                    merge_orchestration_task_completion_metadata(row, data.metadata.as_ref());
                });

                if let Some(request_id) = event.correlation_id.as_deref() {
                    if should_mark_done {
                        self.completed_turn_request_ids
                            .insert(request_id.to_string());
                        if let Some(elapsed_ms) = data
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.timing.as_ref())
                            .and_then(|timing| timing.elapsed_ms)
                        {
                            self.terminal_elapsed_ms
                                .insert(request_id.to_string(), elapsed_ms);
                        }
                    }
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
                                    entry.bump_revision();
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
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    merge_orchestration_task_lineage(
                        row,
                        data.metadata
                            .as_ref()
                            .and_then(|metadata| metadata.lineage.as_ref()),
                    );
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
                let send_now = data.reason == "send_now";
                let should_mark_error =
                    self.is_turn_level_task_cancellation(data.task_id.as_str(), data);
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    row.state = OrchestrationTaskState::Cancelled;
                    row.warning = non_empty_preserved_string(&data.reason);
                });

                if should_mark_error {
                    if let Some(request_id) = event.correlation_id.as_deref() {
                        if let Some(index) = self.activity_index_for_request(request_id) {
                            if let Some(entry) = self.activities.get_mut(index) {
                                entry.status = ActivityStatus::Error;
                                if send_now {
                                    entry.error_message = None;
                                } else {
                                    let reason = non_empty_preserved_string(&data.reason);
                                    entry.error_message = match (reason, entry.error_message.take())
                                    {
                                        (Some(reason), Some(existing))
                                            if !existing.contains(&reason) =>
                                        {
                                            Some(format!("{reason} · {existing}"))
                                        }
                                        (Some(reason), _) => Some(reason),
                                        (None, existing) => existing,
                                    };
                                }
                                mark_activity_event(entry, event.seq, event.mono_ms);
                            }
                        }
                    }
                }
            }
            EventV1::TaskResultLate(data) => {
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    row.state = OrchestrationTaskState::LateResult;
                    row.warning = Some("late result after stale cancellation".to_string());
                });
            }
            EventV1::BackgroundTaskNotification(data) => {
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
                    row.child_session_id = Some(data.child_session_id.to_string());
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
            EventV1::StaleDetected(data) => {
                self.update_orchestration_task(event, data.task_id.as_str(), |row| {
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
                    if entry.transcript_text.is_empty() && entry.tool_calls.is_empty() {
                        entry.finish_thinking_mono(event.mono_ms);
                    }
                    let tool_entry = ToolCallEntry {
                        tool_call_id: data.tool_call_id.to_string(),
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
                if let Some(tool_entry) = self.find_tool_call_mut(data.tool_call_id.as_str()) {
                    tool_entry.lifecycle_state =
                        Some(harness_core::event::ToolCallLifecycleState::Running);
                    tool_entry.sync_display_status();
                    tool_entry.last_seq = event.seq;
                    tool_entry.last_mono_ms = event.mono_ms;
                    tool_entry.last_timestamp = event.ts.clone();
                }
            }
            EventV1::ToolCallFinished(data) => {
                if let Some(tool_entry) = self.find_tool_call_mut(data.tool_call_id.as_str()) {
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
}
