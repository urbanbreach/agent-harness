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
#[path = "ui_diff.rs"]
mod ui_diff;
#[path = "ui_fenced_text.rs"]
mod ui_fenced_text;
#[path = "ui_lifecycle.rs"]
mod ui_lifecycle;
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
#[cfg(test)]
#[path = "ui_shell_exact_tests.rs"]
mod ui_shell_exact_tests;
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
#[path = "ui_tool_style.rs"]
mod ui_tool_style;
#[path = "ui_tool_titles.rs"]
mod ui_tool_titles;
#[path = "ui_tool_titles_harness.rs"]
mod ui_tool_titles_harness;
#[path = "ui_tool_visibility.rs"]
mod ui_tool_visibility;
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
#[path = "ui_transcript_scrollbar.rs"]
mod ui_transcript_scrollbar;
#[path = "ui_transcript_selection.rs"]
mod ui_transcript_selection;
#[path = "ui_transcript_style.rs"]
mod ui_transcript_style;
#[path = "ui_transcript_surface.rs"]
mod ui_transcript_surface;
#[cfg(test)]
#[path = "ui_transcript_test_helpers.rs"]
mod ui_transcript_test_helpers;

use ui_chrome::{
    compact_inline_payload, display_width, elevated_card_surface, interruptive_modal_block,
    live_transcript_shell_section, muted_meta_style, panel_block, panel_style, render_footer,
    render_header, render_unified_bottom_dock, runtime_state_color, status_badge,
    take_width_prefix, truncate_plain_text, ChromeFrame,
};
#[cfg(test)]
pub(crate) use ui_chrome::{
    exact_test_subagent_footer_body_keeps_ordered_transcript_tool_rows,
    exact_test_subagent_footer_matches_harness_layout,
    exact_test_subagent_footer_status_uses_running_and_cancelled_icons,
    exact_test_subagent_replay_suppresses_parent_replay_dock,
};
pub(crate) use ui_chrome::{subagent_footer_target_at, SubagentFooterTarget};
pub(super) use ui_lifecycle::render_startup_lifecycle_surface;
use ui_lifecycle::{
    live_empty_state_visible, live_transcript_area_with_breadcrumb, render_live_breadcrumb,
    render_live_empty_state, startup_shell_visible,
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
pub(crate) use ui_transcript::transcript_mouse_target;
pub(crate) use ui_transcript::transcript_scrollbar_hit;
#[cfg(test)]
pub(crate) use ui_transcript::transcript_selection_debug_snapshot;
pub(crate) use ui_transcript::{transcript_selection_cell, transcript_selection_text};
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
pub(crate) use ui_chrome::{
    exact_test_composer_viewport_wraps_at_word_boundaries,
    exact_test_composer_viewport_wraps_by_display_width,
    exact_test_footer_status_cluster_empty_when_no_activity,
    exact_test_footer_status_cluster_shows_pending_permission_count,
    exact_test_live_composer_disclosure_none_context_shows_est_zero,
    exact_test_live_composer_disclosure_none_context_shows_percent_when_limit_known,
    exact_test_live_composer_disclosure_summarizes_compaction_metrics,
    exact_test_live_composer_metadata_omits_success_without_variant,
    exact_test_live_composer_reserves_right_gap,
    exact_test_live_control_dock_collapses_disclosure_before_status,
    exact_test_live_control_dock_renders_shared_surface,
    exact_test_retry_summary_segment_prioritizes_retry_indicator,
    exact_test_startup_disclosure_matches_harness_hint_row,
    exact_test_tool_status_summary_uses_effective_tool_identity,
};
#[cfg(test)]
pub(crate) use ui_shell_exact_tests::{
    exact_test_compact_operator_rail_does_not_capture_wheel,
    exact_test_persistent_operator_sidebar_uses_panel_gutter,
    exact_test_replay_prompt_pane_is_visibly_read_only,
    exact_test_startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition,
    exact_test_wheel_target_excludes_activity_portion_of_live_overlay,
    exact_test_wheel_target_hits_inspector_inside_live_overlay,
    exact_test_wheel_target_hits_transcript_when_hovered,
};

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
#[cfg(test)]
pub(crate) use ui_transcript::{
    exact_test_block_tool_cards_skip_empty_subtitle_rows,
    exact_test_file_search_rows_match_reference_title_description_shape,
    exact_test_generic_tool_successful_output_prefers_inline_background_rows,
    exact_test_inline_tool_rows_wrap_long_subtitles_cleanly,
    exact_test_latest_assistant_footer_stays_after_trailing_tool_rows,
    exact_test_lsp_tool_successful_output_stays_hidden_until_generic_output_enabled,
    exact_test_markdown_table_rich_selection_matches_rendered_rows,
    exact_test_markdown_table_selection_matches_rendered_rows,
    exact_test_markdown_tables_match_reference_top_level_columns,
    exact_test_markdown_tables_render_inline_links_code_alignment_and_cjk_width,
    exact_test_mcp_tool_transcript_rows_use_effective_identity_without_generic_fallback,
    exact_test_native_tool_transcript_rows_show_reference_timestamps_and_task_metadata,
    exact_test_redacted_only_reasoning_matches_reference_empty_body,
    exact_test_skill_tool_rows_match_reference_title_and_icon,
    exact_test_todo_write_rows_render_open_checklist,
    exact_test_todo_write_running_renders_inline_updating_indicator,
    exact_test_transcript_applied_edit_missing_diff_surfaces_fallback,
    exact_test_transcript_apply_patch_multifile_uses_output_edit_paths,
    exact_test_transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs,
    exact_test_transcript_edit_tool_matches_inline_diff_shape,
    exact_test_transcript_follow_mode_uses_measured_surface_heights,
    exact_test_transcript_harness_tool_progress_indicators,
    exact_test_transcript_inline_diff_stays_compact_between_tool_rows,
    exact_test_transcript_native_edit_renders_inline_diff_from_artifact,
    exact_test_write_tool_hides_redundant_patched_file_header,
    exact_test_write_tool_renders_plain_numbered_dual_line_body,
    exact_test_write_tool_title_matches_thought_lead,
    exact_test_selected_rail_prefers_last_tool_over_thought,
    exact_test_selected_rail_falls_back_to_thought_without_tools,
    exact_test_pending_question_has_no_selected_rail,
    exact_test_done_body_after_tool_packs_wall_clock_on_same_line,
    exact_test_body_after_thought_keeps_separate_wall_clock_row,
    exact_test_no_tool_turn_without_thinking_keeps_thought,
    exact_test_tool_turn_without_thinking_omits_thought,
    exact_test_transcript_pending_permission_stays_after_last_activity,
    exact_test_transcript_proposed_edit_renders_header,
    exact_test_transcript_reasoning_precedes_answer_and_tool_rows,
    exact_test_transcript_rejected_edit_surfaces_reason_inline,
    exact_test_transcript_scroll_offset_preserves_large_overflow,
    exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks,
    exact_test_transcript_section_model_preserves_activity_order,
    exact_test_transcript_task_rows_match_reference_inline_title_and_no_hint,
    exact_test_transcript_task_rows_show_child_status_duration_and_counts,
    exact_test_transcript_tool_rows_follow_chronological_turn_order,
    exact_test_transcript_user_and_reasoning_match_reference_entry_body,
    exact_test_visible_surface_lines_support_large_offsets,
};

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
        Block::default().style(Style::default().bg(ratatui::style::Color::Reset)),
        area,
    );

    render_header(frame, app, &plan, theme);
    render_content(frame, app, plan.content, theme, &plan);
    render_footer(frame, app, &plan, theme);
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

    frame.render_widget(live_transcript_shell_section(theme), plan.shell);
    render_transcript_pane(frame, app, transcript_area, theme);
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

    frame.render_widget(live_transcript_shell_section(theme), plan.shell);
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

    frame.render_widget(live_transcript_shell_section(theme), plan.shell);
    render_live_breadcrumb(frame, app, transcript_area, theme);
    let transcript_area = live_transcript_area_with_breadcrumb(transcript_area);
    render_transcript_pane(frame, app, transcript_area, theme);
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
    let x = area.right().saturating_sub(width + 2);
    let popup = Rect::new(x, area.y.saturating_add(2), width, 3);
    let accent = match toast.variant {
        ToastVariant::Info => theme.status.info,
        ToastVariant::Error => theme.status.error,
    };
    let surface = theme.surface.panel;
    let block = Block::default()
        .style(Style::default().bg(surface))
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(accent).bg(surface));
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let inner = Rect::new(
        popup.x.saturating_add(2),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(4),
        1,
    );
    if inner.width == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_plain_text(&toast.message, usize::from(inner.width)),
            Style::default().fg(theme.text.primary).bg(surface),
        ))),
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
        // Freeze run1-stream-probe: fail chrome is flat transcript `Retry failed: …`,
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
