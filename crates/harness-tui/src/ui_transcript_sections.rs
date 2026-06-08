use super::ui_transcript_tool_sections::build_tool_call_section;
use super::*;
use crate::text::has_trimmed_content;

pub(super) fn build_transcript_sections(app: &AppState) -> Vec<TranscriptTurnSection> {
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
            activity,
            queued_user_message: pending_assistant_index
                .is_some_and(|pending| visible_index > pending),
            is_selected: *activity_index == app.selected_activity_index,
            thinking_visible: app.transcript_thinking_visible(),
            timestamps_visible: app.transcript_timestamps_visible(),
            show_tool_details: app.tool_details_visible(),
            show_generic_tool_output: app.generic_tool_output_visible(),
            stacked_diffs: app.stacked_transcript_diffs(),
            session_path: app.session_path.as_deref(),
            app,
        }));
    }

    turn_sections
}

fn build_turn_section(args: BuildTurnSectionArgs<'_>) -> TranscriptTurnSection {
    let BuildTurnSectionArgs {
        activity,
        queued_user_message,
        is_selected,
        thinking_visible,
        timestamps_visible,
        show_tool_details,
        show_generic_tool_output,
        stacked_diffs,
        session_path,
        app,
    } = args;

    let user_message =
        activity
            .user_message
            .as_ref()
            .map(|user_msg| TranscriptUserMessageSection {
                text: user_msg.text.clone(),
                queued: queued_user_message,
            });

    let thinking = (thinking_visible && activity_has_thinking_text(activity)).then(|| {
        reasoning_section(
            activity.thinking_text.clone(),
            activity.status,
            Some(activity.first_mono_ms),
            reasoning_duration_for_status(activity.status, activity.duration_ms()),
        )
    });

    let mut body_blocks = Vec::new();
    if !activity.transcript_text.is_empty() {
        body_blocks.push(TranscriptBodyBlock::RichText(
            activity.transcript_text.clone(),
        ));
    }

    let ordered_tool_calls = activity
        .tool_calls
        .iter()
        .filter_map(|tool_call| {
            build_tool_call_section(
                tool_call,
                app,
                show_tool_details,
                timestamps_visible,
                show_generic_tool_output,
                app.tool_output_expanded(tool_call),
                stacked_diffs,
                session_path,
            )
            .map(|section| TranscriptOrderedToolCallSection {
                tool_call_id: tool_call.tool_call_id.clone(),
                first_seq: tool_call.first_seq,
                section,
            })
        })
        .collect::<Vec<_>>();
    let tool_calls = ordered_tool_calls
        .iter()
        .map(|tool_call| tool_call.section.clone())
        .collect::<Vec<_>>();
    let error = activity
        .error_message
        .as_ref()
        .map(|text| TranscriptErrorSection { text: text.clone() });
    let assistant_parts = build_ordered_assistant_parts(
        activity,
        app,
        thinking_visible,
        thinking.clone(),
        body_blocks.clone(),
        ordered_tool_calls,
        error.clone(),
    );

    TranscriptTurnSection {
        request_id: activity.request_id.clone(),
        user_message,
        animation_phase: app.transcript_animation_phase(),
        header: TranscriptTurnHeader {
            status: activity.status,
            is_selected,
            profile_label: activity.profile_label.clone(),
            model_id: activity.model_id.clone(),
            duration_ms: activity.duration_ms(),
        },
        body_blocks,
        tool_calls,
        thinking,
        error,
        assistant_parts,
        subagent_hint_key: app.keymap.get_binding_str(Action::SessionChildFirst),
    }
}

fn build_ordered_assistant_parts(
    activity: &ActivityEntry,
    app: &AppState,
    thinking_visible: bool,
    thinking: Option<TranscriptLabeledTextSection>,
    body_blocks: Vec<TranscriptBodyBlock>,
    ordered_tool_calls: Vec<TranscriptOrderedToolCallSection>,
    error: Option<TranscriptErrorSection>,
) -> Vec<TranscriptAssistantPart> {
    let mut event_parts = build_ordered_assistant_parts_from_events(
        activity,
        app,
        &ordered_tool_calls,
        thinking_visible,
    );
    if event_parts.is_empty() {
        let mut fallback_parts = Vec::new();
        if let Some(thinking) = thinking {
            fallback_parts.push(TranscriptAssistantPart::Reasoning(thinking));
        }
        fallback_parts.extend(body_blocks.into_iter().map(TranscriptAssistantPart::Body));
        fallback_parts.extend(
            ordered_tool_calls
                .into_iter()
                .map(|tool_call| TranscriptAssistantPart::ToolCall(Box::new(tool_call.section))),
        );
        insert_subagent_hint_after_task_tools(&mut fallback_parts);
        if let Some(error) = error {
            fallback_parts.push(TranscriptAssistantPart::Error(error));
        }
        return fallback_parts;
    }

    sync_reasoning_parts_with_activity(&mut event_parts, activity, thinking_visible);
    insert_subagent_hint_after_task_tools(&mut event_parts);

    if let Some(error) = error {
        event_parts.push(TranscriptAssistantPart::Error(error));
    }
    event_parts
}

