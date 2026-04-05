use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::{
    session_history_artifact_label, session_history_lineage_label, session_history_profile_label,
    session_history_provider_model_label, session_history_resumability_label,
    session_history_run_name, session_history_status_label, ActivityEntry, ActivityStatus,
    AppState, Focus, OrchestrationTaskRow, OrchestrationTaskState, ReviewSurface, RuntimeStateKind,
    StartupLauncherAction, Tab, ToolCallDisplayStatus,
};
use crate::keybindings::Action;
use crate::layout::{
    composer_input_height, inset_rect, live_empty_state_area, secondary_surface_layout,
    split_secondary_surface, FrameLayoutPlan,
};
use crate::overlay::OverlayKind;
use crate::theme::Theme;

#[path = "ui_chrome.rs"]
mod ui_chrome;
#[path = "ui_lifecycle.rs"]
mod ui_lifecycle;
#[path = "ui_overlays.rs"]
mod ui_overlays;
#[path = "ui_secondary.rs"]
mod ui_secondary;
#[path = "ui_transcript.rs"]
mod ui_transcript;

use ui_chrome::{
    chromeless_shell_section, compact_inline_payload, elevated_card_surface,
    interruptive_modal_block, muted_meta_style, panel_block, panel_style, render_footer,
    render_header, render_unified_bottom_dock, runtime_state_color, status_badge,
    subdued_payload_style, transcript_prefix_style, truncate_plain_text, ChromeFrame,
};
pub(super) use ui_lifecycle::render_startup_lifecycle_surface;
use ui_lifecycle::{live_empty_state_visible, render_live_empty_state, startup_shell_visible};
use ui_overlays::render_overlays;
use ui_secondary::{
    render_events_tab, render_help_tab, render_live_details_overlay, render_operator_sidebar,
};
pub use ui_transcript::hovered_wheel_target;
use ui_transcript::{append_text_block, render_transcript_pane};

#[cfg(test)]
pub(crate) use ui_chrome::{
    exact_test_live_control_dock_collapses_disclosure_before_status,
    exact_test_live_control_dock_renders_shared_surface,
};
#[cfg(test)]
pub(crate) fn exact_test_startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition()
{
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_option(
        &crate::app::ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
    ));

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw frame");
    let debug = format!("{:?}", terminal.backend().buffer());

    assert!(debug.contains("Launch: deep · GPT-5.4 Mini · Deterministic"));
    assert!(debug.contains("Provider default"));
    assert!(debug.contains("Ask Harness anything…"));
    assert!(!debug.contains("Tabs"));
    assert!(!debug.contains("Actions:"));
    assert!(!debug.contains("Enter select"));
}
#[cfg(test)]
pub(crate) fn exact_test_replay_prompt_pane_is_visibly_read_only() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_option(
        &crate::app::ModelOption {
            profile: "archive".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        },
    ));

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw frame");
    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(debug.contains("Replay · read-only"));
    assert!(debug.contains("Recorded runtime · read-only:"));
    assert!(debug.contains("Provider default"));
    assert!(debug.contains("Replay is read-only"));
    assert!(!debug.contains("Type a prompt for the next turn"));
    assert!(!debug.contains("Recent context for the next turn"));
}

