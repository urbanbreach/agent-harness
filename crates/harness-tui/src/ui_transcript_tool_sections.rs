//! Tool rows are projected from recorded state before layout and painting.
use super::ui_tool_delegation::agent_spawn_is_background;
use super::ui_tool_visibility::{
    tool_call_has_transcript_disclosure, tool_error_hidden_inline, tool_output_is_viewer_only,
};
use super::*;
use crate::app::ToolCallEntry;

#[path = "ui_transcript_tool_sections/content.rs"]
mod content;
#[path = "ui_transcript_tool_sections/details.rs"]
mod details;
#[path = "ui_transcript_tool_sections/diffs.rs"]
mod diffs;
#[cfg(test)]
#[path = "ui_transcript_tool_sections/tests.rs"]
mod tests;

pub(super) use diffs::set_diff_highlight_phase;

#[expect(
    clippy::too_many_arguments,
    reason = "transcript tool-row assembly keeps the rendering toggles explicit at the call site"
)]
pub(super) fn build_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    show_tool_details: bool,
    timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> Option<TranscriptToolCallSection> {
    if tool_hidden_from_transcript(tool_call) {
        return None;
    }

    let task_row = app.transcript_task_row_for_tool_call(tool_call);

    let mut section = build_transcript_tool_call_section(
        tool_call,
        app,
        task_row.as_ref(),
        timestamps_visible,
        show_generic_tool_output,
        tool_output_expanded,
        stacked_diffs,
        session_path,
    );
    section.group.expanded = app.tool_group_expanded(&section.tool_call_id);
    if app.tool_output_previewed(&tool_call.tool_call_id) {
        section.expanded = false;
        section.header.disclosure_state = Some(TranscriptToolCallDisclosureState::Collapsed);
    }
    if let Some(child) = section
        .child_session_id
        .as_ref()
        .filter(|_| tool_family(&section) == TranscriptToolFamily::Task)
    {
        section.group.sources.insert(child.clone());
    } else if matches!(tool_call.effective_tool_id(), "search.web" | "websearch")
        && tool_call.status == ToolCallDisplayStatus::Succeeded
    {
        section.group.sources = super::super::ui_recorded_tool_output::web_sources(tool_call)
            .into_iter()
            .collect();
    }
    section.details_preview_visible |= show_tool_details
        && !section.detail_blocks.is_empty()
        && (app.tool_output_previewed(&tool_call.tool_call_id)
            || matches!(
                tool_family(&section),
                TranscriptToolFamily::Task | TranscriptToolFamily::Question
            )
            || matches!(
                tool_call.effective_tool_id(),
                "edit.hashline_apply" | "apply_patch" | "todo.write" | "todowrite" | "eval"
            ));
    // These reference blocks have no animated accent; only commands and task
    // lifecycle rows signal execution with a wave.
    section.cancellation_requested = app.inspected_child_cancel_started().is_some()
        && matches!(
            section.header.presentation.status,
            ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Running
        );
    if section.cancellation_requested {
        section.rail_motion = ToolRailMotion::Settled;
    }
    if matches!(section.rail_motion, ToolRailMotion::Running { .. })
        && matches!(
            tool_family(&section),
            TranscriptToolFamily::Read
                | TranscriptToolFamily::Edit
                | TranscriptToolFamily::Search
                | TranscriptToolFamily::List
        )
    {
        section.rail_motion = ToolRailMotion::Settled;
    }
    if !section.details_visible()
        && matches!(section.rail_motion, ToolRailMotion::FinishFlash { .. })
    {
        section.rail_motion = ToolRailMotion::Settled;
    }
    if (is_mcp_tool_id(&section.header.tool_id)
        || super::ui_tool_titles::generic_tool_id(&section.header.tool_id))
        && !section.expanded
        && !section.details_preview_visible
    {
        section.rail_motion = ToolRailMotion::Settled;
    }
    Some(section)
}