fn insert_subagent_hint_after_task_tools(parts: &mut Vec<TranscriptAssistantPart>) {
    if parts
        .iter()
        .any(|part| matches!(part, TranscriptAssistantPart::SubagentHint))
    {
        return;
    }
    let has_task_tool = parts.iter().any(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if matches!(tool_call.header.tool_id.as_str(), "agent.spawn" | "task")
        )
    });
    if !has_task_tool {
        return;
    }
    let insert_at = parts
        .iter()
        .position(|part| matches!(part, TranscriptAssistantPart::Error(_)))
        .unwrap_or(parts.len());
    parts.insert(insert_at, TranscriptAssistantPart::SubagentHint);
}

fn sync_reasoning_parts_with_activity(
    parts: &mut Vec<TranscriptAssistantPart>,
    activity: &ActivityEntry,
    thinking_visible: bool,
) {
    if !thinking_visible {
        parts.retain(|part| !matches!(part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }

    if !activity_has_thinking_text(activity) {
        parts.retain(|part| !matches!(part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }

    let reasoning_indices = parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            matches!(part, TranscriptAssistantPart::Reasoning(_)).then_some(index)
        })
        .collect::<Vec<_>>();

    let Some(first_reasoning_index) = reasoning_indices.first().copied() else {
        return;
    };

    if reasoning_indices.len() == 1 {
        if let Some(TranscriptAssistantPart::Reasoning(reasoning)) =
            parts.get_mut(first_reasoning_index)
        {
            reasoning.text = activity.thinking_text.clone();
        }
        return;
    }

    let rendered = reasoning_indices
        .iter()
        .filter_map(|index| match parts.get(*index) {
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
            if let Some(TranscriptAssistantPart::Reasoning(reasoning)) =
                parts.get_mut(last_reasoning_index)
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

fn build_ordered_assistant_parts_from_events(
    activity: &ActivityEntry,
    app: &AppState,
    ordered_tool_calls: &[TranscriptOrderedToolCallSection],
    thinking_visible: bool,
) -> Vec<TranscriptAssistantPart> {
    let mut parts = Vec::new();
    let mut next_index = 0usize;
    let mut saw_turn_event = false;
    let mut saw_reasoning_event = false;
    let mut saw_body_event = false;
    let mut saw_tool_call = false;
    let mut pending_pre_tool_stream: Option<(u64, u64, String)> = None;
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
                    &data.request_id,
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                if thinking_visible {
                    saw_reasoning_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        event.mono_ms,
                        TranscriptAssistantTextKind::Reasoning,
                        &data.delta,
                    );
                }
            }
            harness_core::event::EventV1::ProviderStreamDelta(data)
                if provider_event_matches_activity(
                    event,
                    &data.request_id,
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
                        event.mono_ms,
                        TranscriptAssistantTextKind::Body,
                        &data.delta,
                    );
                } else {
                    let pending = pending_pre_tool_stream
                        .get_or_insert_with(|| (event.seq, event.mono_ms, String::new()));
                    pending.2.push_str(&data.delta);
                }
            }
            harness_core::event::EventV1::ProviderRequestFinished(data)
                if provider_event_matches_activity(
                    event,
                    &data.request_id,
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                record_flushed_pending_stream(
                    flush_pending_pre_tool_stream(
                        &mut parts,
                        &mut next_index,
                        &mut pending_pre_tool_stream,
                        activity,
                        thinking_visible,
                        event.mono_ms,
                    ),
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                finalize_open_reasoning_part(&mut parts, event.mono_ms, activity.status);
            }
            harness_core::event::EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(activity.request_id.as_str()) =>
            {
                saw_turn_event = true;
                record_flushed_pending_stream(
                    flush_pending_pre_tool_stream(
                        &mut parts,
                        &mut next_index,
                        &mut pending_pre_tool_stream,
                        activity,
                        thinking_visible,
                        event.mono_ms,
                    ),
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                finalize_open_reasoning_part(&mut parts, event.mono_ms, activity.status);
                if crate::app::task_completed_updates_assistant_transcript(data)
                    && !saw_body_event
                    && has_trimmed_content(&data.result_summary)
                {
                    saw_body_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        event.mono_ms,
                        TranscriptAssistantTextKind::Body,
                        &data.result_summary,
                    );
                }
            }
            harness_core::event::EventV1::ToolCallRequested(data)
                if event.correlation_id.as_deref() == Some(activity.request_id.as_str()) =>
            {
                saw_turn_event = true;
                saw_tool_call = true;
                record_flushed_pending_stream(
                    flush_pending_pre_tool_stream(
                        &mut parts,
                        &mut next_index,
                        &mut pending_pre_tool_stream,
                        activity,
                        thinking_visible,
                        event.mono_ms,
                    ),
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                finalize_open_reasoning_part(&mut parts, event.mono_ms, ActivityStatus::Done);
                if let Some(tool_call) = pending_tool_calls.remove(&data.tool_call_id) {
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

    record_flushed_pending_stream(
        flush_pending_pre_tool_stream(
            &mut parts,
            &mut next_index,
            &mut pending_pre_tool_stream,
            activity,
            thinking_visible,
            activity.last_mono_ms,
        ),
        &mut saw_reasoning_event,
        &mut saw_body_event,
    );
    if matches!(
        activity.status,
        ActivityStatus::Done | ActivityStatus::Error
    ) {
        finalize_open_reasoning_part(&mut parts, activity.last_mono_ms, activity.status);
    }

    if !saw_reasoning_event && !saw_body_event && activity_has_thinking_text(activity) {
        parts.push(SequencedTranscriptAssistantPart {
            seq: activity.first_seq,
            index: next_index,
            part: TranscriptAssistantPart::Reasoning(reasoning_section(
                activity.thinking_text.clone(),
                activity.status,
                Some(activity.first_mono_ms),
                reasoning_duration_for_status(activity.status, activity.duration_ms()),
            )),
        });
        next_index += 1;
    }

    if !saw_body_event && !activity.transcript_text.is_empty() {
        parts.push(SequencedTranscriptAssistantPart {
            seq: activity.last_seq,
            index: next_index,
            part: TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(
                activity.transcript_text.clone(),
            )),
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

    parts.sort_by_key(|part| (part.seq, part.index));
    parts.into_iter().map(|part| part.part).collect()
}

fn flush_pending_pre_tool_stream(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    next_index: &mut usize,
    pending_pre_tool_stream: &mut Option<(u64, u64, String)>,
    activity: &ActivityEntry,
    thinking_visible: bool,
    close_mono_ms: u64,
) -> Option<TranscriptAssistantTextKind> {
    let (seq, started_mono_ms, text) = pending_pre_tool_stream.take()?;

    if text.is_empty() {
        return None;
    }

    let treat_as_reasoning = activity_has_thinking_text(activity) && activity.thinking_text == text;

    if treat_as_reasoning {
        if thinking_visible {
            push_sequenced_text_part(
                parts,
                next_index,
                seq,
                started_mono_ms,
                TranscriptAssistantTextKind::Reasoning,
                &text,
            );
            finalize_open_reasoning_part(parts, close_mono_ms, ActivityStatus::Done);
        }
        Some(TranscriptAssistantTextKind::Reasoning)
    } else {
        push_sequenced_text_part(
            parts,
            next_index,
            seq,
            started_mono_ms,
            TranscriptAssistantTextKind::Body,
            &text,
        );
        Some(TranscriptAssistantTextKind::Body)
    }
}

fn record_flushed_pending_stream(
    flushed_kind: Option<TranscriptAssistantTextKind>,
    saw_reasoning_event: &mut bool,
    saw_body_event: &mut bool,
) {
    match flushed_kind {
        Some(TranscriptAssistantTextKind::Reasoning) => *saw_reasoning_event = true,
        Some(TranscriptAssistantTextKind::Body) => *saw_body_event = true,
        None => {}
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
    mono_ms: u64,
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
            ) if existing.status == ActivityStatus::Streaming => {
                existing.text.push_str(text);
                return;
            }
            (
                TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(existing)),
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
            TranscriptAssistantPart::Reasoning(reasoning_section(
                text.to_string(),
                ActivityStatus::Streaming,
                Some(mono_ms),
                None,
            ))
        }
        TranscriptAssistantTextKind::Body => {
            finalize_open_reasoning_part(parts, mono_ms, ActivityStatus::Done);
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text.to_string()))
        }
    };
    parts.push(SequencedTranscriptAssistantPart {
        seq,
        index: *next_index,
        part,
    });
    *next_index += 1;
}

fn finalize_open_reasoning_part(
    parts: &mut [SequencedTranscriptAssistantPart],
    finished_mono_ms: u64,
    fallback_status: ActivityStatus,
) {
    let Some(TranscriptAssistantPart::Reasoning(reasoning)) =
        parts.last_mut().map(|part| &mut part.part)
    else {
        return;
    };
    if reasoning.status != ActivityStatus::Streaming {
        return;
    }
    reasoning.status = if fallback_status == ActivityStatus::Error {
        ActivityStatus::Error
    } else {
        ActivityStatus::Done
    };
    reasoning.duration_ms = reasoning
        .started_mono_ms
        .and_then(|started| (finished_mono_ms >= started).then_some(finished_mono_ms - started));
}

fn reasoning_section(
    text: String,
    status: ActivityStatus,
    started_mono_ms: Option<u64>,
    duration_ms: Option<u64>,
) -> TranscriptLabeledTextSection {
    TranscriptLabeledTextSection {
        label: THINKING_TRACE_LABEL,
        text,
        status,
        started_mono_ms,
        duration_ms,
    }
}

fn reasoning_duration_for_status(status: ActivityStatus, duration_ms: Option<u64>) -> Option<u64> {
    matches!(status, ActivityStatus::Done | ActivityStatus::Error).then_some(duration_ms)?
}