#[cfg(test)]
pub(crate) fn exact_test_unified_bottom_dock_uses_single_layout_entrypoint() {
    fn count_occurrences(haystack: &str, needle: &str) -> usize {
        haystack.matches(needle).count()
    }

    let source = include_str!("ui.rs");
    assert_eq!(
        count_occurrences(
            source,
            "render_unified_bottom_dock(frame, app, dock, theme);"
        )
        .saturating_sub(1),
        3,
        "startup, live, and replay shells should each route through the unified dock renderer"
    );
    assert_eq!(
        count_occurrences(source, "let Some(dock) = plan.dock else {").saturating_sub(1),
        3,
        "startup, live, and replay shells should consume the shared dock layout entrypoint"
    );
    assert_eq!(
        count_occurrences(
            source,
            "render_live_control_dock_surface(frame, status_area, composer_area, theme);"
        )
        .saturating_sub(1),
        0,
        "legacy stacked live dock composition should be gone from ui.rs"
    );
    assert_eq!(
        count_occurrences(
            source,
            "render_replay_read_only_control_dock(frame, app, composer_area, theme);"
        )
        .saturating_sub(1),
        0,
        "replay should no longer own a separate dock renderer entrypoint in ui.rs"
    );
}
#[cfg(test)]
use ui_secondary::format_detail_payload;
#[cfg(test)]
pub(crate) use ui_secondary::operator_sidebar_text_for_test;
#[cfg(test)]
pub(crate) use ui_secondary::orchestration_card_text_for_test;
#[cfg(test)]
pub(crate) use ui_secondary::{
    exact_test_operator_rail_low_activity_presentation_prefers_primary_stack,
    exact_test_operator_rail_section_model_builds_pinned_summary,
    exact_test_operator_rail_section_model_counts_generic_mcp_activity,
    exact_test_operator_rail_section_model_hides_empty_sources_but_preserves_order,
    exact_test_operator_rail_section_model_keeps_native_prefix_tools_out_of_mcp,
    exact_test_operator_rail_section_model_separates_mcp_from_native_tool_activity,
    exact_test_operator_rail_section_model_surfaces_pending_permissions_first,
};
#[cfg(test)]
use ui_transcript::build_transcript_lines;
#[cfg(test)]
pub(crate) use ui_transcript::{
    exact_test_native_tool_transcript_rows_show_disclosure_timestamps_and_task_metadata,
    exact_test_transcript_answer_precedes_nested_context,
    exact_test_transcript_edit_tool_matches_opencode_inline_diff_shape,
    exact_test_transcript_follow_mode_uses_measured_surface_heights,
    exact_test_transcript_pending_permission_stays_after_last_activity,
    exact_test_transcript_proposed_edit_renders_opencode_header,
    exact_test_transcript_rejected_edit_surfaces_reason_inline,
    exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks,
    exact_test_transcript_section_model_preserves_activity_order,
    exact_test_transcript_task_rows_show_child_status_duration_and_counts,
};

#[cfg(test)]
fn rect_center(area: Rect) -> (u16, u16) {
    (
        area.x.saturating_add(area.width.saturating_sub(1) / 2),
        area.y.saturating_add(area.height.saturating_sub(1) / 2),
    )
}

#[cfg(test)]
pub(crate) fn exact_test_wheel_target_hits_transcript_when_hovered() {
    let app = AppState::new_live(None, false, None);
    let area = Rect::new(0, 0, 140, 40);
    let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
    let transcript = hit_areas.transcript.expect("transcript area");
    let (column, row) = rect_center(transcript);

    assert_eq!(
        hovered_wheel_target(&app, area, column, row),
        Some(WheelTarget::Transcript)
    );
}

#[cfg(test)]
pub(crate) fn exact_test_wheel_target_hits_inspector_inside_live_overlay() {
    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    let area = Rect::new(0, 0, 140, 40);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let rail = plan.operator_sidebar.expect("compact operator rail");
    let hit_areas = plan.wheel_hit_areas;
    let (column, row) = rect_center(rail);

    assert_eq!(hit_areas.overlay, None);
    assert_eq!(hit_areas.inspector, None);
    assert_eq!(hovered_wheel_target(&app, area, column, row), None);
}

#[cfg(test)]
pub(crate) fn exact_test_wheel_target_excludes_activity_portion_of_live_overlay() {
    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    let area = Rect::new(0, 0, 140, 40);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let rail = plan.operator_sidebar.expect("compact operator rail");
    let hit_areas = plan.wheel_hit_areas;

    assert_eq!(hit_areas.overlay, None);
    assert_eq!(hit_areas.inspector, None);
    assert_eq!(
        hovered_wheel_target(
            &app,
            area,
            rail.x.saturating_add(1),
            rail.y.saturating_add(1),
        ),
        None
    );
}

