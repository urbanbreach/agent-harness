// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::ui_transcript_tool_sections::{build_tool_call_section, successful_edit_summary_title};
use super::*;

pub(super) fn build_transcript_sections(app: &AppState) -> Vec<TranscriptTurnSection> {
    let motion_enabled = app.transcript_motion_enabled() && !app.replay_mode;
    let hidden_child_request_ids = hidden_delegated_child_request_ids(app);
    let visible_activities = app
        .activities
        .iter()
        .enumerate()
        .filter(|(_, activity)| !hidden_child_request_ids.contains(activity.request_id.as_str()))
        .collect::<Vec<_>>();
    let mut turn_sections = Vec::with_capacity(visible_activities.len());
    let pending_assistant_index = visible_activities
        .iter()
        .rposition(|(_, activity)| activity.status == ActivityStatus::Streaming);

    for (visible_index, (activity_index, activity)) in visible_activities.iter().enumerate() {
        turn_sections.push(build_turn_section(BuildTurnSectionArgs {
            activity_first_seq: activity.first_seq,
            activity,
            queued_user_message: pending_assistant_index
                .is_some_and(|pending| visible_index > pending),
            is_selected: transcript_surface_focused(app)
                && *activity_index == app.transcript_view.selected_activity_index,
            is_latest: false,
            thinking_visible: app.transcript_thinking_visible(),
            timestamps_visible: app.transcript_timestamps_visible(),
            show_tool_details: app.tool_details_visible(),
            show_generic_tool_output: app.generic_tool_output_visible(),
            stacked_diffs: app.stacked_transcript_diffs(),
            motion_enabled,
            session_path: app.session_path.as_deref(),
            app,
        }));
    }

    if let Some(latest_assistant_footer_index) = turn_sections
        .iter()
        .rposition(|turn| turn_supports_assistant_footer(turn, app))
    {
        let original_activity_index = visible_activities[latest_assistant_footer_index].0;
        if let Some(turn) = turn_sections.get_mut(latest_assistant_footer_index) {
            turn.show_footer = true;
            turn.footer_timestamp = app.activities[original_activity_index]
                .user_timestamp
                .as_deref()
                .map(crate::time_format::wall_clock_12h);
        }
    }

    inject_compaction_events(app, &visible_activities, &mut turn_sections);

    turn_sections
}

fn inject_compaction_events(
    app: &AppState,
    visible_activities: &[(usize, &ActivityEntry)],
    turn_sections: &mut Vec<TranscriptTurnSection>,
) {
    for event in &app.events {
        let compaction_section = match &event.payload {
            harness_core::event::EventV1::SessionCompaction(data) => TranscriptCompactionSection {
                kind: TranscriptCompactionKind::SessionCompaction,
                summary: data.summary.clone(),
                tokens_before: Some(data.tokens_before),
                read_files: data.read_files.clone(),
                modified_files: data.modified_files.clone(),
            },
            harness_core::event::EventV1::BranchSummary(data) => TranscriptCompactionSection {
                kind: TranscriptCompactionKind::BranchSummary,
                summary: data.summary.clone(),
                tokens_before: None,
                read_files: data.read_files.clone(),
                modified_files: data.modified_files.clone(),
            },
            _ => continue,
        };

        let target_turn_index = visible_activities
            .iter()
            .enumerate()
            .filter(|(_, (_, activity))| activity.last_seq <= event.seq)
            .map(|(turn_idx, _)| turn_idx)
            .next_back()
            .or_else(|| (!visible_activities.is_empty()).then_some(0));

        if let Some(turn_index) = target_turn_index {
            if let Some(turn) = turn_sections.get_mut(turn_index) {
                turn.assistant_parts
                    .push(TranscriptAssistantPart::Compaction(compaction_section));
                turn.assistant_part_source_ids
                    .push(TranscriptAssistantPartSourceId(event.seq));
            }
        }
    }
}

fn turn_supports_assistant_footer(turn: &TranscriptTurnSection, app: &AppState) -> bool {
    matches!(turn.header.status, ActivityStatus::Streaming)
        || app.turn_completion_seen(&turn.request_id)
}

