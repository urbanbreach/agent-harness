// allow: SIZE_OK — TUI UI rendering (widget layout + wheel target + render dispatch)
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{
    ActivityEntry, ActivityStatus, AppState, Focus, ReviewSurface, RuntimeStateKind, Tab,
    ToastVariant, ToolCallDisplayStatus,
};
use crate::keybindings::Action;
use crate::layout::{
    composer_input_height, inset_rect, live_empty_state_area, secondary_surface_layout,
    startup_composer_input_height, FrameLayoutPlan,
};
use crate::overlay::OverlayKind;
use crate::theme::Theme;

#[path = "ui_chrome.rs"]
mod ui_chrome;
#[path = "ui_context_budget.rs"]
mod ui_context_budget;
#[path = "ui_diff.rs"]
mod ui_diff;
#[path = "ui_fenced_text.rs"]
mod ui_fenced_text;
#[path = "ui_lifecycle.rs"]
mod ui_lifecycle;
#[path = "ui_live_turn_status.rs"]
mod ui_live_turn_status;
#[path = "ui_lsp.rs"]
mod ui_lsp;
#[path = "ui_markdown.rs"]
mod ui_markdown;
#[path = "ui_markdown_table.rs"]
mod ui_markdown_table;
#[path = "ui_overlays.rs"]
pub(crate) mod ui_overlays;
#[path = "ui_secondary.rs"]
mod ui_secondary;
#[path = "ui_secondary_events_tab.rs"]
mod ui_secondary_events_tab;
#[path = "ui_streaming_markdown.rs"]
mod ui_streaming_markdown;
#[path = "ui_syntax_highlight.rs"]
mod ui_syntax_highlight;
#[path = "ui_terminal.rs"]
mod ui_terminal;
#[path = "ui_tool_delegation.rs"]
mod ui_tool_delegation;
#[path = "ui_tool_diffs.rs"]
mod ui_tool_diffs;
#[path = "ui_tool_error.rs"]
mod ui_tool_error;
#[path = "ui_tool_input.rs"]
mod ui_tool_input;
#[path = "ui_tool_metadata.rs"]
mod ui_tool_metadata;
#[path = "ui_tool_output.rs"]
mod ui_tool_output;
#[path = "ui_tool_paths.rs"]
mod ui_tool_paths;
#[path = "ui_tool_question_todo.rs"]
mod ui_tool_question_todo;
pub(crate) use ui_tool_question_todo::{
    todo_items_from_tool_call, TranscriptTodoItem, TranscriptTodoStatus,
};
#[path = "ui_todo_pane.rs"]
mod ui_todo_pane;
#[path = "ui_tool_style.rs"]
mod ui_tool_style;
#[path = "ui_tool_titles.rs"]
mod ui_tool_titles;
#[path = "ui_tool_titles_harness.rs"]
mod ui_tool_titles_harness;
#[path = "ui_tool_visibility.rs"]
mod ui_tool_visibility;
#[path = "ui_tool_wrapping.rs"]
mod ui_tool_wrapping;
#[path = "ui_transcript.rs"]
mod ui_transcript;
#[path = "ui_transcript_bash.rs"]
mod ui_transcript_bash;
#[path = "ui_transcript_events.rs"]
mod ui_transcript_events;
#[path = "ui_transcript_interaction.rs"]
mod ui_transcript_interaction;
#[path = "ui_transcript_layout.rs"]
mod ui_transcript_layout;
#[path = "ui_transcript_mermaid.rs"]
mod ui_transcript_mermaid;
#[path = "ui_transcript_mermaid_art.rs"]
mod ui_transcript_mermaid_art;
#[path = "ui_transcript_page_flip.rs"]
mod ui_transcript_page_flip;
#[path = "ui_transcript_scrollbar.rs"]
mod ui_transcript_scrollbar;
#[cfg(test)]
#[path = "ui_transcript_scrollbar_hover_tests.rs"]
mod ui_transcript_scrollbar_hover_tests;
#[path = "ui_transcript_selection.rs"]
mod ui_transcript_selection;
#[path = "ui_transcript_style.rs"]
mod ui_transcript_style;
#[path = "ui_transcript_surface.rs"]
mod ui_transcript_surface;
#[cfg(test)]
#[path = "ui_transcript_surface_tests.rs"]
mod ui_transcript_surface_tests;
#[cfg(test)]
#[path = "ui_transcript_test_helpers.rs"]
mod ui_transcript_test_helpers;

