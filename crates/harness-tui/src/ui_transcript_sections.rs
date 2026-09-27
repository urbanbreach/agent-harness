use super::ui_transcript_parts::build_assistant_parts;
use super::ui_transcript_tool_sections::{build_tool_call_section, edit_tool_action};
use super::*;

pub(super) fn build_transcript_sections(app: &AppState) -> Vec<Arc<TranscriptTurnSection>> {
    prepare_transcript_sections(app, &[], 0)
}

pub(super) fn prepare_transcript_sections(
    app: &AppState,
    previous: &[Arc<TranscriptTurnSection>],
    dirty_from: usize,
) -> Vec<Arc<TranscriptTurnSection>> {
    let hidden_child_request_ids = hidden_delegated_child_request_ids(app);
    let mut notifications = super::ui_transcript_subagent::notification_sections(app);
    let notification_first_seqs = notifications
        .values()
        .flatten()
        .map(|row| row.first_seq)
        .collect::<std::collections::BTreeSet<_>>();
    let visible_activities = app
        .activities
        .iter()
        .enumerate()
        .filter(|(_, activity)| {
            !hidden_child_request_ids.contains(activity.request_id.as_str())
                || notification_first_seqs.contains(&activity.first_seq)
        })
        .collect::<Vec<_>>();
    let mut turn_sections = Vec::with_capacity(visible_activities.len());
    let pending_assistant_index = visible_activities
        .iter()
        .rposition(|(_, activity)| activity.status == ActivityStatus::Streaming);

    for (visible_index, (activity_index, activity)) in visible_activities.iter().enumerate() {
        if *activity_index < dirty_from {
            if let Some(section) = previous.get(visible_index).filter(|section| {
                (section.activity_first_seq, &section.request_id)
                    == (activity.first_seq, &activity.request_id)
            }) {
                turn_sections.push(Arc::clone(section));
                continue;
            }
        }
        turn_sections.push(Arc::new(build_turn_section(
            app,
            *activity_index,
            notifications
                .remove(activity.request_id.as_str())
                .unwrap_or_default(),
            activity.status == ActivityStatus::Queued
                && pending_assistant_index.is_some_and(|pending| visible_index > pending),
        )));
    }

    let latest_assistant_footer_index = turn_sections
        .iter()
        .rposition(|turn| turn_supports_assistant_footer(turn, app));
    for (index, turn) in turn_sections.iter_mut().enumerate() {
        let footer = Some(index) == latest_assistant_footer_index;
        if turn.show_footer != footer {
            let turn = Arc::make_mut(turn);
            turn.show_footer = footer;
            turn.footer_timestamp = footer
                .then(|| {
                    app.activities[visible_activities[index].0]
                        .user_timestamp
                        .as_deref()
                        .filter(|_| app.transcript_timestamps_visible())
                        .map(crate::time_format::wall_clock_12h)
                })
                .flatten();
        }
    }

    inject_compaction_events(app, &visible_activities, &mut turn_sections, dirty_from);

    turn_sections
}