fn build_turn_section(args: BuildTurnSectionArgs<'_>) -> TranscriptTurnSection {
    let BuildTurnSectionArgs {
        activity_first_seq,
        activity,
        queued_user_message,
        is_selected,
        is_latest,
        thinking_visible,
        timestamps_visible,
        show_tool_details,
        show_generic_tool_output,
        stacked_diffs,
        motion_enabled,
        session_path,
        app,
    } = args;

    let user_message = activity.user_message.as_ref().map(|user_msg| {
        let timestamp = activity
            .user_timestamp
            .as_deref()
            .filter(|_| timestamps_visible);
        TranscriptUserMessageSection {
            text: user_msg.text.clone(),
            queued: queued_user_message,
            wall_clock: timestamp.map(crate::time_format::wall_clock_12h),
            expanded_wall_clock: timestamp.map(crate::time_format::wall_clock_hover_detail),
            wall_clock_hovered: matches!(
                app.hovered_transcript_target(),
                Some(TranscriptMouseTarget::UserTimestamp { request_id })
                    if request_id == &activity.request_id
            ),
        }
    });

    let thinking = thinking_visible
        .then(|| {
            if !activity.thinking_text.trim().is_empty()
                || activity.thinking_duration_ms().is_some()
            {
                Some(TranscriptLabeledTextSection {
                    label: THINKING_TRACE_LABEL,
                    text: activity.thinking_text.clone(),
                })
            } else {
                None
            }
        })
        .flatten();

    let mut body_blocks = Vec::new();
    if !activity.transcript_text.is_empty() {
        body_blocks.push(match activity.status {
            ActivityStatus::Streaming => {
                TranscriptBodyBlock::StreamingRichText(activity.transcript_text.clone())
            }
            ActivityStatus::Queued | ActivityStatus::Done | ActivityStatus::Error => {
                TranscriptBodyBlock::RichText(activity.transcript_text.clone())
            }
        });
    }

    let mut ordered_tool_calls: Vec<TranscriptOrderedToolCallSection> = Vec::new();
    for tool_call in &activity.tool_calls {
        let Some(section) = build_tool_call_section(
            tool_call,
            app,
            show_tool_details,
            timestamps_visible,
            show_generic_tool_output,
            app.tool_output_expanded(tool_call),
            stacked_diffs,
            session_path,
        ) else {
            continue;
        };
        if let Some(previous) = ordered_tool_calls.last_mut() {
            let previous_call = activity
                .tool_calls
                .iter()
                .find(|candidate| candidate.tool_call_id == previous.tool_call_id);
            if previous_call.is_some_and(|candidate| safe_same_file_edit_pair(candidate, tool_call))
                && !previous.section.detail_blocks.is_empty()
                && !section.detail_blocks.is_empty()
                && previous
                    .section
                    .detail_blocks
                    .iter()
                    .all(is_structured_diff_block)
                && section.detail_blocks.iter().all(is_structured_diff_block)
            {
                if previous.section.detail_blocks != section.detail_blocks {
                    previous.section.detail_blocks.extend(section.detail_blocks);
                }
                previous
                    .section
                    .coalesced_tool_call_ids
                    .push(tool_call.tool_call_id.clone());
                previous.section.expanded |= section.expanded;
                previous.section.details_collapsed_by_default = true;
                previous.section.details_preview_visible = false;
                previous.section.header.disclosure_state = Some(if previous.section.expanded {
                    TranscriptToolCallDisclosureState::Expanded
                } else {
                    TranscriptToolCallDisclosureState::Collapsed
                });
                previous.section.header.title =
                    successful_edit_summary_title(tool_call, &previous.section.detail_blocks);
                continue;
            }
        }
        ordered_tool_calls.push(TranscriptOrderedToolCallSection {
            tool_call_id: tool_call.tool_call_id.clone(),
            first_seq: tool_call.first_seq,
            section,
        });
    }
    let error = activity
        .error_message
        .as_ref()
        .map(|text| TranscriptErrorSection {
            text: cancel_error_display_text(text, activity.duration_ms())
                .unwrap_or_else(|| text.clone()),
        });
    let BuiltTranscriptAssistantParts {
        parts: assistant_parts,
        source_ids: assistant_part_source_ids,
    } = build_ordered_assistant_parts(
        activity,
        app,
        thinking_visible,
        thinking.clone(),
        body_blocks.clone(),
        ordered_tool_calls,
        error.clone(),
    );

    TranscriptTurnSection {
        activity_first_seq,
        request_id: activity.request_id.clone(),
        user_message,
        show_footer: is_latest
            && (matches!(activity.status, ActivityStatus::Streaming)
                || app.turn_completion_seen(&activity.request_id)),
        footer_timestamp: activity
            .user_timestamp
            .as_deref()
            .map(crate::time_format::wall_clock_12h),
        animation_phase: app.transcript_animation_phase(),
        motion_enabled,
        reasoning_expanded: app.reasoning_expanded(&activity.request_id),
        header: TranscriptTurnHeader {
            status: activity.status,
            is_selected,
            is_hovered: matches!(
                app.hovered_transcript_target(),
                Some(TranscriptMouseTarget::Reasoning { request_id })
                    if request_id == &activity.request_id
            ),
            provider_request_open: app
                .events
                .iter()
                .rev()
                .find_map(|event| match &event.payload {
                    harness_core::event::EventV1::ProviderRequestStarted(data)
                        if provider_event_matches_activity(
                            event,
                            data.request_id.as_str(),
                            &activity.request_id,
                        ) =>
                    {
                        Some(true)
                    }
                    harness_core::event::EventV1::ProviderRequestFinished(data)
                        if provider_event_matches_activity(
                            event,
                            data.request_id.as_str(),
                            &activity.request_id,
                        ) =>
                    {
                        Some(false)
                    }
                    _ => None,
                })
                == Some(true),
            profile_label: activity.profile_label.clone(),
            model_id: activity.model_id.clone(),
            duration_ms: app
                .terminal_elapsed_ms(&activity.request_id)
                .or_else(|| activity.duration_ms()),
            thinking_duration_ms: activity.thinking_duration_ms(),
            responding_duration_ms: activity.responding_duration_ms(),
            total_tokens: activity.usage.map(|usage| usage.total_tokens),
            retry: activity
                .request_data
                .as_ref()
                .and_then(|data| data.metadata.as_ref())
                .and_then(|metadata| metadata.retry),
            retry_elapsed_ms: activity
                .request_started_mono_ms
                .zip(activity.duration_ms())
                .map(|(started, _)| activity.last_mono_ms.saturating_sub(started)),
        },
        assistant_parts,
        assistant_part_source_ids,
    }
}