#[expect(
    clippy::too_many_arguments,
    reason = "explicit transcript display inputs"
)]
pub(super) fn build_transcript_tool_call_section(
    tool: &ToolCallEntry,
    app: &AppState,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    _timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> TranscriptToolCallSection {
    let id = tool.effective_tool_id();
    let viewer_only = tool_output_is_viewer_only(tool);
    let expanded = tool_output_expanded && !viewer_only;
    let visible = show_generic_tool_output || expanded;
    let collapsed = viewer_only || tool_call_has_transcript_disclosure(tool);
    let mut row = TranscriptToolCallSection {
        group: Default::default(),
        hook_executions: tool.hook_executions.clone(),
        tool_call_id: tool.tool_call_id.clone(),
        coalesced_tool_call_ids: vec![tool.tool_call_id.clone()],
        child_session_id: task_tool_child_session_id(tool)
            .or_else(|| task_row.and_then(|task| task.child_session_id.as_deref()))
            .map(str::to_owned),
        subagent_background: matches!(id, "spawn_subagent" | "agent.spawn" | "task")
            && app
                .subagents
                .rows
                .values()
                .find(|row| row.parent_tool == tool.tool_call_id)
                .map_or_else(|| agent_spawn_is_background(tool), |row| row.background),
        output_truncated: tool.truncated_output.is_some(),
        replay_read_only: app.replay_mode,
        hovered_target: app.hovered_transcript_target().cloned(),
        header: TranscriptToolCallHeader {
            selected: tool_header_selected(app, &tool.tool_call_id),
            tool_id: if matches!(id, "shell.run" | "bash") {
                id
            } else {
                &tool.tool_id
            }
            .to_owned(),
            title: String::new(),
            subtitle: None,
            path_metadata: None,
            icon: None,
            presentation: tool.presentation(),
            visual_style: generic_tool_visual_style(tool, visible),
            struck_out: tool_call_denied(tool),
            disclosure_state: (collapsed && !viewer_only).then_some(if expanded {
                TranscriptToolCallDisclosureState::Expanded
            } else {
                TranscriptToolCallDisclosureState::Collapsed
            }),
        },
        detail_blocks: Vec::new(),
        details_collapsed_by_default: collapsed,
        details_preview_visible: false,
        animation_phase: app.transcript_animation_phase(),
        expanded,
        rail_motion: ToolRailMotion::Settled,
        cancellation_requested: false,
    };
    let generic = content::populate(&mut row, tool, app, visible, stacked_diffs, session_path);
    details::finish(&mut row, tool, app, generic && visible, visible);
    content::subtitle(&mut row.header, tool, app, expanded, session_path);
    row.details_preview_visible = show_generic_tool_output && generic && !viewer_only;
    row.rail_motion = tool_rail_motion(tool, app, !row.detail_blocks.is_empty());
    super::ui_transcript_subagent::refresh_status(&mut row, tool, task_row, app);
    row
}

fn tool_rail_motion(
    tool: &crate::app::ToolCallEntry,
    app: &AppState,
    has_details: bool,
) -> ToolRailMotion {
    if !app.presentation_is_live() || !app.transcript_motion_enabled() {
        return ToolRailMotion::Settled;
    }
    if tool.has_execution_motion() {
        return ToolRailMotion::Running {
            elapsed: std::time::Duration::from_millis(
                u64::try_from(app.transcript_animation_phase())
                    .unwrap_or(u64::MAX)
                    .saturating_mul(crate::scheduling::active_animation_period_ms()),
            ),
            sampled_phase: app.transcript_animation_phase(),
        };
    }
    match tool.status {
        ToolCallDisplayStatus::PendingPermission => ToolRailMotion::Waiting,
        ToolCallDisplayStatus::Queued => ToolRailMotion::Queued,
        ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => app
            .tool_finish_elapsed(&tool.tool_call_id)
            .filter(|_| has_details)
            .map_or(ToolRailMotion::Settled, |elapsed| {
                ToolRailMotion::FinishFlash {
                    elapsed,
                    sampled_phase: app.transcript_animation_phase(),
                }
            }),
        ToolCallDisplayStatus::Running => ToolRailMotion::Settled,
    }
}

pub(super) fn tool_header_selected(app: &AppState, tool_call_id: &str) -> bool {
    app.focus == Focus::Details
        && !app.todo_pane_focused()
        && matches!(
            app.transcript_view.selected_entry,
            Some(TranscriptVisualEntryId::Part { semantic_key, .. }
                | TranscriptVisualEntryId::ToolGroup { semantic_key, .. })
                if semantic_key == super::ui_transcript_entry::semantic_key([tool_call_id])
        )
}

pub(super) fn edit_tool_action(tool_call: &crate::app::ToolCallEntry) -> &'static str {
    if matches!(tool_call.effective_tool_id(), "write" | "fs.write") {
        "Creating"
    } else {
        "Edit"
    }
}
