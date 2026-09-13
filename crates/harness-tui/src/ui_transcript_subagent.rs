use super::*;
use crate::app::{OrchestrationTaskRow, ToolCallEntry};
use harness_core::event::{
    BackgroundTaskNotificationEvent, BackgroundTaskNotificationStatus, EventEnvelopeV1, EventV1,
};

pub(super) fn refresh_started_status(
    section: &mut TranscriptToolCallSection,
    tool: &ToolCallEntry,
    task: Option<&OrchestrationTaskRow>,
    app: &AppState,
) {
    if section.header.visual_style != TranscriptToolCallVisualStyle::TaskInline {
        return;
    }
    // A background launch finishing is not the child finishing. Its original
    // row stays in place, and the recorded terminal notification gets its own row.
    if section.subagent_background && tool.status != ToolCallDisplayStatus::Failed {
        if let Some(task) = task {
            if task.state.is_terminal() {
                section.header.presentation.status = ToolCallPresentationStatus::Succeeded;
                section.rail_motion = ToolRailMotion::Settled;
            } else {
                section.header.presentation.status = ToolCallPresentationStatus::Running;
                if app.transcript_motion_enabled() && !app.replay_mode {
                    section.rail_motion = ToolRailMotion::Running {
                        elapsed: std::time::Duration::from_millis(
                            u64::try_from(app.transcript_animation_phase())
                                .unwrap_or(u64::MAX)
                                .saturating_mul(crate::scheduling::active_animation_period_ms()),
                        ),
                        sampled_phase: app.transcript_animation_phase(),
                    };
                }
            }
        }
    }
}

pub(super) fn notification_sections(
    app: &AppState,
) -> std::collections::BTreeMap<&str, Vec<TranscriptOrderedToolCallSection>> {
    let mut sections = std::collections::BTreeMap::<_, Vec<_>>::new();
    for event in &app.events {
        let EventV1::BackgroundTaskNotification(data) = &event.payload else {
            continue;
        };
        let request = data
            .delivered_turn_request_id
            .as_deref()
            .unwrap_or(&data.child_request_id);
        sections
            .entry(request)
            .or_default()
            .push(notification_section(app, event, data));
    }
    sections
}

fn notification_section(
    app: &AppState,
    event: &EventEnvelopeV1,
    data: &BackgroundTaskNotificationEvent,
) -> TranscriptOrderedToolCallSection {
    let launch = app
        .activities
        .iter()
        .flat_map(|activity| &activity.tool_calls)
        .find(|tool| {
            matches!(tool.effective_tool_id(), "agent.spawn" | "task")
                && (task_tool_child_session_id(tool) == Some(data.child_session_id.as_str())
                    || app
                        .transcript_task_row_for_tool_call(tool)
                        .is_some_and(|row| row.task_id == data.task_id.as_str()))
        });
    let duration = launch.and_then(|tool| event.mono_ms.checked_sub(tool.first_mono_ms));
    let verb = match data.status {
        BackgroundTaskNotificationStatus::Completed => "completed",
        BackgroundTaskNotificationStatus::Cancelled => "cancelled",
        BackgroundTaskNotificationStatus::Failed | BackgroundTaskNotificationStatus::TimedOut => {
            "failed"
        }
    };
    let elapsed = duration
        .map(|ms| format!(" in {}", format_duration(ms)))
        .unwrap_or_default();
    let title = format!(
        "Subagent {verb}{elapsed}: “{}”",
        collapse_inline_whitespace(&data.description)
    );
    let tool_call_id = format!("background-notification:{}", event.event_id);
    let status = match data.status {
        BackgroundTaskNotificationStatus::Completed => ToolCallPresentationStatus::Succeeded,
        BackgroundTaskNotificationStatus::Cancelled => ToolCallPresentationStatus::Cancelled,
        BackgroundTaskNotificationStatus::Failed | BackgroundTaskNotificationStatus::TimedOut => {
            ToolCallPresentationStatus::Failed
        }
    };
    let mut group = TranscriptToolGroupMember {
        expanded: app.tool_group_expanded(&tool_call_id),
        ..TranscriptToolGroupMember::default()
    };
    group.sources.insert(data.child_session_id.to_string());
    let selected = super::ui_transcript_tool_sections::tool_header_selected(app, &tool_call_id);
    TranscriptOrderedToolCallSection {
        tool_call_id: tool_call_id.clone(),
        first_seq: event.seq,
        section: TranscriptToolCallSection {
            group,
            hook_executions: Vec::new(),
            tool_call_id: tool_call_id.clone(),
            coalesced_tool_call_ids: vec![tool_call_id],
            child_session_id: Some(data.child_session_id.to_string()),
            subagent_background: true,
            output_truncated: false,
            replay_read_only: app.replay_mode,
            hovered_target: app.hovered_transcript_target().cloned(),
            header: TranscriptToolCallHeader {
                selected,
                tool_id: "agent.spawn".into(),
                title,
                subtitle: None,
                path_metadata: None,
                icon: None,
                presentation: ToolCallPresentation {
                    status,
                    duration_ms: duration,
                    result_count: None,
                },
                visual_style: TranscriptToolCallVisualStyle::TaskInline,
                struck_out: false,
                disclosure_state: None,
            },
            detail_blocks: Vec::new(),
            details_collapsed_by_default: false,
            details_preview_visible: false,
            animation_phase: app.transcript_animation_phase(),
            expanded: false,
            rail_motion: ToolRailMotion::Settled,
        },
    }
}

fn format_duration(ms: u64) -> String {
    let duration = std::time::Duration::from_millis(ms);
    let secs = duration.as_secs();
    if secs < 10 {
        format!("{:.1}s", duration.as_secs_f64())
    } else if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, secs % 3600 / 60)
    }
}