fn is_structured_diff_block(block: &TranscriptToolCallDetailBlock) -> bool {
    matches!(block, TranscriptToolCallDetailBlock::StructuredDiff { .. })
}

fn safe_same_file_edit_pair(
    before: &crate::app::ToolCallEntry,
    after: &crate::app::ToolCallEntry,
) -> bool {
    let trusted_success = |call: &crate::app::ToolCallEntry| {
        call.status == ToolCallDisplayStatus::Succeeded
            && matches!(
                call.effective_tool_id(),
                "edit" | "edit.hashline_apply" | "write" | "fs.write"
            )
            && call
                .edit
                .as_ref()
                .is_none_or(|edit| edit.status == crate::app::EditDisplayStatus::Applied)
    };
    let both_writes = matches!(before.effective_tool_id(), "write" | "fs.write")
        && matches!(after.effective_tool_id(), "write" | "fs.write");
    trusted_success(before)
        && trusted_success(after)
        && before.edit_path_display().is_some()
        && before.edit_path_display() == after.edit_path_display()
        && (!both_writes || before.args_summary == after.args_summary)
}

fn build_ordered_assistant_parts(
    activity: &ActivityEntry,
    app: &AppState,
    thinking_visible: bool,
    thinking: Option<TranscriptLabeledTextSection>,
    body_blocks: Vec<TranscriptBodyBlock>,
    ordered_tool_calls: Vec<TranscriptOrderedToolCallSection>,
    error: Option<TranscriptErrorSection>,
) -> BuiltTranscriptAssistantParts {
    let mut event_parts = build_ordered_assistant_parts_from_events(
        activity,
        app,
        &ordered_tool_calls,
        thinking_visible,
    );
    if event_parts.is_empty() {
        let mut fallback_parts = Vec::new();
        let mut next_index = 0;
        if let Some(thinking) = thinking {
            fallback_parts.push(SequencedTranscriptAssistantPart {
                seq: activity.first_seq,
                index: next_index,
                part: TranscriptAssistantPart::Reasoning(thinking),
            });
            next_index += 1;
        }
        for body in body_blocks {
            fallback_parts.push(SequencedTranscriptAssistantPart {
                seq: activity.last_seq,
                index: next_index,
                part: TranscriptAssistantPart::Body(body),
            });
            next_index += 1;
        }
        for tool_call in ordered_tool_calls {
            fallback_parts.push(SequencedTranscriptAssistantPart {
                seq: tool_call.first_seq,
                index: next_index,
                part: TranscriptAssistantPart::ToolCall(Box::new(tool_call.section)),
            });
            next_index += 1;
        }
        if let Some(error) = error {
            fallback_parts.push(SequencedTranscriptAssistantPart {
                seq: activity.last_seq,
                index: next_index,
                part: TranscriptAssistantPart::Error(error),
            });
        }
        return BuiltTranscriptAssistantParts::from_sequenced(fallback_parts);
    }

    sync_reasoning_parts_with_activity(&mut event_parts, activity, thinking_visible);
    ensure_completed_thought_header(&mut event_parts, activity, thinking_visible);
    if let Some(error) = error {
        event_parts.push(SequencedTranscriptAssistantPart {
            seq: activity.last_seq,
            index: event_parts.len(),
            part: TranscriptAssistantPart::Error(error),
        });
    }
    BuiltTranscriptAssistantParts::from_sequenced(event_parts)
}