pub(crate) use ui_transcript_layout::TranscriptContentAnchor;

use ui_chrome::{
    compact_inline_payload, display_width, elevated_card_surface, interruptive_modal_block,
    live_transcript_shell_section, muted_meta_style, panel_block, panel_style, render_footer,
    render_header, render_unified_bottom_dock, runtime_state_color, status_badge,
    take_width_prefix, truncate_plain_text, ChromeFrame,
};
pub(crate) use ui_chrome::{subagent_footer_target_at, SubagentFooterTarget};
pub(crate) use ui_diff::structured_diff_stats;
pub(super) use ui_lifecycle::render_startup_lifecycle_surface;
pub(crate) use ui_lifecycle::{live_empty_composer_guidance_visible, live_empty_state_visible};
use ui_lifecycle::{
    live_transcript_area_with_breadcrumb, render_live_breadcrumb, render_live_empty_state,
    startup_shell_visible,
};
pub(crate) use ui_live_turn_status::{
    live_turn_background_rect, live_turn_stop_rect, live_turn_watching_rect,
};
use ui_overlays::render_overlays;
pub(crate) use ui_secondary::{
    operator_sidebar_keyboard_targets, operator_sidebar_section_hit_target,
    operator_sidebar_selection_cell, operator_sidebar_selection_text,
    operator_sidebar_subagent_group_hit_target, operator_sidebar_subagent_session_hit_target,
    OperatorSidebarKeyboardTarget, OperatorSidebarKeyboardTargetKind, OperatorSidebarSelection,
    OperatorSidebarSelectionCell,
};
use ui_secondary::{render_live_details_overlay, render_operator_sidebar};
use ui_secondary_events_tab::render_help_tab;
use ui_terminal::render_terminal_panel;
use ui_transcript::render_transcript_pane;
pub(crate) use ui_transcript::transcript_diff_hunk_rows;
pub(crate) use ui_transcript::transcript_entry_scroll_top;
pub(crate) use ui_transcript::transcript_mouse_target;
pub(crate) use ui_transcript::transcript_return_to_live_hit;
pub(crate) use ui_transcript::transcript_scrollbar_hit;
#[cfg(test)]
pub(crate) use ui_transcript::transcript_selection_debug_snapshot;
pub(crate) use ui_transcript::transcript_timeline_turn_at;
pub(crate) use ui_transcript::TranscriptRenderSurfaceKind;
pub(crate) use ui_transcript::{
    transcript_navigation_entries, TranscriptNavigationEntry, TranscriptVisualEntryId,
};
pub(crate) use ui_transcript::{
    transcript_selection_cell, transcript_selection_patch_text, transcript_selection_text,
    transcript_selection_text_with_destinations,
};
pub use ui_transcript_interaction::hovered_wheel_target;
pub(crate) use ui_transcript_interaction::TranscriptMouseTarget;
pub(crate) use ui_transcript_scrollbar::TranscriptScrollbarHit;
#[cfg(test)]
pub(crate) use ui_transcript_selection::{
    reset_transcript_selection_cache_metrics_for_test,
    transcript_selection_cache_build_count_for_test,
};
pub(crate) use ui_transcript_selection::{TranscriptSelection, TranscriptSelectionCell};

