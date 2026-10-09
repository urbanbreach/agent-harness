use super::*;

impl SessionProjection {
    pub(super) fn update_live_tool_event(&mut self, event: &EventEnvelopeV1) -> bool {
        match &event.payload {
            EventV1::ToolCallRequested(data) => {
                let target_corr_id = event.correlation_id.clone();
                let use_back = self
                    .activities
                    .back()
                    .is_none_or(|entry| target_corr_id.is_none() || entry.request_id.is_empty());

                let entry = if use_back {
                    self.activities.back_mut()
                } else if let Some(corr) = &target_corr_id {
                    self.activity_index_for_correlation(corr)
                        .and_then(|index| self.activities.get_mut(index))
                } else {
                    None
                };

                if let Some(entry) = entry {
                    if entry.transcript_text.is_empty() && entry.tool_calls.is_empty() {
                        entry.finish_thinking_mono(event.mono_ms);
                    }
                    let existing_index = entry
                        .tool_calls
                        .iter()
                        .position(|tool_call| tool_call.tool_call_id == data.tool_call_id.as_str());
                    if let Some(existing_index) = existing_index {
                        let tool_entry = &mut entry.tool_calls[existing_index];
                        tool_entry.tool_id.clone_from(&data.tool_id);
                        tool_entry.args_summary.clone_from(&data.args_summary);
                        tool_entry.args_digest.clone_from(&data.args_digest);
                        tool_entry.lifecycle_state = Some(ToolCallLifecycleState::Pending);
                        tool_entry.last_seq = event.seq;
                        tool_entry.last_mono_ms = event.mono_ms;
                        tool_entry.last_timestamp.clone_from(&event.ts);
                        merge_resolved_tool_identity(
                            tool_entry,
                            ResolvedToolIdentity::from_tool_call(
                                Some(data.tool_id.as_str()),
                                data.metadata.as_ref(),
                            ),
                        );
                        merge_tool_call_metadata(tool_entry, data.metadata.as_ref());
                        tool_entry.sync_display_status();
                    } else {
                        let mut tool_entry = ToolCallEntry {
                            hook_executions: Vec::new(),
                            tool_call_id: data.tool_call_id.to_string(),
                            tool_id: data.tool_id.clone(),
                            canonical_tool_id: None,
                            alias_source_tool_id: None,
                            resolved_tool_identity: None,
                            args_summary: data.args_summary.clone(),
                            args_digest: data.args_digest.clone(),
                            lifecycle_state: Some(ToolCallLifecycleState::Pending),
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
                    }
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
            EventV1::EditProposed(_) | EventV1::EditApplied(_) | EventV1::EditRejected(_) => {
                if let Some(tool) = event
                    .correlation_id
                    .as_deref()
                    .and_then(|id| self.find_tool_call_mut(id))
                {
                    super::edit::apply_edit(tool, &event.payload);
                    tool.last_seq = event.seq;
                }
            }
            _ => return false,
        }
        true
    }

    /// A turn's request id, or the tool call id of the eval cell that made a nested call.
    fn activity_index_for_correlation(&self, correlation: &str) -> Option<usize> {
        self.activities
            .iter()
            .position(|activity| activity.request_id == correlation)
            .or_else(|| {
                self.activities.iter().rposition(|activity| {
                    activity
                        .tool_calls
                        .iter()
                        .any(|tool| tool.tool_call_id == correlation)
                })
            })
    }
}