fn ensure_completed_thought_header(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    activity: &ActivityEntry,
    thinking_visible: bool,
) {
    if !thinking_visible {
        return;
    }
    // Completed turns without reasoning do not show
    // Thought for completed turns without reasoning deltas. Only add Thought
    // when reasoning events were received or thinking text exists.
    let has_reasoning =
        !activity.thinking_text.trim().is_empty() || activity.thinking_duration_ms().is_some();
    let complete_with_reasoning = matches!(activity.status, ActivityStatus::Done) && has_reasoning;
    let error_with_reasoning = matches!(activity.status, ActivityStatus::Error) && has_reasoning;
    if !complete_with_reasoning && !error_with_reasoning {
        return;
    }
    if !activity.tool_calls.is_empty() && !activity_has_thinking_text(activity) {
        return;
    }
    if parts
        .iter()
        .any(|part| matches!(part.part, TranscriptAssistantPart::Reasoning(_)))
    {
        return;
    }
    parts.insert(
        0,
        SequencedTranscriptAssistantPart {
            seq: activity.first_seq,
            index: 0,
            part: TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: activity.thinking_text.clone(),
            }),
        },
    );
}

fn sync_reasoning_parts_with_activity(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    activity: &ActivityEntry,
    thinking_visible: bool,
) {
    if !thinking_visible {
        parts.retain(|part| !matches!(part.part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }

    let has_reasoning =
        !activity.thinking_text.trim().is_empty() || activity.thinking_duration_ms().is_some();
    if !has_reasoning {
        // No reasoning events or thinking text — remove reasoning parts
        // (turns without reasoning do not show Thought).
        parts.retain(|part| !matches!(part.part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }

    let reasoning_indices = parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            matches!(part.part, TranscriptAssistantPart::Reasoning(_)).then_some(index)
        })
        .collect::<Vec<_>>();

    let Some(first_reasoning_index) = reasoning_indices.first().copied() else {
        return;
    };

    if reasoning_indices.len() == 1 {
        parts[first_reasoning_index].part =
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: activity.thinking_text.clone(),
            });
        return;
    }

    let rendered = reasoning_indices
        .iter()
        .filter_map(|index| match parts.get(*index).map(|part| &part.part) {
            Some(TranscriptAssistantPart::Reasoning(reasoning)) => Some(reasoning.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if rendered == activity.thinking_text {
        return;
    }

    if let Some(remainder) = activity.thinking_text.strip_prefix(&rendered) {
        if remainder.is_empty() {
            return;
        }
        if let Some(last_reasoning_index) = reasoning_indices.last().copied() {
            if let Some(TranscriptAssistantPart::Reasoning(reasoning)) = parts
                .get_mut(last_reasoning_index)
                .map(|part| &mut part.part)
            {
                reasoning.text.push_str(remainder);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct SequencedTranscriptAssistantPart {
    seq: u64,
    index: usize,
    part: TranscriptAssistantPart,
}

struct BuiltTranscriptAssistantParts {
    parts: Vec<TranscriptAssistantPart>,
    source_ids: Vec<TranscriptAssistantPartSourceId>,
}

impl BuiltTranscriptAssistantParts {
    fn from_sequenced(parts: Vec<SequencedTranscriptAssistantPart>) -> Self {
        let mut assistant_parts = Vec::with_capacity(parts.len());
        let mut source_ids = Vec::with_capacity(parts.len());
        for part in parts {
            source_ids.push(TranscriptAssistantPartSourceId(part.seq));
            assistant_parts.push(part.part);
        }
        Self {
            parts: assistant_parts,
            source_ids,
        }
    }
}

fn build_ordered_assistant_parts_from_events(
    activity: &ActivityEntry,
    app: &AppState,
    ordered_tool_calls: &[TranscriptOrderedToolCallSection],
    thinking_visible: bool,
) -> Vec<SequencedTranscriptAssistantPart> {
    let mut parts = Vec::new();
    let mut next_index = 0usize;
    let mut saw_turn_event = false;
    let mut saw_reasoning_event = false;
    let mut saw_body_event = false;
    let mut saw_tool_call = false;
    let mut pending_pre_tool_stream: Option<(u64, String)> = None;
    let mut pending_tool_calls = ordered_tool_calls
        .iter()
        .cloned()
        .map(|tool_call| (tool_call.tool_call_id.clone(), tool_call))
        .collect::<std::collections::BTreeMap<_, _>>();

    for event in app.events.iter().filter(|event| {
        event.seq >= activity.first_seq
            && event.seq <= activity.last_seq
            && turn_event_matches_activity(event, &activity.request_id)
    }) {
        match &event.payload {
            harness_core::event::EventV1::ProviderReasoningDelta(data)
                if provider_event_matches_activity(
                    event,
                    data.request_id.as_str(),
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                if thinking_visible {
                    saw_reasoning_event = true;
                    settle_trailing_body(&mut parts);
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        TranscriptAssistantTextKind::Reasoning,
                        &data.delta,
                    );
                }
            }
            harness_core::event::EventV1::ProviderStreamDelta(data)
                if provider_event_matches_activity(
                    event,
                    data.request_id.as_str(),
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                if saw_tool_call {
                    saw_body_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        TranscriptAssistantTextKind::Body,
                        &data.delta,
                    );
                } else {
                    let pending =
                        pending_pre_tool_stream.get_or_insert_with(|| (event.seq, String::new()));
                    pending.1.push_str(&data.delta);
                }
            }
            harness_core::event::EventV1::AssistantMessageFinished(data)
                if provider_event_matches_activity(
                    event,
                    data.request_id.as_str(),
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                flush_pending_pre_tool_stream(
                    &mut parts,
                    &mut next_index,
                    &mut pending_pre_tool_stream,
                    activity,
                    thinking_visible,
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                settle_trailing_body(&mut parts);
                for committed_part in &data.parts {
                    match committed_part {
                        harness_core::session::AssistantPart::Reasoning { text }
                            if thinking_visible =>
                        {
                            saw_reasoning_event = true;
                            push_sequenced_text_part(
                                &mut parts,
                                &mut next_index,
                                event.seq,
                                TranscriptAssistantTextKind::Reasoning,
                                text,
                            );
                        }
                        harness_core::session::AssistantPart::Reasoning { .. } => {}
                        harness_core::session::AssistantPart::Text { text } => {
                            saw_body_event = true;
                            push_sequenced_text_part(
                                &mut parts,
                                &mut next_index,
                                event.seq,
                                TranscriptAssistantTextKind::Body,
                                text,
                            );
                        }
                        harness_core::session::AssistantPart::ToolCall(tool_call) => {
                            saw_tool_call = true;
                            settle_trailing_body(&mut parts);
                            if let Some(tool_call) =
                                pending_tool_calls.remove(tool_call.tool_call_id.as_str())
                            {
                                parts.push(SequencedTranscriptAssistantPart {
                                    seq: event.seq,
                                    index: next_index,
                                    part: TranscriptAssistantPart::ToolCall(Box::new(
                                        tool_call.section,
                                    )),
                                });
                                next_index += 1;
                            }
                        }
                    }
                }
                settle_all_streaming_bodies(&mut parts);
            }
            harness_core::event::EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(activity.request_id.as_str()) =>
            {
                saw_turn_event = true;
                flush_pending_pre_tool_stream(
                    &mut parts,
                    &mut next_index,
                    &mut pending_pre_tool_stream,
                    activity,
                    thinking_visible,
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                if crate::app::task_completed_updates_assistant_transcript(data)
                    && !saw_body_event
                    && has_trimmed_content(&data.result_summary)
                {
                    saw_body_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        TranscriptAssistantTextKind::Body,
                        &data.result_summary,
                    );
                }
                settle_trailing_body(&mut parts);
            }
            harness_core::event::EventV1::ToolCallRequested(data)
                if event.correlation_id.as_deref() == Some(activity.request_id.as_str()) =>
            {
                saw_turn_event = true;
                saw_tool_call = true;
                flush_pending_pre_tool_stream(
                    &mut parts,
                    &mut next_index,
                    &mut pending_pre_tool_stream,
                    activity,
                    thinking_visible,
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                settle_trailing_body(&mut parts);
                if let Some(tool_call) = pending_tool_calls.remove(data.tool_call_id.as_str()) {
                    parts.push(SequencedTranscriptAssistantPart {
                        seq: event.seq,
                        index: next_index,
                        part: TranscriptAssistantPart::ToolCall(Box::new(tool_call.section)),
                    });
                    next_index += 1;
                }
            }
            _ => {}
        }
    }

    if !saw_turn_event {
        return Vec::new();
    }

    flush_pending_pre_tool_stream(
        &mut parts,
        &mut next_index,
        &mut pending_pre_tool_stream,
        activity,
        thinking_visible,
        &mut saw_reasoning_event,
        &mut saw_body_event,
    );

    if !saw_reasoning_event && !saw_body_event && activity_has_thinking_text(activity) {
        parts.push(SequencedTranscriptAssistantPart {
            seq: activity.first_seq,
            index: next_index,
            part: TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: activity.thinking_text.clone(),
            }),
        });
        next_index += 1;
    }

    if !saw_body_event && !activity.transcript_text.is_empty() {
        let body = match activity.status {
            ActivityStatus::Streaming => {
                TranscriptBodyBlock::StreamingRichText(activity.transcript_text.clone())
            }
            ActivityStatus::Queued | ActivityStatus::Done | ActivityStatus::Error => {
                TranscriptBodyBlock::RichText(activity.transcript_text.clone())
            }
        };
        parts.push(SequencedTranscriptAssistantPart {
            seq: activity.last_seq,
            index: next_index,
            part: TranscriptAssistantPart::Body(body),
        });
        next_index += 1;
    }

    for tool_call in pending_tool_calls.into_values() {
        parts.push(SequencedTranscriptAssistantPart {
            seq: tool_call.first_seq,
            index: next_index,
            part: TranscriptAssistantPart::ToolCall(Box::new(tool_call.section)),
        });
        next_index += 1;
    }

    if !matches!(activity.status, ActivityStatus::Streaming) {
        settle_all_streaming_bodies(&mut parts);
    }

    parts.sort_by_key(|part| (part.seq, part.index));
    parts
}

fn flush_pending_pre_tool_stream(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    next_index: &mut usize,
    pending_pre_tool_stream: &mut Option<(u64, String)>,
    activity: &ActivityEntry,
    thinking_visible: bool,
    saw_reasoning_event: &mut bool,
    saw_body_event: &mut bool,
) {
    let Some((seq, text)) = pending_pre_tool_stream.take() else {
        return;
    };

    if text.is_empty() {
        return;
    }

    let treat_as_reasoning =
        thinking_visible && activity_has_thinking_text(activity) && activity.thinking_text == text;

    if treat_as_reasoning {
        *saw_reasoning_event = true;
        push_sequenced_text_part(
            parts,
            next_index,
            seq,
            TranscriptAssistantTextKind::Reasoning,
            &text,
        );
    } else {
        *saw_body_event = true;
        push_sequenced_text_part(
            parts,
            next_index,
            seq,
            TranscriptAssistantTextKind::Body,
            &text,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptAssistantTextKind {
    Reasoning,
    Body,
}

fn push_sequenced_text_part(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    next_index: &mut usize,
    seq: u64,
    kind: TranscriptAssistantTextKind,
    text: &str,
) {
    if text.is_empty() {
        return;
    }

    if let Some(last) = parts.last_mut() {
        match (&mut last.part, kind) {
            (
                TranscriptAssistantPart::Reasoning(existing),
                TranscriptAssistantTextKind::Reasoning,
            ) => {
                existing.text.push_str(text);
                return;
            }
            (
                TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(existing)),
                TranscriptAssistantTextKind::Body,
            ) => {
                existing.push_str(text);
                return;
            }
            _ => {}
        }
    }

    let part = match kind {
        TranscriptAssistantTextKind::Reasoning => {
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: text.to_string(),
            })
        }
        TranscriptAssistantTextKind::Body => {
            TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text.to_string()))
        }
    };
    parts.push(SequencedTranscriptAssistantPart {
        seq,
        index: *next_index,
        part,
    });
    *next_index += 1;
}

fn settle_trailing_body(parts: &mut [SequencedTranscriptAssistantPart]) {
    let Some(TranscriptAssistantPart::Body(body)) = parts.last_mut().map(|part| &mut part.part)
    else {
        return;
    };
    if let TranscriptBodyBlock::StreamingRichText(text) = body {
        let text = std::mem::take(text);
        *body = TranscriptBodyBlock::RichText(text);
    }
}

fn settle_all_streaming_bodies(parts: &mut [SequencedTranscriptAssistantPart]) {
    for part in parts {
        let TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text)) =
            &mut part.part
        else {
            continue;
        };
        let text = std::mem::take(text);
        part.part = TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text));
    }
}