#[cfg(test)]
use ui_secondary::format_detail_payload;
#[cfg(test)]
pub(crate) use ui_secondary::operator_sidebar_text_for_test;
#[cfg(test)]
pub(crate) use ui_secondary::orchestration_card_text_for_test;
#[cfg(test)]
pub(crate) use ui_secondary::{
    exact_test_compaction_applied_updates_active_context_usage_estimate,
    exact_test_operator_rail_collapses_modified_files_section_body,
    exact_test_operator_rail_collapses_todo_section_body,
    exact_test_operator_rail_hides_completed_todo_state,
    exact_test_operator_rail_keeps_subagents_visible_in_replay,
    exact_test_operator_rail_low_activity_presentation_prefers_primary_stack,
    exact_test_operator_rail_marks_background_subagent_terminal_from_notification,
    exact_test_operator_rail_matches_sidebar_text_styles,
    exact_test_operator_rail_places_todo_below_subagents,
    exact_test_operator_rail_renders_subagent_rows_from_orchestration_state,
    exact_test_operator_rail_renders_todo_items_from_artifact_state,
    exact_test_operator_rail_renders_todo_items_from_tool_state,
    exact_test_operator_rail_sanitizes_control_chars_in_sidebar_strings,
    exact_test_operator_rail_section_model_builds_pinned_summary,
    exact_test_operator_rail_section_model_counts_generic_mcp_activity,
    exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order,
    exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp,
    exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity,
    exact_test_operator_rail_section_model_surfaces_pending_permissions_first,
    exact_test_operator_rail_section_model_uses_runtime_mcp_activity_without_config,
    exact_test_operator_rail_shows_replay_wakeup_report_without_task_tool_row,
    exact_test_operator_rail_shows_wakeup_report_without_task_tool_row,
    exact_test_operator_rail_uses_generated_session_title,
    exact_test_operator_rail_uses_simple_subagent_task_labels,
    exact_test_operator_sidebar_hit_target_maps_section_headers,
};
#[cfg(test)]
use ui_transcript::build_transcript_lines;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelTarget {
    Transcript,
    Terminal,
    Inspector,
}

/// Compose the full frame from `app` without mutating state or emitting intents.
///
/// Orchestration-only: layout plan → chrome/content/footer/overlays/toast.
/// Event ingestion, key/mouse handlers, and UiIntent emission stay outside this path.
pub fn render_app(frame: &mut Frame, app: &AppState) {
    let theme = app.theme();
    let area = frame.area();
    let plan = FrameLayoutPlan::for_app(app, area);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.canvas)),
        area,
    );

    if app.status_dashboard_is_active() {
        ui_overlays::render_status_dashboard_surface(frame, app, theme, area);
        render_toast(frame, app, area, theme);
        return;
    }

    render_header(frame, app, &plan, theme);
    render_content(frame, app, plan.content, theme, &plan);
    if let (Some(message), Some(notice_area)) = (&app.model_prompt_notice, plan.model_prompt_notice)
    {
        let lines = wrap_completion_text(message, usize::from(notice_area.width));
        frame.render_widget(
            Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).style(
                Style::default()
                    .fg(theme.text.secondary)
                    .bg(theme.surface.canvas),
            ),
            notice_area,
        );
    }
    render_footer(frame, app, &plan, theme);
    if let Some(viewer) = app.transcript_viewer() {
        let surface = viewer.render_surface(area);
        crate::transcript_block_viewer::render_to_buffer(frame.buffer_mut(), area, &surface, theme);
    }
    render_overlays(frame, app, theme, &plan);
    render_toast(frame, app, area, theme);
}

fn render_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    render_surface(frame, app, area, theme, plan);
}

fn render_surface(
    frame: &mut Frame,
    app: &AppState,
    _area: Rect,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    match app.review_surface() {
        None => {
            if app.replay_mode {
                render_replay_session_surface(frame, app, theme, plan)
            } else {
                render_live_session_surface(frame, app, theme, plan)
            }
        }
        Some(surface) => {
            if app.replay_mode {
                render_replay_session_surface(frame, app, theme, plan)
            } else {
                render_live_session_surface(frame, app, theme, plan)
            }
            render_review_surface(frame, app, theme, plan, surface);
        }
    }
}

fn render_review_surface(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
    surface: ReviewSurface,
) {
    match surface {
        ReviewSurface::Events | ReviewSurface::Help => {
            render_help_tab(frame, app, plan.root, plan.content, plan.composer, theme);
        }
    }
}

