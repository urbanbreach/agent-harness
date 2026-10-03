use super::*;
use crate::app::{OrchestrationTaskRow, OrchestrationTaskState, ToolCallEntry};
use harness_core::event::{
    BackgroundTaskNotificationEvent, BackgroundTaskNotificationStatus, EventEnvelopeV1, EventV1,
};

pub(super) fn refresh_status(
    section: &mut TranscriptToolCallSection,
    tool: &ToolCallEntry,
    task: Option<&OrchestrationTaskRow>,
    app: &AppState,
) {
    if section.header.visual_style != TranscriptToolCallVisualStyle::TaskInline {
        return;
    }
    use ToolCallPresentationStatus as Status;
    let projection = app
        .native_subagent_task(&tool.tool_call_id)
        .is_none()
        .then(|| app.subagent_request_projection(tool))
        .flatten();
    let (status, verb) = if let Some(projection) = projection.as_ref() {
        match projection.status.as_str() {
            "completed" => (Status::Succeeded, "completed"),
            "cancelled" => (Status::Cancelled, "cancelled"),
            "failed" => (Status::Failed, "failed"),
            "timed_out" => (Status::Failed, "timed out"),
            "running" => (Status::Running, "running"),
            _ => (Status::Queued, "queued"),
        }
    } else if tool.status == ToolCallDisplayStatus::Failed {
        (Status::Failed, "failed")
    } else if tool.status == ToolCallDisplayStatus::PendingPermission {
        (Status::Waiting, "waiting for approval")
    } else if let Some(task) = task.filter(|task| {
        section.subagent_background
            || tool.status != ToolCallDisplayStatus::Succeeded
            || task.state.is_terminal()
    }) {
        match task.state {
            OrchestrationTaskState::Queued => (Status::Queued, "queued"),
            OrchestrationTaskState::Running => (Status::Running, "running"),
            OrchestrationTaskState::Stale => (Status::Waiting, "stalled"),
            OrchestrationTaskState::Completed => (Status::Succeeded, "completed"),
            OrchestrationTaskState::Cancelled => (Status::Cancelled, "cancelled"),
            OrchestrationTaskState::Failed => (Status::Failed, "failed"),
            OrchestrationTaskState::TimedOut => (Status::Failed, "timed out"),
            OrchestrationTaskState::LateResult => (Status::Succeeded, "completed late"),
        }
    } else {
        match tool.status {
            ToolCallDisplayStatus::Queued => (Status::Queued, "queued"),
            ToolCallDisplayStatus::PendingPermission => (Status::Waiting, "waiting for approval"),
            ToolCallDisplayStatus::Running => (Status::Running, "running"),
            ToolCallDisplayStatus::Succeeded if section.subagent_background => {
                (Status::Running, "running")
            }
            ToolCallDisplayStatus::Succeeded => (Status::Succeeded, "completed"),
            ToolCallDisplayStatus::Failed => (Status::Failed, "failed"),
        }
    };
    section.header.presentation.status = status;
    section.header.title = agent_spawn_title(
        agent_spawn_description(tool),
        verb,
        task.filter(|_| status == Status::Running)
            .and_then(|row| row.current_child_tool_title.as_deref()),
    );
    if let Some(row) = app
        .subagents
        .rows
        .values()
        .find(|row| row.parent_tool == tool.tool_call_id)
    {
        let terminal = task.is_some_and(|task| task.state.is_terminal());
        let title = if terminal && !row.background {
            format!(
                "Subagent {verb} in {}: “{}”",
                crate::app::subagents::duration_label(
                    task.and_then(|task| task.timing_elapsed_ms).unwrap_or(0)
                ),
                collapse_inline_whitespace(&row.description)
            )
        } else {
            let activity = task
                .filter(|task| !task.state.is_terminal())
                .and_then(|task| task.current_child_tool_title.as_deref())
                .map_or_else(String::new, |activity| format!(" · {activity}"));
            format!(
                "Subagent {}: “{}”{activity}{}",
                if row.background { "started" } else { "running" },
                collapse_inline_whitespace(&row.description),
                row.meta
            )
        };
        section.header.title = title;
        section.header.subtitle = None;
        if terminal && row.background {
            section.header.presentation.status = Status::Succeeded;
        }
    }
    section.rail_motion = match status {
        Status::Running if app.transcript_motion_enabled() && app.presentation_is_live() => {
            ToolRailMotion::Running {
                elapsed: std::time::Duration::from_millis(
                    u64::try_from(app.transcript_animation_phase())
                        .unwrap_or(u64::MAX)
                        .saturating_mul(crate::scheduling::active_animation_period_ms()),
                ),
                sampled_phase: app.transcript_animation_phase(),
            }
        }
        Status::Queued => ToolRailMotion::Queued,
        Status::Waiting => ToolRailMotion::Waiting,
        _ => ToolRailMotion::Settled,
    };
}