fn cancel_error_display_text(raw: &str, duration_ms: Option<u64>) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let is_cancel = lower.contains("interrupted")
        || lower.contains("cancelled")
        || lower.contains("canceled")
        || lower.contains("user cancel");
    if !is_cancel {
        return None;
    }
    let duration = match duration_ms {
        Some(ms) if ms >= 60_000 => format_duration_ms(ms),
        Some(ms) => {
            format!(
                "{:.1}s",
                f64::from(u32::try_from(ms).unwrap_or(u32::MAX)) / 1_000.0
            )
        }
        None => "0.0s".to_string(),
    };
    Some(format!("Turn cancelled by user in {duration}."))
}

#[cfg(test)]
mod ui10_tests {
    use super::*;

    fn successful_edit(id: &str, path: &str) -> crate::app::ToolCallEntry {
        let mut call = transcript_section_model_test_tool_call(id, "edit");
        call.canonical_tool_id = Some("edit".to_string());
        call.args_summary = serde_json::json!({
            "filePath": path,
            "oldString": "old\n",
            "newString": "new\n"
        })
        .to_string();
        call.status = ToolCallDisplayStatus::Succeeded;
        call
    }

    #[test]
    fn same_file_coalescing_accepts_only_trusted_successful_adjacent_edits() {
        // arrange
        let first = successful_edit("edit-1", "src/lib.rs");
        let second = successful_edit("edit-2", "src/lib.rs");
        assert!(safe_same_file_edit_pair(&first, &second));
        let mut failed = successful_edit("edit-3", "src/lib.rs");
        failed.status = ToolCallDisplayStatus::Failed;
        assert!(!safe_same_file_edit_pair(&second, &failed));
        assert!(!safe_same_file_edit_pair(
            &second,
            &successful_edit("edit-4", "src/main.rs")
        ));

        // act
        let mut first_write = successful_edit("write-1", "src/lib.rs");
        first_write.tool_id = "fs.write".to_string();
        first_write.canonical_tool_id = Some("fs.write".to_string());
        let mut duplicate_write = first_write.clone();
        duplicate_write.tool_call_id = "write-2".to_string();
        // assert
        assert!(safe_same_file_edit_pair(&first_write, &duplicate_write));
        duplicate_write.args_summary.push(' ');
        assert!(!safe_same_file_edit_pair(&first_write, &duplicate_write));
    }