fn render_replay_session_surface(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    let Some(transcript_area) = plan.transcript else {
        return;
    };
    let Some(dock) = plan.dock else {
        return;
    };

    frame.render_widget(
        live_transcript_shell_section(theme.surface.shell),
        plan.shell,
    );
    render_transcript_pane(frame, app, transcript_area, theme);
    if let Some(todo) = plan.todo {
        ui_todo_pane::render_todo_pane(frame, app, todo, theme);
    }
    if let Some(terminal_panel) = plan.terminal_panel {
        render_terminal_panel(frame, app, terminal_panel, theme);
    }
    if let Some(operator_sidebar) = plan.operator_sidebar {
        render_operator_sidebar(frame, app, operator_sidebar, theme);
    }
    render_live_details_overlay(frame, app, theme, plan.details_overlay);
    render_unified_bottom_dock(frame, app, dock, theme);
}

fn render_live_session_surface(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    if app.startup_shell_visible() {
        render_startup_session_surface(frame, app, theme, plan);
        return;
    }

    render_live_run_shell(frame, app, theme, plan);
}

fn render_startup_session_surface(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    let Some(transcript_area) = plan.transcript else {
        return;
    };
    let Some(dock) = plan.dock else {
        return;
    };

    frame.render_widget(
        live_transcript_shell_section(theme.surface.shell),
        plan.shell,
    );
    render_transcript_pane(frame, app, transcript_area, theme);
    render_unified_bottom_dock(frame, app, dock, theme);
}

fn render_live_run_shell(frame: &mut Frame, app: &AppState, theme: &Theme, plan: &FrameLayoutPlan) {
    let Some(transcript_area) = plan.transcript else {
        return;
    };
    let Some(dock) = plan.dock else {
        return;
    };

    frame.render_widget(
        live_transcript_shell_section(theme.surface.shell),
        plan.shell,
    );
    render_live_breadcrumb(frame, app, plan.shell, theme);
    let transcript_area = live_transcript_area_with_breadcrumb(transcript_area);
    render_transcript_pane(frame, app, transcript_area, theme);
    if let Some(todo) = plan.todo {
        ui_todo_pane::render_todo_pane(frame, app, todo, theme);
    }
    if let Some(terminal_panel) = plan.terminal_panel {
        render_terminal_panel(frame, app, terminal_panel, theme);
    }
    debug_assert!(
        plan.operator_sidebar.is_none(),
        "live run shell must not reserve a primary operator sidebar rect"
    );
    render_runtime_state_surface(frame, app, transcript_area, theme);
    render_live_details_overlay(frame, app, theme, plan.details_overlay);
    render_unified_bottom_dock(frame, app, dock, theme);
}

#[cfg(test)]
fn live_anchor_for_runtime_state(
    _app: &AppState,
    _runtime_kind: RuntimeStateKind,
    _planned_anchor: Option<Rect>,
) -> Option<Rect> {
    None
}

fn render_runtime_state_surface(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if app.replay_mode || app.startup_shell_visible() || app.active_permission().is_some() {
        return;
    }

    let state = app.runtime_state();
    let Some((title, guidance, accent)) = runtime_state_surface_copy(app, &state) else {
        return;
    };

    let Some(width) = crate::layout::runtime_state_surface_width(area) else {
        return;
    };

    let surface = elevated_card_surface(theme);
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let emphasis_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let overlay = runtime_state_surface_text(app, &state, usize::from(width)).unwrap_or(
        RuntimeStateSurfaceText {
            summary: state.summary.clone(),
            detail: None,
        },
    );
    let body_height = 1 + u16::from(overlay.detail.is_some());
    let Some(popup) = crate::layout::runtime_state_surface_area(area, width, body_height) else {
        return;
    };
    let block = interruptive_modal_block(
        theme,
        Line::from(vec![
            status_badge(
                state.kind.label(),
                runtime_state_color(state.kind, theme),
                theme,
            ),
            Span::styled("  ", metadata_style),
            Span::styled(
                title,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ]),
        accent,
        accent,
        ChromeFrame::Frame,
    );
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(body_height), Constraint::Length(1)])
        .split(inner);

    let mut body = vec![Line::from(vec![Span::styled(
        truncate_plain_text(&overlay.summary, usize::from(sections[0].width)),
        emphasis_style,
    )])];
    if let Some(detail) = overlay.detail.as_deref() {
        body.push(Line::from(vec![Span::styled(
            truncate_plain_text(detail, usize::from(sections[0].width)),
            metadata_style,
        )]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(body))
            .style(panel_style(surface, theme.text.primary))
            .wrap(Wrap { trim: true }),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            truncate_plain_text(guidance, usize::from(sections[1].width)),
            Style::default()
                .fg(accent)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(Alignment::Left),
        sections[1],
    );
}

