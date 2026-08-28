use super::*;

impl SessionProjection {
    pub(super) fn update_live_tool_event(&mut self, event: &EventEnvelopeV1) -> bool {
        match &event.payload {
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
            _ => return false,
        }
        true
    }
}