pub(super) fn notification_sections(
    app: &AppState,
) -> std::collections::BTreeMap<&str, Vec<TranscriptOrderedToolCallSection>> {
    let mut sections = std::collections::BTreeMap::<_, Vec<_>>::new();
    for event in app.events() {
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
    for row in app.subagents.rows.values().filter(|row| row.background) {
        let Some(record) = app
            .subagents
            .history
            .records
            .get(&row.id)
            .filter(|record| record.lifecycle.is_finished())
        else {
            continue;
        };
        let Some(outcome) = record.outcome else {
            continue;
        };
        let Some(event) = app.events().find(|event| matches!(&event.payload, EventV1::SubagentTransition(data) if data.child_id.0 == row.id && data.generation == record.generation && data.transition == harness_core::subagent::SubagentTransitionKind::Finished && (data.attempt_id.is_none() || data.attempt_id.as_deref() == record.lifecycle.current_attempt_id()))) else { continue };
        let delegated = app.delegated_child_request_ids_for_parent_view(app.current_session_id());
        let current_request = app
            .activities
            .iter()
            .rev()
            .find(|activity| {
                activity.first_seq <= event.seq && !delegated.contains(activity.request_id.as_str())
            })
            .map(|activity| activity.request_id.as_str());
        let Some(request) = current_request
            .or(row.parent_request.as_deref())
            .or_else(|| {
                app.activities
                    .iter()
                    .find(|activity| {
                        activity
                            .tool_calls
                            .iter()
                            .any(|tool| tool.tool_call_id == row.parent_tool)
                    })
                    .map(|activity| activity.request_id.as_str())
            })
        else {
            continue;
        };
        use harness_core::subagent::SubagentTerminalOutcome as Outcome;
        let (verb, status) = match outcome {
            Outcome::Completed | Outcome::StationarityEnded => {
                ("completed", ToolCallPresentationStatus::Succeeded)
            }
            Outcome::Cancelled | Outcome::RemovedFromQueue => {
                ("cancelled", ToolCallPresentationStatus::Cancelled)
            }
            _ => ("failed", ToolCallPresentationStatus::Failed),
        };
        let duration = record.accounting.map(|accounting| accounting.duration_ms);
        sections.entry(request).or_default().push(terminal_section(
            app,
            TerminalRow {
                id: format!("background-notification:{}:{}", row.id, record.generation),
                child: row.id.clone(),
                seq: event.seq,
                title: format!(
                    "Subagent {verb} in {}: “{}”",
                    crate::app::subagents::duration_label(duration.unwrap_or(0)),
                    collapse_inline_whitespace(&row.description)
                ),
                subtitle: None,
                status,
                duration,
            },
        ));
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
            matches!(
                tool.effective_tool_id(),
                "spawn_subagent" | "agent.spawn" | "task"
            ) && app
                .subagent_request_projection(tool)
                .is_some_and(|projection| projection.request_id.as_str() == data.child_request_id)
        });
    let duration = launch.and_then(|tool| event.mono_ms.checked_sub(tool.first_mono_ms));
    let verb = match data.status {
        BackgroundTaskNotificationStatus::Completed => "completed",
        BackgroundTaskNotificationStatus::Cancelled => "cancelled",
        BackgroundTaskNotificationStatus::Failed => "failed",
        BackgroundTaskNotificationStatus::TimedOut => "timed out",
    };
    let elapsed = duration
        .map(|ms| format!(" · {}", format_duration(ms)))
        .unwrap_or_default();
    let title = format!(
        "Subagent {verb}: “{}”",
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
    terminal_section(
        app,
        TerminalRow {
            id: tool_call_id,
            child: data.child_session_id.to_string(),
            seq: event.seq,
            title,
            subtitle: Some(format!(
                "{}{elapsed}",
                launch
                    .map(|tool| agent_spawn_subtitle(tool, app))
                    .unwrap_or_default()
            )),
            status,
            duration,
        },
    )
}

struct TerminalRow {
    id: String,
    child: String,
    seq: u64,
    title: String,
    subtitle: Option<String>,
    status: ToolCallPresentationStatus,
    duration: Option<u64>,
}

fn terminal_section(app: &AppState, row: TerminalRow) -> TranscriptOrderedToolCallSection {
    let TerminalRow {
        id: tool_call_id,
        child,
        seq,
        title,
        subtitle,
        status,
        duration,
    } = row;
    let mut group = TranscriptToolGroupMember {
        expanded: app.tool_group_expanded(&tool_call_id),
        ..TranscriptToolGroupMember::default()
    };
    group.sources.insert(child.clone());
    let selected = super::ui_transcript_tool_sections::tool_header_selected(app, &tool_call_id);
    TranscriptOrderedToolCallSection {
        tool_call_id: tool_call_id.clone(),
        first_seq: seq,
        section: TranscriptToolCallSection {
            group,
            hook_executions: Vec::new(),
            tool_call_id: tool_call_id.clone(),
            coalesced_tool_call_ids: vec![tool_call_id],
            child_session_id: Some(child),
            subagent_background: true,
            output_truncated: false,
            replay_read_only: app.replay_mode,
            hovered_target: app.hovered_transcript_target().cloned(),
            header: TranscriptToolCallHeader {
                selected,
                tool_id: "background.notification".into(),
                title,
                subtitle,
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
            cancellation_requested: false,
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