fn render_toast(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if app.overlay_stack().top().is_some() {
        return;
    }
    let Some(toast) = app.toast() else {
        return;
    };
    if area.width <= 6 || area.height <= 4 {
        return;
    }

    let max_width = area.width.saturating_sub(6).min(60);
    if max_width < 8 {
        return;
    }

    let text_width = u16::try_from(display_width(&toast.message)).unwrap_or(u16::MAX);
    let width = text_width.saturating_add(4).min(max_width).max(8);
    let lines = wrap_completion_text(&toast.message, usize::from(width.saturating_sub(4)));
    let padding_y = theme.live_shell.rhythm.surface_margin_y;
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(padding_y.saturating_mul(2))
        .min(area.height.saturating_sub(2));
    let x = area.right().saturating_sub(width + 2);
    let popup = Rect::new(x, area.y.saturating_add(2), width, height);
    let accent = match toast.variant {
        ToastVariant::Info => theme.status.info,
        ToastVariant::Error => theme.status.error,
        ToastVariant::Mode => theme.text.accent,
    };
    let surface = theme.surface.panel;
    let fade_alpha = app.toast_fade_alpha().unwrap_or(1.0);
    let accent = ui_transcript_style::blend_color(surface, accent, fade_alpha);
    let text_color = ui_transcript_style::blend_color(surface, theme.text.primary, fade_alpha);
    let block = Block::default()
        .style(Style::default().bg(surface))
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(accent).bg(surface));
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let inner = Rect::new(
        popup.x.saturating_add(2),
        popup.y.saturating_add(padding_y),
        popup.width.saturating_sub(4),
        popup.height.saturating_sub(padding_y.saturating_mul(2)),
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>())
            .style(Style::default().fg(text_color).bg(surface)),
        inner,
    );
}

struct RuntimeStateSurfaceText {
    summary: String,
    detail: Option<String>,
}

fn runtime_state_surface_text(
    app: &AppState,
    state: &crate::app::RuntimeState,
    max_chars: usize,
) -> Option<RuntimeStateSurfaceText> {
    runtime_state_surface_copy(app, state)?;

    Some(RuntimeStateSurfaceText {
        summary: runtime_state_surface_summary(state),
        detail: runtime_state_surface_detail(state, max_chars),
    })
}

fn runtime_state_surface_summary(state: &crate::app::RuntimeState) -> String {
    match state.kind {
        RuntimeStateKind::Degraded => {
            "Live updates are catching up before sending resumes.".to_string()
        }
        RuntimeStateKind::Disconnected => {
            "Transcript stays visible, but sending is paused.".to_string()
        }
        RuntimeStateKind::Failure if state.composer_disabled => {
            "The failed run is preserved in this shell.".to_string()
        }
        RuntimeStateKind::Failure => "Review the latest failure before continuing.".to_string(),
        _ => state.summary.clone(),
    }
}

fn runtime_state_surface_detail(
    state: &crate::app::RuntimeState,
    max_chars: usize,
) -> Option<String> {
    match state.kind {
        RuntimeStateKind::Degraded | RuntimeStateKind::Disconnected | RuntimeStateKind::Failure => {
        }
        _ => return None,
    }

    let detail = state.detail.as_deref()?.trim();
    if detail.is_empty() || detail.eq_ignore_ascii_case("check transcript for details") {
        return None;
    }

    compact_inline_payload(detail, max_chars).or_else(|| Some(detail.to_string()))
}