#[cfg(test)]
pub(crate) fn exact_test_compact_operator_rail_does_not_capture_wheel() {
    let app = AppState::new_live(None, false, None);
    let area = Rect::new(0, 0, 140, 40);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let rail = plan.operator_sidebar.expect("compact operator rail");
    let (column, row) = rect_center(rail);

    assert_eq!(plan.wheel_hit_areas.overlay, None);
    assert_eq!(plan.wheel_hit_areas.inspector, None);
    assert_eq!(hovered_wheel_target(&app, area, column, row), None);
}

#[cfg(test)]
pub(crate) fn exact_test_persistent_operator_sidebar_uses_panel_gutter() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_sidebar_panel".to_string(),
        seq: 1,
        run_id: "run_sidebar_panel".to_string(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("ui-tests".to_string()),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_sidebar_panel".to_string()),
        payload: harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_sidebar_panel".to_string(),
            path: "demo.txt".to_string(),
            new_file_digest: "digest-sidebar-panel".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    });

    let theme = Theme::default();
    let area = Rect::new(0, 0, 160, 30);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let sidebar = plan.operator_sidebar.expect("persistent operator sidebar");
    let transcript = plan.transcript.expect("transcript area");

    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw frame");

    let buffer = terminal.backend().buffer();
    let gutter_x = transcript.x.saturating_add(transcript.width);
    let sample_y = sidebar.y.saturating_add(1);

    assert_eq!(buffer[(gutter_x, sample_y)].bg, theme.surface.shell);
    assert_eq!(buffer[(gutter_x, sample_y)].symbol(), " ");
    assert_eq!(buffer[(sidebar.x, sample_y)].bg, theme.surface.panel);
    assert_eq!(buffer[(sidebar.x, sample_y)].symbol(), " ");
    assert_eq!(
        buffer[(sidebar.x.saturating_add(1), sample_y)].bg,
        theme.surface.panel
    );
    assert_eq!(
        buffer[(sidebar.x.saturating_add(1), sample_y)].symbol(),
        " "
    );
    assert_eq!(
        buffer[(sidebar.x.saturating_add(2), sample_y)].bg,
        theme.surface.panel
    );
    assert_ne!(buffer[(sidebar.x, sample_y)].symbol(), "│");
    assert_ne!(buffer[(sidebar.x, sample_y)].symbol(), "┃");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelTarget {
    Transcript,
    Inspector,
}

pub fn render_app(frame: &mut Frame, app: &AppState) {
    let theme = app.theme();
    let area = frame.area();
    let plan = FrameLayoutPlan::for_app(app, area);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.canvas)),
        area,
    );

    render_header(frame, app, &plan, theme);
    render_content(frame, app, plan.content, theme, &plan);
    render_footer(frame, app, &plan, theme);
    render_overlays(frame, app, theme, &plan);
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
    let Some(transcript_area) = plan.transcript else {
        return;
    };

    match surface {
        ReviewSurface::Events => render_events_tab(frame, app, transcript_area, theme),
        ReviewSurface::Help => render_help_tab(frame, app, transcript_area, theme),
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

    frame.render_widget(chromeless_shell_section(theme), plan.shell);
    render_transcript_pane(frame, app, transcript_area, theme);
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

    frame.render_widget(chromeless_shell_section(theme), plan.shell);
    render_startup_lifecycle_surface(frame, app, transcript_area, theme);
    render_unified_bottom_dock(frame, app, dock, theme);
}

