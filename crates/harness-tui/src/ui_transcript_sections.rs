use super::ui_transcript_tool_sections::build_tool_call_section;
use super::*;

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
            is_selected: *activity_index == app.transcript_view.selected_activity_index,
            is_latest: false,
            thinking_visible: app.transcript_thinking_visible(),
            timestamps_visible: app.transcript_timestamps_visible(),
            show_tool_details: app.tool_details_visible(),
            show_generic_tool_output: app.generic_tool_output_visible(),
            stacked_diffs: app.stacked_transcript_diffs(),
            session_path: app.session_path.as_deref(),
            app,
        }));
    }

    if let Some(latest_assistant_footer_index) = turn_sections
        .iter()
        .rposition(turn_supports_assistant_footer)
    {
        let original_activity_index = visible_activities[latest_assistant_footer_index].0;
        if let Some(turn) = turn_sections.get_mut(latest_assistant_footer_index) {
            turn.show_footer = true;
            turn.footer_timestamp = app
                .transcript_timestamps_visible()
                .then_some(
                    app.activities[original_activity_index]
                        .user_timestamp
                        .as_deref(),
                )
                .flatten()
                .map(short_time_or_trimmed);
        }
    }

    turn_sections
}

fn turn_supports_assistant_footer(turn: &TranscriptTurnSection) -> bool {
    !turn.assistant_parts.is_empty() || activity_status_supports_footer_only(turn.header.status)
}

fn build_turn_section(args: BuildTurnSectionArgs<'_>) -> TranscriptTurnSection {
    let BuildTurnSectionArgs {
        activity,
        queued_user_message,
        is_selected,
        is_latest,
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
        TranscriptLabeledTextSection {
            label: THINKING_TRACE_LABEL,
            text: activity.thinking_text.clone(),
        }
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
        show_footer: is_latest,
        footer_timestamp: (is_latest && timestamps_visible)
            .then_some(activity.user_timestamp.as_deref())
            .flatten()
            .map(short_time_or_trimmed),
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
        parts[first_reasoning_index] =
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: activity.thinking_text.clone(),
            });
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
                        TranscriptAssistantTextKind::Body,
                        &data.delta,
                    );
                } else {
                    let pending =
                        pending_pre_tool_stream.get_or_insert_with(|| (event.seq, String::new()));
                    pending.1.push_str(&data.delta);
                }
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
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: text.to_string(),
            })
        }
        TranscriptAssistantTextKind::Body => {
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