    #[test]
    fn live_duplicate_writes_coalesce_under_first_identity_and_expand_as_a_group() {
        // arrange
        let run_dir = tempfile::tempdir().unwrap_or_abort();
        let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
        let mut activity = transcript_section_model_test_activity(
            "request-live-diff",
            ActivityStatus::Streaming,
            "",
        );
        let mut writes = Vec::new();
        for index in 0..3 {
            let mut write = successful_edit(&format!("write-{index}"), "demo.txt");
            write.tool_id = "fs.write".to_string();
            write.canonical_tool_id = Some("fs.write".to_string());
            write.args_summary =
                r#"{"path":"demo.txt","content":"consistency-diff-ok\n","oldContent":"old content\n"}"#
                    .to_string();
            writes.push(write);
        }
        activity.tool_calls = writes;
        app.activities = std::collections::VecDeque::from([activity]);

        let collapsed = build_transcript_sections(&app);
        let collapsed_tools = collapsed[0].assistant_tools().collect::<Vec<_>>();
        assert_eq!(collapsed_tools.len(), 1);
        assert_eq!(collapsed_tools[0].tool_call_id, "write-0");
        assert_eq!(
            collapsed_tools[0].coalesced_tool_call_ids,
            ["write-0", "write-1", "write-2"]
        );
        assert_eq!(collapsed_tools[0].header.title, "Edit demo.txt +1/-1");
        assert!(!collapsed_tools[0].details_visible());

        // act
        for id in ["write-0", "write-1", "write-2"] {
            app.toggle_tool_output_for_test(id);
        }
        let expanded = build_transcript_sections(&app);
        let expanded_tools = expanded[0].assistant_tools().collect::<Vec<_>>();
        // assert
        assert!(expanded_tools[0].details_visible());
        assert_eq!(expanded_tools[0].detail_blocks.len(), 1);
    }

    #[test]
    fn tool_lifecycle_upgrades_diff_highlight_without_replacing_identity_or_content() {
        // arrange
        // act
        let mut blocks = vec![TranscriptToolCallDetailBlock::StructuredDiff {
            diff_content: "--- src/lib.rs\n+++ src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
            fallback_path: Some("src/lib.rs".to_string()),
            force_stacked: false,
            plain_numbered: false,
            highlight_syntax: false,
            show_file_header: false,
        }];
        let before = blocks.clone();
        super::super::ui_transcript_tool_sections::set_diff_highlight_phase(&mut blocks, true);
        let TranscriptToolCallDetailBlock::StructuredDiff {
            diff_content: before_text,
            fallback_path: before_path,
            ..
        } = &before[0]
        else {
            panic!("structured before block");
        };
        let TranscriptToolCallDetailBlock::StructuredDiff {
            diff_content: after_text,
            fallback_path: after_path,
            highlight_syntax,
            ..
        } = &blocks[0]
        else {
            panic!("structured after block");
        };
        // assert
        assert!(*highlight_syntax);
        assert_eq!(before_text, after_text);
        assert_eq!(before_path, after_path);
    }
}