fn runtime_state_surface_copy(
    app: &AppState,
    state: &crate::app::RuntimeState,
) -> Option<(&'static str, &'static str, Color)> {
    match state.kind {
        RuntimeStateKind::Degraded => Some((
            "Recovery in progress",
            "Draft locally until recovery completes.",
            app.theme().status.warning,
        )),
        RuntimeStateKind::Disconnected => Some((
            "Connection lost",
            "Reopen the TUI, then continue from the transcript.",
            app.theme().status.error,
        )),
        // Freeze h1-stream-probe: fail chrome is flat transcript `Retry failed: …`,
        // not an elevated Failure / Review required card over the body.
        RuntimeStateKind::Failure => None,
        _ => None,
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeOverlayTextForTest {
    pub badge: String,
    pub title: String,
    pub summary: String,
    pub detail: Option<String>,
    pub guidance: String,
}

#[cfg(test)]
pub(crate) fn runtime_overlay_text_for_test(
    app: &AppState,
    max_chars: usize,
) -> Option<RuntimeOverlayTextForTest> {
    if app.replay_mode || app.startup_shell_visible() || app.active_permission().is_some() {
        return None;
    }

    let state = app.runtime_state();
    let (title, guidance, _) = runtime_state_surface_copy(app, &state)?;
    let overlay = runtime_state_surface_text(app, &state, max_chars)?;

    Some(RuntimeOverlayTextForTest {
        badge: state.kind.label().to_string(),
        title: title.to_string(),
        summary: overlay.summary,
        detail: overlay.detail,
        guidance: guidance.to_string(),
    })
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;

#[path = "ui_dashboard_preview.rs"]
mod ui_dashboard_preview;
pub(crate) use ui_dashboard_preview::{
    frame as dashboard_preview_frame, lines as dashboard_preview_lines,
};

pub(crate) fn wrap_completion_text(text: &str, width: usize) -> Vec<String> {
    text.split('\n')
        .flat_map(|row| {
            ui_transcript_surface::wrap_surface_spans(
                vec![Span::raw(row.to_string())],
                width.max(1),
            )
        })
        .map(|spans| spans.iter().map(|span| span.content.as_ref()).collect())
        .collect()
}

#[path = "ui_terminal_output.rs"]
mod ui_terminal_output;

#[path = "ui_recorded_tool_output.rs"]
mod ui_recorded_tool_output;

pub(crate) use ui_tool_visibility::tool_output_is_viewer_only;

pub(crate) fn recorded_tool_viewer_content(
    tool: &crate::app::ToolCallEntry,
) -> crate::transcript_block_viewer::ViewerBlockContent {
    use crate::transcript_block_viewer::{ViewerBlockContent, ViewerPreamble};
    let text = recorded_tool_viewer_text(tool);
    if matches!(tool.effective_tool_id(), "read" | "fs.read") {
        let start_line = (tool.status == crate::app::ToolCallDisplayStatus::Succeeded
            && ui_tool_metadata::read_media_mime(tool).is_none())
        .then(|| {
            tool.output_json
                .as_ref()
                .and_then(|value| value.pointer("/metadata/display/lineStart"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1)
        });
        return ViewerBlockContent::new(&text, Some(&text)).with_preamble(ViewerPreamble::Read {
            path: ui_tool_paths::tool_path_display(tool).unwrap_or_default(),
            details: ui_tool_paths::read_tool_input_suffix(tool),
            start_line,
        });
    }
    if matches!(tool.effective_tool_id(), "bash" | "shell.run") {
        let command = ui_transcript_bash::shell_tool_command(tool).unwrap_or_default();
        let body = text
            .strip_prefix(&format!("$ {command}\n"))
            .unwrap_or(&text);
        return ViewerBlockContent::new(body, Some(body)).with_preamble(ViewerPreamble::Command {
            command: ui_tool_output::safe_tool_text(&command),
            description: ui_transcript_bash::shell_tool_title_description(tool, None),
        });
    }
    let label = match tool.effective_tool_id() {
        "read" | "fs.read" => "Read",
        "write" | "fs.write" => "Create",
        "edit" | "edit.hashline_apply" => "Edit",
        "list" | "fs.ls" => "List",
        _ => tool.effective_tool_id(),
    };
    ViewerBlockContent::new(&text, Some(&text)).with_preamble(ViewerPreamble::Title {
        label: label.to_string(),
        argument: ui_tool_paths::tool_path_display(tool).unwrap_or_default(),
    })
}

pub(crate) fn viewer_wrap_lines(
    lines: Vec<Line<'static>>,
    width: usize,
) -> (Vec<Line<'static>>, Vec<String>) {
    let mut output = Vec::new();
    let mut joiners: Vec<String> = Vec::new();
    for line in lines {
        let text = line.to_string();
        let mut cursor = 0;
        for (index, spans) in ui_tool_wrapping::words(line.spans, width)
            .into_iter()
            .enumerate()
        {
            let wrapped = Line::from(spans);
            let value = wrapped.to_string();
            let start = cursor
                + text
                    .get(cursor..)
                    .and_then(|rest| rest.find(&value))
                    .unwrap_or(0);
            if index > 0 {
                if let Some(joiner) = joiners.last_mut() {
                    *joiner = text.get(cursor..start).unwrap_or_default().to_owned();
                }
            }
            cursor = start + value.len();
            output.push(wrapped);
            joiners.push("\n".to_owned());
        }
    }
    (output, joiners)
}

pub(crate) fn viewer_preamble_lines(
    preamble: &crate::transcript_block_viewer::ViewerPreamble,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    use crate::transcript_block_viewer::ViewerPreamble;
    if let ViewerPreamble::Read { path, details, .. } = preamble {
        let mut header = vec![
            Span::styled(
                "Read ",
                Style::default()
                    .fg(theme.text.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                path.clone(),
                Style::default().fg(ui_tool_paths::tool_path_color(theme)),
            ),
        ];
        if !details.is_empty() {
            header.push(Span::styled(
                format!(" {details}"),
                Style::default().fg(theme.terminal_colors.muted),
            ));
        }
        let mut lines = ui_tool_wrapping::words(header, width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        lines.push(Line::default());
        return lines;
    }
    let bold = Style::default()
        .fg(theme.text.primary)
        .add_modifier(Modifier::BOLD);
    let plain = Style::default().fg(theme.text.primary);
    let (label, argument, shell, secondary) = match preamble {
        ViewerPreamble::Command {
            command,
            description,
        } => (
            "Run",
            description.as_deref().unwrap_or(command),
            description.is_none(),
            description.as_ref().map(|_| command),
        ),
        ViewerPreamble::Title { label, argument } => {
            (label.as_str(), argument.as_str(), false, None)
        }
        ViewerPreamble::Read { .. } => return Vec::new(),
    };
    let mut header = vec![Span::styled(label.to_string(), bold), Span::raw(" ")];
    let mut lines = if shell {
        let highlighted = ui_syntax_highlight::render_highlighted_code_block(
            Some("bash"),
            argument,
            argument,
            "",
            theme.text.primary,
            theme,
        );
        let rows =
            ui_tool_wrapping::shell(highlighted, width.saturating_sub(label.len() + 1).max(1));
        rows.into_iter()
            .enumerate()
            .map(|(index, spans)| {
                let mut prefix = if index == 0 {
                    header.clone()
                } else {
                    Vec::new()
                };
                prefix.extend(spans);
                Line::from(prefix)
            })
            .collect::<Vec<_>>()
    } else {
        header.push(Span::styled(argument.to_string(), plain));
        ui_tool_wrapping::words(header, width)
            .into_iter()
            .map(Line::from)
            .collect()
    };
    if let Some(command) = secondary {
        let highlighted = ui_syntax_highlight::render_highlighted_code_block(
            Some("bash"),
            command,
            command,
            "",
            theme.text.primary,
            theme,
        );
        for (index, spans) in ui_tool_wrapping::shell(highlighted, width.saturating_sub(2).max(1))
            .into_iter()
            .enumerate()
        {
            let mut line = vec![Span::styled(
                if index == 0 { "$ " } else { "  " },
                Style::default().fg(theme.terminal_colors.muted),
            )];
            line.extend(spans);
            lines.push(Line::from(line));
        }
    }
    for span in lines.iter_mut().flat_map(|line| &mut line.spans) {
        span.style = span.style.bg(theme.surface.shell);
    }
    lines.push(Line::default());
    lines
}

pub(crate) fn recorded_tool_viewer_text(tool: &crate::app::ToolCallEntry) -> String {
    let mut text = recorded_tool_viewer_body(tool);
    for hook in &tool.hook_executions {
        text.push_str("\n\n");
        if let Some(phase) = &hook.hook_event {
            text.push_str(phase);
            text.push_str(": ");
        }
        text.push_str(&hook.hook_name);
        let status = match hook.status {
            harness_core::event::HookExecutionStatus::Succeeded => "succeeded",
            harness_core::event::HookExecutionStatus::Blocked => "blocked",
            harness_core::event::HookExecutionStatus::Failed => "failed",
            harness_core::event::HookExecutionStatus::Skipped => "skipped",
            harness_core::event::HookExecutionStatus::Unknown => "unknown",
        };
        text.push_str(&format!(" ({status})"));
        if let Some(duration) = hook.duration_ms {
            text.push_str(&format!(" {duration}ms"));
        }
        if let Some(output) = &hook.output_summary {
            text.push('\n');
            text.push_str(output);
        }
    }
    ui_tool_output::safe_tool_text(&text)
}

fn recorded_tool_viewer_body(tool: &crate::app::ToolCallEntry) -> String {
    if let Some(output) = ui_recorded_tool_output::project(tool) {
        let text = output.full_text();
        if matches!(
            output,
            ui_recorded_tool_output::RecordedToolOutput::Mcp { error: Some(_), .. }
        ) {
            return text;
        }
        return ui_tool_error::tool_error_text(tool)
            .map_or_else(|| text.clone(), |error| format!("{text}\n\n{error}"));
    }
    if matches!(tool.effective_tool_id(), "shell.run" | "bash") {
        let command = ui_transcript_bash::shell_tool_command(tool).unwrap_or_default();
        let output = ui_transcript_bash::shell_tool_output(tool).unwrap_or_default();
        let lines = ui_terminal_output::render(&output, Style::default(), &Theme::default());
        return ui_tool_output::safe_tool_text(&format!(
            "$ {command}\n{}",
            lines
                .iter()
                .map(Line::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let text = tool
        .output_json
        .as_ref()
        .and_then(|value| value.pointer("/metadata/display/text"))
        .and_then(serde_json::Value::as_str)
        .or(tool.output_summary.as_deref())
        .unwrap_or("No recorded output");
    ui_tool_output::safe_tool_text(text)
}

pub(crate) fn safe_product_text(text: &str) -> String {
    ui_tool_output::safe_tool_text(text)
}

pub(crate) fn viewer_markdown_lines(text: &str, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    ui_markdown::append_rich_text_block(&mut lines, text, theme.text.primary, "", theme, width);
    lines
}

pub(crate) fn viewer_read_lines(
    text: &str,
    path: &str,
    start: u64,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let last = start
        .saturating_add(u64::try_from(text.lines().count().saturating_sub(1)).unwrap_or(u64::MAX));
    let gutter_width = last.to_string().len();
    ui_syntax_highlight::render_highlighted_code_block(
        Some(path),
        text,
        text,
        "",
        theme.text.primary,
        theme,
    )
    .into_iter()
    .enumerate()
    .map(|(index, line)| {
        let number = start.saturating_add(u64::try_from(index).unwrap_or(u64::MAX));
        let mut spans = vec![Span::styled(
            format!("{number:>gutter_width$}  "),
            Style::default().fg(theme.terminal_colors.muted),
        )];
        spans.extend(line.spans.into_iter().map(|mut span| {
            span.style.bg = None;
            span
        }));
        Line::from(spans)
    })
    .collect()
}