fn inject_compaction_events(
    app: &AppState,
    visible_activities: &[(usize, &ActivityEntry)],
    turn_sections: &mut [Arc<TranscriptTurnSection>],
    dirty_from: usize,
) {
    for event in app.events() {
        let compaction_section = match &event.payload {
            harness_core::event::EventV1::SessionCompaction(data) => TranscriptCompactionSection {
                expanded: app.transcript_view.compaction_details_expanded,
                kind: TranscriptCompactionKind::SessionCompaction,
                summary: data.summary.clone(),
                tokens_before: Some(data.tokens_before),
                read_files: data.read_files.clone(),
                modified_files: data.modified_files.clone(),
            },
            harness_core::event::EventV1::BranchSummary(data) => TranscriptCompactionSection {
                expanded: app.transcript_view.compaction_details_expanded,
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

        if let Some(turn_index) =
            target_turn_index.filter(|index| visible_activities[*index].0 >= dirty_from)
        {
            if let Some(turn) = turn_sections.get_mut(turn_index) {
                let turn = Arc::make_mut(turn);
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

pub(super) fn events_for_activity<'a>(
    app: &'a AppState,
    activity: &ActivityEntry,
) -> impl DoubleEndedIterator<Item = &'a harness_core::event::EventEnvelopeV1> + Clone {
    // Durable history is ordered by sequence; unrelated turns need no scan.
    app.event_slices()
        .map(|events| {
            let start = events.partition_point(|event| event.seq < activity.first_seq);
            let end = events.partition_point(|event| event.seq <= activity.last_seq);
            events.get(start..end).unwrap_or_default()
        })
        .into_iter()
        .flatten()
}

fn build_turn_section(
    app: &AppState,
    activity_index: usize,
    notifications: Vec<TranscriptOrderedToolCallSection>,
    queued_user_message: bool,
) -> TranscriptTurnSection {
    let activity = &app.activities[activity_index];
    let timestamps_visible = app.transcript_timestamps_visible();
    let notification_created_activity = notifications
        .iter()
        .any(|row| row.first_seq == activity.first_seq);
    let user_message = activity
        .user_message
        .as_ref()
        .filter(|_| !notification_created_activity)
        .map(|user_msg| {
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

    let mut ordered_tool_calls: Vec<TranscriptOrderedToolCallSection> = Vec::new();
    for tool_call in &activity.tool_calls {
        let Some(section) = build_tool_call_section(
            tool_call,
            app,
            app.tool_details_visible(),
            timestamps_visible,
            app.generic_tool_output_visible(),
            app.tool_output_expanded(tool_call),
            app.stacked_transcript_diffs(),
            app.session_path.as_deref(),
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
                // The first recorded identity owns disclosure for repeated,
                // identical writes, including a fold made before a later duplicate.
                previous.section.details_collapsed_by_default = true;
                previous.section.details_preview_visible = false;
                previous.section.header.disclosure_state = Some(if previous.section.expanded {
                    TranscriptToolCallDisclosureState::Expanded
                } else {
                    TranscriptToolCallDisclosureState::Collapsed
                });
                previous.section.header.title = edit_tool_action(tool_call).to_string();
                previous.section.header.subtitle = None;
                continue;
            }
        }
        ordered_tool_calls.push(TranscriptOrderedToolCallSection {
            tool_call_id: tool_call.tool_call_id.clone(),
            first_seq: tool_call.first_seq,
            section,
        });
    }
    ordered_tool_calls.extend(notifications);
    let (assistant_parts, assistant_part_source_ids) =
        build_assistant_parts(app, activity, ordered_tool_calls);

    TranscriptTurnSection {
        activity_first_seq: activity.first_seq,
        request_id: activity.request_id.clone(),
        user_message,
        show_footer: false,
        footer_timestamp: activity
            .user_timestamp
            .as_deref()
            .filter(|_| timestamps_visible)
            .map(crate::time_format::wall_clock_12h),
        animation_phase: app.transcript_animation_phase(),
        motion_enabled: app.transcript_motion_enabled() && !app.replay_mode,
        reasoning_expanded: app.reasoning_expanded(&activity.request_id),
        header: TranscriptTurnHeader {
            status: activity.status,
            is_selected: transcript_surface_focused(app)
                && activity_index == app.transcript_view.selected_activity_index,
            is_hovered: matches!(
                app.hovered_transcript_target(),
                Some(TranscriptMouseTarget::Reasoning { request_id })
                    if request_id == &activity.request_id
            ),
            provider_request_open: events_for_activity(app, activity).rev().find_map(|event| {
                match &event.payload {
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
                }
            }) == Some(true),
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

#[cfg(test)]
#[path = "ui_transcript_sections_tests.rs"]
mod ui10_tests;
