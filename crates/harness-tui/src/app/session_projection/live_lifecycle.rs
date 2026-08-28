use super::*;

impl SessionProjection {
    pub(super) fn update_live_lifecycle_event(
        &mut self,
        event: &EventEnvelopeV1,
        historical: bool,
    ) -> bool {
        match &event.payload {
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
                let is_latest = self
                    .latest_request_budget
                    .is_none_or(|(seq, _)| event.seq > seq);
                if is_latest {
                    let snapshot = data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.context_budget);
                    self.latest_request_budget = Some((event.seq, snapshot));
                }
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
            _ => return false,
        }
        true
    }
}