fn render_live_run_shell(frame: &mut Frame, app: &AppState, theme: &Theme, plan: &FrameLayoutPlan) {
    let Some(transcript_area) = plan.transcript else {
        return;
    };
    let Some(dock) = plan.dock else {
        return;
    };

    frame.render_widget(chromeless_shell_section(theme), plan.shell);
    render_transcript_pane(frame, app, transcript_area, theme);
    if let Some(operator_sidebar) = plan.operator_sidebar {
        render_operator_sidebar(frame, app, operator_sidebar, theme);
    }
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
        RuntimeStateKind::Failure => Some((
            "Review required",
            if state.composer_disabled {
                "inspect transcript, then use commands to adjust the draft or recover."
            } else {
                "Review the failure, then retry or continue."
            },
            app.theme().status.error,
        )),
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
mod tests {
    use super::*;
    use crate::app::{LaunchMetadata, ModelOption};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
        PermissionResolvedEvent, ProviderRequestFinishedEvent, ProviderRequestStartedEvent,
        ProviderStreamDeltaEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
        ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
    };

    fn render_debug(app: &AppState, width: u16, height: u16) -> String {
        use ratatui::{backend::TestBackend, Terminal};

        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        terminal
            .draw(|frame| render_app(frame, app))
            .expect("draw frame");
        format!("{:?}", terminal.backend().buffer())
    }

    fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_{seq:04}"),
            seq,
            run_id: "run_ui_tests".to_string(),
            mono_ms: seq,
            ts: Some("2026-02-03T12:00:00Z".to_string()),
            actor: EventActor::new(ActorKind::System, Some("ui-tests".to_string())),
            correlation_id: Some(request_id.to_string()),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn transcript_debug(app: &AppState) -> String {
        build_transcript_lines(app, app.theme())
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn live_anchor_stays_hidden_during_active_turn_and_permission_checkpoint_states() {
        let planned_anchor = Some(Rect::new(0, 0, 80, 1));

        let mut sending = AppState::new_live(None, false, None);
        sending.handle_key(key(KeyCode::Char('h')));
        sending.handle_key(key(KeyCode::Enter));
        assert_eq!(sending.runtime_state().kind, RuntimeStateKind::Sending);
        assert_eq!(
            live_anchor_for_runtime_state(&sending, sending.runtime_state().kind, planned_anchor),
            None
        );

        sending.ingest_event(envelope(
            1,
            "req_anchor_streaming",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_anchor_streaming".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "hello".to_string(),
                request_digest: "digest-anchor-streaming".to_string(),
            }),
        ));
        sending.ingest_event(envelope(
            2,
            "req_anchor_streaming",
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_anchor_streaming".to_string(),
                delta: "hello world".to_string(),
            }),
        ));
        assert_eq!(sending.runtime_state().kind, RuntimeStateKind::Streaming);
        assert_eq!(
            live_anchor_for_runtime_state(&sending, sending.runtime_state().kind, planned_anchor),
            None
        );

        let mut permission = AppState::new_live(None, false, None);
        permission.ingest_event(envelope(
            1,
            "req_anchor_permission",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_anchor_permission".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "edit the file".to_string(),
                request_digest: "digest-anchor-permission".to_string(),
            }),
        ));
        permission.ingest_event(envelope(
            2,
            "req_anchor_permission",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_anchor_permission".to_string(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: r#"{"path":"demo.txt"}"#.to_string(),
                args_digest: "digest-anchor-permission-args".to_string(),
                metadata: None,
            }),
        ));
        permission.ingest_event(envelope(
            3,
            "req_anchor_permission",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_anchor_permission".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_anchor_permission".to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-anchor-permission-request".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));
        assert_eq!(
            permission.runtime_state().kind,
            RuntimeStateKind::PermissionBlocked
        );
        assert_eq!(
            live_anchor_for_runtime_state(
                &permission,
                permission.runtime_state().kind,
                planned_anchor,
            ),
            None
        );
    }

    #[test]
    fn transcript_debug_places_assistant_answer_before_nested_context() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_answer_first",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_answer_first".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Restyle the transcript shell".to_string(),
                request_digest: "digest-answer-first".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_answer_first",
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_answer_first".to_string(),
                delta: "Drafting a document-like plan".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_answer_first",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_answer_first".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
                args_digest: "digest-answer-first-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_answer_first",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_answer_first".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read from src/ui.rs".to_string()),
                output_digest: Some("digest-answer-first-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            5,
            "req_answer_first",
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_answer_first".to_string(),
                delta: "Found the transcript renderer and the composer chrome.".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            6,
            "req_answer_first",
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_answer_first".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-answer-first-finished".to_string()),
            }),
        ));

        let transcript = transcript_debug(&app);
        let answer_index = transcript
            .find("Found the transcript renderer and the composer chrome.")
            .expect("answer text");
        let thinking_index = transcript
            .find("Thinking: · Drafting a document-like plan")
            .expect("thinking summary");
        let tool_index = transcript.find("Read src/ui.rs").expect("tool summary");

        assert!(answer_index < thinking_index);
        assert!(thinking_index < tool_index);
    }

    #[test]
    fn theme_provides_default_colors() {
        let theme = Theme::default();
        assert!(matches!(
            theme.surface.canvas,
            ratatui::style::Color::Rgb(_, _, _)
        ));
    }

    #[test]
    fn wheel_hit_testing_uses_app_theme() {
        let area = Rect::new(0, 0, 140, 40);

        let mut default_app = AppState::new_live(None, false, None);
        default_app.live_details_drawer_open = true;
        default_app.ingest_event(harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: "evt_theme_probe_default".to_string(),
            seq: 1,
            run_id: "run_theme_probe".to_string(),
            mono_ms: 1,
            ts: None,
            actor: harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                Some("ui-tests".to_string()),
            ),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_theme_probe".to_string()),
            payload: harness_core::event::EventV1::EditApplied(
                harness_core::event::EditAppliedEvent {
                    edit_id: "edit_theme_probe_default".to_string(),
                    path: "demo.txt".to_string(),
                    new_file_digest: "digest-theme-probe-default".to_string(),
                    diff_rel_path: None,
                    diff_digest: None,
                },
            ),
        });
        let default_plan = FrameLayoutPlan::for_app(&default_app, area);

        let mut themed_app = AppState::new_live(None, false, None);
        themed_app.live_details_drawer_open = true;
        themed_app.ingest_event(harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: "evt_theme_probe_themed".to_string(),
            seq: 1,
            run_id: "run_theme_probe".to_string(),
            mono_ms: 1,
            ts: None,
            actor: harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                Some("ui-tests".to_string()),
            ),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_theme_probe".to_string()),
            payload: harness_core::event::EventV1::EditApplied(
                harness_core::event::EditAppliedEvent {
                    edit_id: "edit_theme_probe_themed".to_string(),
                    path: "demo.txt".to_string(),
                    new_file_digest: "digest-theme-probe-themed".to_string(),
                    diff_rel_path: None,
                    diff_digest: None,
                },
            ),
        });
        let mut custom_theme = Theme::default();
        custom_theme.live_shell.primary.centered_content_width = 72;
        custom_theme.live_shell.primary.content_margin_x = 10;
        custom_theme.live_shell.primary.activity_drawer_width = 18;
        custom_theme.live_shell.primary.details_sidebar_width = 36;
        themed_app.set_theme_for_test(custom_theme);

        let default_transcript = default_plan.transcript.expect("default transcript area");
        let themed_plan = FrameLayoutPlan::for_app(&themed_app, area);
        let themed_rail = themed_plan.operator_sidebar.expect("themed operator rail");

        assert_eq!(
            default_plan.wheel_hit_areas.overlay,
            default_plan.operator_sidebar
        );
        assert_eq!(
            default_plan.wheel_hit_areas.inspector,
            default_plan.operator_sidebar
        );
        assert_eq!(
            themed_plan.wheel_hit_areas.overlay,
            themed_plan.operator_sidebar
        );
        assert_eq!(
            themed_plan.wheel_hit_areas.inspector,
            themed_plan.operator_sidebar
        );

        assert_eq!(
            hovered_wheel_target(
                &default_app,
                area,
                default_transcript.x.saturating_add(2),
                default_transcript.y.saturating_add(1),
            ),
            Some(WheelTarget::Transcript)
        );
        assert_eq!(
            hovered_wheel_target(
                &themed_app,
                area,
                themed_rail.x.saturating_add(1),
                themed_rail.y.saturating_add(1),
            ),
            Some(WheelTarget::Inspector)
        );
    }

    #[test]
    fn live_header_uses_actual_launch_metadata() {
        let mut app = AppState::new_live(None, false, None);
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(!debug.contains("run unknown"));
        assert!(debug.contains("Launch: deep · gpt-5.4"));
        assert!(!debug.contains("Launch: deep · gpt-5.4 · Demo"));
        assert!(!debug.contains("default/default"));
    }

    #[test]
    fn live_control_dock_keeps_current_runtime_primary_and_next_turn_secondary() {
        let variant_cycle_overrides = [("variant_cycle".to_string(), "tab".to_string())]
            .into_iter()
            .collect();
        let primary = ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        };
        let alternate = ModelOption {
            profile: "writer".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("creative".to_string()),
            display_label: Some("GPT-5.4 Mini · Creative".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        };

        let mut app = AppState::new_live(None, false, None);
        app.apply_keybindings(variant_cycle_overrides);
        app.set_launch_metadata(
            LaunchMetadata::from_model_option(&primary)
                .with_available_models(vec![primary.clone(), alternate]),
        );

        app.handle_key(key(KeyCode::Tab));

        assert_eq!(
            app.runtime_context_summary_segment_text(),
            Some("Next turns: writer · GPT-5.4 Mini · Creative".to_string())
        );

        let debug = render_debug(&app, 160, 24);
        assert!(debug.contains("Current runtime: deep · GPT-5.4 Mini · Deterministic"));
        assert!(!debug.contains("Current runtime: writer · GPT-5.4 Mini · Creative"));
    }

    #[test]
    fn continued_live_control_dock_preserves_continued_runtime_after_switch() {
        let variant_cycle_overrides = [("variant_cycle".to_string(), "tab".to_string())]
            .into_iter()
            .collect();
        let primary = ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        };
        let alternate = ModelOption {
            profile: "writer".to_string(),
            provider: "default".to_string(),
            model: "gpt-5.4-mini".to_string(),
            variant: Some("creative".to_string()),
            display_label: Some("GPT-5.4 Mini · Creative".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            reasoning_effort: None,
            text_verbosity: None,
            recommended_for: None,
        };

        let mut app = AppState::new_live(None, false, None);
        app.apply_keybindings(variant_cycle_overrides);
        app.set_launch_metadata(
            LaunchMetadata::from_model_option(&primary)
                .with_available_models(vec![primary.clone(), alternate])
                .with_mode_label("Continued"),
        );

        app.handle_key(key(KeyCode::Tab));

        assert_eq!(
            app.runtime_context_summary_segment_text(),
            Some("Next turns: writer · GPT-5.4 Mini · Creative".to_string())
        );

        let debug = render_debug(&app, 160, 24);
        assert!(debug.contains("Continued runtime: deep · GPT-5.4 Mini · Deterministic"));
        assert!(!debug.contains("Continued runtime: writer · GPT-5.4 Mini · Creative"));
    }

    #[test]
    fn footer_hints_follow_keymap_overrides() {
        let mut app = AppState::new_live(None, false, None);
        app.apply_keybindings(
            [
                ("submit_prompt".to_string(), "ctrl+s".to_string()),
                ("insert_newline".to_string(), "ctrl+j".to_string()),
                ("help".to_string(), "g".to_string()),
                ("quit".to_string(), "x".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Ctrl+s send"));
        assert!(!debug.contains("Ctrl+j nl"));
        assert!(!debug.contains("g shortcuts"));
        assert!(!debug.contains("q quit"));
    }

    #[test]
    fn live_empty_state_uses_shared_startup_copy_without_mode_badges() {
        let mut demo = AppState::new_live(None, false, None);
        demo.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
        );

        let demo_debug = render_debug(&demo, 100, 24);
        assert!(demo_debug.contains("Harness"));
        assert!(demo_debug.contains("Launch: worker · model-1"));
        assert!(demo_debug.contains("Start a conversation to begin"));
        assert!(!demo_debug.contains("Demo mode · mock provider"));

        let mut mock = AppState::new_live(None, false, None);
        mock.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
        );

        let mock_debug = render_debug(&mock, 100, 24);
        assert!(mock_debug.contains("Harness"));
        assert!(mock_debug.contains("Launch: worker · model-1"));
        assert!(mock_debug.contains("Start a conversation to begin"));
        assert!(!mock_debug.contains("Mock mode · mock provider"));
        assert!(!mock_debug.contains("Launch: worker · model-1 · Mock"));
    }

    #[test]
    fn startup_shell_shows_profile_provider_and_model_chrome() {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻") || debug.contains("Harness"));
        assert!(debug.contains("Launch: deep · gpt-5.4"));
        assert!(debug.contains("Provider proxy · Demo"));
        assert!(debug.contains("Launch: deep · gpt-5.4 · provider proxy · Demo"));
        assert!(debug.contains("Ctrl+p open"));
        assert!(!debug.contains("Enter select"));
        assert!(debug.contains("Ask Harness anything…"));
        assert!(!debug.contains("Dispatch a new run, reopen live work, or inspect saved history."));
        assert!(!debug.contains("Actions:"));
    }

    #[test]
    fn startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition() {
        exact_test_startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition();
    }

    #[test]
    fn replay_prompt_pane_is_visibly_read_only() {
        exact_test_replay_prompt_pane_is_visibly_read_only();
    }

    #[test]
    fn help_surface_lists_active_bindings() {
        let mut app = AppState::new_live(None, false, None);
        app.active_review_surface = Some(ReviewSurface::Help);
        app.apply_keybindings(
            [
                ("open_event_log".to_string(), "e".to_string()),
                ("help".to_string(), "g".to_string()),
                ("toggle_follow".to_string(), "z".to_string()),
                ("submit_prompt".to_string(), "ctrl+s".to_string()),
                ("insert_newline".to_string(), "ctrl+j".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let debug = render_debug(&app, 100, 30);
        assert!(debug.contains("Live shell:"));
        assert!(debug.contains("z"));
        assert!(debug.contains("Toggle follow mode"));
        assert!(debug.contains("Ctrl+s"));
        assert!(debug.contains("Submit prompt"));
        assert!(debug.contains("Ctrl+j"));
        assert!(debug.contains("Insert newline"));
        assert!(!debug.contains("Review event log"));
        assert!(!debug.contains("Review diff artifact"));
        assert!(!debug.contains("Reopen shortcut reference"));
        assert!(!debug.contains("4 / h"));
    }

    #[test]
    fn wheel_target_hits_transcript_when_hovered() {
        exact_test_wheel_target_hits_transcript_when_hovered();
    }

    #[test]
    fn wheel_target_hits_inspector_inside_live_overlay() {
        exact_test_wheel_target_hits_inspector_inside_live_overlay();
    }

    #[test]
    fn wheel_target_excludes_activity_portion_of_live_overlay() {
        exact_test_wheel_target_excludes_activity_portion_of_live_overlay();
    }

    #[test]
    fn compact_operator_rail_does_not_capture_wheel() {
        exact_test_compact_operator_rail_does_not_capture_wheel();
    }

    #[test]
    fn inspector_shows_tool_call_detail_for_selected_activity() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_detail",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_tool_detail".to_string(),
                text: "Read the file".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_detail",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_detail".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read src/lib.rs and report the first 20 lines".to_string(),
                request_digest: "digest-tool-detail-request".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_detail",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_tool_detail".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":1,"limit":20}"#.to_string(),
                args_digest: "digest-tool-detail-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_tool_detail",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_tool_detail".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            5,
            "req_tool_detail",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_tool_detail".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(
                    r#"{"lines":["use std::path::PathBuf;","use std::sync::Arc;"]}"#.to_string(),
                ),
                output_digest: Some("digest-tool-detail-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ));

        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('i')));

        let sidebar_text = super::ui_secondary::operator_sidebar_text_for_test(&app).join("\n");
        assert!(sidebar_text.contains("Live · run run_ui_tests"));
        assert!(sidebar_text.contains("Current runtime: default · gpt-5-codex"));
        assert!(sidebar_text.contains("Provider openai"));
        assert!(sidebar_text.contains("Context"));
        assert!(sidebar_text.contains("0 active todos · 0 modified files"));
        assert!(!sidebar_text.contains("Todo ·"));
        assert!(!sidebar_text.contains("Modified Files ·"));
    }

    #[test]
    fn permission_detail_remains_available_outside_modal() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_permission_detail",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_permission_detail".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Apply the edit".to_string(),
                request_digest: "digest-permission-detail-request".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_permission_detail",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_permission_detail".to_string(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: r#"{"path":"demo.txt","ops":[{"Replace":{"line":2}}]}"#.to_string(),
                args_digest: "digest-permission-detail-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_permission_detail",
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_permission_detail".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tc_permission_detail".to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-permission-detail".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            }),
        ));

        app.handle_key(key(KeyCode::Esc));
        app.ingest_event(envelope(
            4,
            "req_permission_detail",
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_permission_detail".to_string(),
                decision: harness_core::event::PermissionDecision::Deny,
                reason: Some("operator denied in test".to_string()),
            }),
        ));
        assert_eq!(app.activities.len(), 1);
        assert_eq!(app.activities[0].tool_calls.len(), 1);
        assert_eq!(app.activities[0].tool_calls[0].permissions.len(), 1);
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('i')));

        let sidebar_text = super::ui_secondary::operator_sidebar_text_for_test(&app).join("\n");
        assert!(sidebar_text.contains("Live · run run_ui_tests"));
        assert!(sidebar_text.contains("Current runtime: default · gpt-5-codex"));
        assert!(sidebar_text.contains("Provider openai"));
        assert!(sidebar_text.contains("Context"));
        assert!(sidebar_text.contains("0 active todos · 0 modified files"));
        assert!(!sidebar_text.contains("No modified files recorded"));
        assert!(!sidebar_text.contains("Permission context:"));
        assert!(!sidebar_text.contains("perm_permission_detail"));
        assert!(!sidebar_text.contains("Resolved: deny"));
    }

    #[test]
    fn transcript_tool_rows_keep_status_but_not_raw_json_dump() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_compact",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_tool_compact".to_string(),
                text: "Read the file".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_compact",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_compact".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-tool-compact".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_compact",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_compact".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#.to_string(),
                args_digest: "digest-tool-compact-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_tool_compact",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_compact".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            5,
            "req_tool_compact",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_compact".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("12 lines read".to_string()),
                output_digest: Some("digest-tool-compact-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ));

        let transcript = transcript_debug(&app);
        assert!(transcript.contains("Read src/lib.rs [offset=42, limit=20]"));
        assert!(!transcript.contains(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#));
        assert!(!transcript.contains("args {"));
        assert_eq!(
            format_detail_payload(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#),
            "{\n  \"limit\": 20,\n  \"path\": \"src/lib.rs\",\n  \"start_line\": 42\n}"
        );
    }

    #[test]
    fn failed_tool_rows_still_surface_error_summary() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_error",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Run the command".to_string(),
                request_digest: "digest-tool-error".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_error",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_error".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false","cwd":"/tmp/demo"}"#.to_string(),
                args_digest: "digest-tool-error-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_error",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_error".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            4,
            "req_tool_error",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_error".to_string(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: permission denied".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));

        let transcript = transcript_debug(&app);
        assert!(transcript.contains("$ false"));
        assert!(transcript.contains("exit code: 1"));
        assert!(transcript.contains("stderr: permission denied"));
        assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
        assert!(!transcript.contains("args {"));
    }

    #[test]
    fn status_strip_surfaces_selected_tool_summary() {
        let mut app = AppState::new_live(None, false, None);

        app.ingest_event(envelope(
            1,
            "req_tool_status",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_tool_status".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Check tool status".to_string(),
                request_digest: "digest-tool-status".to_string(),
            }),
        ));
        app.ingest_event(envelope(
            2,
            "req_tool_status",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_status".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-tool-status-args".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            3,
            "req_tool_status",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_status".to_string(),
            }),
        ));

        let debug = render_debug(&app, 160, 30);
        assert!(debug.contains("shell.run") || debug.contains("running"));
        assert!(!debug.contains("orch 0a 0q 0r 0s"));
    }
}
