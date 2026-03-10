use std::borrow::Cow;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
    Frame,
};

use crate::app::{
    session_history_profile_label, session_history_provider_model_label,
    session_history_resumability_label, session_history_run_name, session_history_status_label,
    ActivityEntry, ActivityStatus, AppState, Focus, OrchestrationTaskRow, OrchestrationTaskState,
    PostRunHandoffAction, RuntimeStateKind, StartupLauncherAction, Tab, ToolCallDisplayStatus,
};
use crate::keybindings::Action;
use crate::layout::{
    composer_input_height, details_drawer_areas, inset_rect, lifecycle_card_area,
    live_empty_state_area, replay_workspace_layout, secondary_surface_layout,
    split_secondary_surface, startup_shell_area, FrameLayoutPlan,
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
    compact_inline_payload, elevated_card_block, muted_meta_style, panel_block, panel_style,
    render_footer, render_header, render_prompt_pane, render_status_strip, render_tabs,
    request_id_label, runtime_state_color, status_badge, subdued_payload_style,
    tool_detail_label_style, tool_footer_summary, tool_state_summary, tool_status_badge,
    tool_status_tokens, transcript_label_style, transcript_prefix_style, truncate_plain_text,
};
use ui_lifecycle::{
    live_empty_state_visible, render_continued_live_reopen_surface, render_live_empty_state,
    render_post_run_handoff_surface, render_startup_lifecycle_surface, startup_shell_visible,
};
use ui_overlays::render_overlays;
use ui_secondary::{
    render_diff_tab, render_events_tab, render_help_tab, render_live_details_overlay,
    render_replay_secondary_column,
};
pub use ui_transcript::hovered_wheel_target;
use ui_transcript::{append_text_block, render_transcript_pane};

#[cfg(test)]
use ui_secondary::format_detail_payload;
#[cfg(test)]
pub(crate) use ui_secondary::orchestration_card_text_for_test;
#[cfg(test)]
use ui_transcript::build_transcript_lines;

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

    render_header(frame, app, plan.header, plan.header_text, theme);
    render_content(frame, app, plan.content, theme, &plan);
    render_footer(frame, app, plan.footer, plan.footer_text, theme);
    render_overlays(frame, app, theme, &plan);
}

fn render_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    if app.replay_mode {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(theme.live_shell.heights.tabs),
                Constraint::Min(0),
            ])
            .split(area);

        render_tabs(frame, app, chunks[0], theme);
        render_surface(frame, app, chunks[1], theme, plan);
    } else {
        render_surface(frame, app, area, theme, plan);
    }
}

fn render_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    let area = if app.replay_mode { area } else { plan.shell };

    match app.active_tab {
        Tab::Run => {
            if app.replay_mode {
                render_run_workspace(frame, app, area, theme)
            } else {
                render_live_session_surface(frame, app, theme, plan)
            }
        }
        Tab::Details => render_live_session_surface(frame, app, theme, plan),
        Tab::Events => render_events_tab(frame, app, area, theme),
        Tab::Diff => render_diff_tab(frame, app, area, theme),
        Tab::Help => render_help_tab(frame, app, area, theme),
    }
}

/// Render the Run workspace with 3-pane layout + prompt
fn render_run_workspace(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let shell = app.theme().live_shell_layout(area.width, area.height);
    let replay = replay_workspace_layout(area, theme, shell);

    render_transcript_pane(frame, app, replay.transcript, theme);
    render_replay_secondary_column(frame, app, replay.secondary, theme);
    render_prompt_pane(frame, app, replay.composer, theme);
}

fn render_live_session_surface(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    let Some(transcript_area) = plan.transcript else {
        return;
    };
    let Some(status_area) = plan.status else {
        return;
    };

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.shell)),
        plan.shell,
    );
    render_transcript_pane(frame, app, transcript_area, theme);
    render_runtime_state_surface(frame, app, transcript_area, theme);
    if app.continued_live_reopen_surface_visible() {
        render_continued_live_reopen_surface(frame, app, transcript_area, theme);
    }
    render_live_details_overlay(frame, app, theme, plan.details_overlay);
    render_status_strip(frame, app, status_area, theme);
    if !app.post_run_handoff_visible() {
        if let Some(composer_area) = plan.composer {
            render_prompt_pane(frame, app, composer_area, theme);
        }
    }
}

fn render_runtime_state_surface(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if app.replay_mode
        || app.startup_shell_visible()
        || app.post_run_handoff_visible()
        || app.active_permission().is_some()
    {
        return;
    }

    let state = app.runtime_state();
    let Some((title, guidance, accent)) = runtime_state_surface_copy(app, &state) else {
        return;
    };

    let width = area.width.saturating_sub(6).min(68);
    let height = area.height.saturating_sub(4).min(7);
    if width < 32 || height < 5 {
        return;
    }

    let popup = Rect::new(
        area.x
            .saturating_add((area.width.saturating_sub(width)) / 2),
        area.y
            .saturating_add((area.height.saturating_sub(height)) / 2),
        width,
        height,
    );
    let surface = theme.surface.panel_elevated;
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let emphasis_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let detail = state.detail.unwrap_or_else(|| state.summary.clone());
    let block = elevated_card_block(
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
        surface,
        accent,
        accent,
    );
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![Span::styled(
                truncate_plain_text(&state.summary, usize::from(sections[0].width)),
                emphasis_style,
            )]),
            Line::from(vec![Span::styled(
                truncate_plain_text(&detail, usize::from(sections[0].width)),
                metadata_style,
            )]),
            Line::from(vec![Span::styled(
                truncate_plain_text(&state.composer_hint, usize::from(sections[0].width)),
                metadata_style,
            )]),
        ]))
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

fn runtime_state_surface_copy(
    app: &AppState,
    state: &crate::app::RuntimeState,
) -> Option<(&'static str, &'static str, Color)> {
    match state.kind {
        RuntimeStateKind::Degraded => Some((
            "Live recovery in progress",
            "Sending paused · keep drafting locally until catch-up completes",
            app.theme().status.warning,
        )),
        RuntimeStateKind::Disconnected => Some((
            "Connection lost",
            "Reconnect required · reopen the TUI, then continue from the visible transcript",
            app.theme().status.error,
        )),
        RuntimeStateKind::Failure => Some((
            "Turn attention required",
            if state.composer_disabled {
                "Follow the recovery guidance above before continuing"
            } else {
                "Composer stays available · adjust the draft and retry when ready"
            },
            app.theme().status.error,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LaunchMetadata;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
        PermissionResolvedEvent, ProviderRequestStartedEvent, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
        SCHEMA_VERSION,
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

    fn rect_center(area: Rect) -> (u16, u16) {
        (
            area.x.saturating_add(area.width.saturating_sub(1) / 2),
            area.y.saturating_add(area.height.saturating_sub(1) / 2),
        )
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
        default_app.active_tab = Tab::Details;
        let default_hit_areas = FrameLayoutPlan::for_app(&default_app, area).wheel_hit_areas;

        let mut themed_app = AppState::new_live(None, false, None);
        themed_app.active_tab = Tab::Details;
        let mut custom_theme = Theme::default();
        custom_theme.live_shell.primary.centered_content_width = 72;
        custom_theme.live_shell.primary.content_margin_x = 10;
        custom_theme.live_shell.primary.activity_drawer_width = 18;
        custom_theme.live_shell.primary.details_sidebar_width = 36;
        themed_app.set_theme(custom_theme);

        let themed_hit_areas = FrameLayoutPlan::for_app(&themed_app, area).wheel_hit_areas;
        assert_ne!(default_hit_areas.overlay, themed_hit_areas.overlay);
        assert_ne!(default_hit_areas.inspector, themed_hit_areas.inspector);

        let themed_inspector = themed_hit_areas.inspector.expect("themed inspector area");
        let probe_column = themed_inspector.x.saturating_add(2);
        let probe_row = themed_inspector.y.saturating_add(1);

        assert_eq!(
            hovered_wheel_target(&themed_app, area, probe_column, probe_row),
            Some(WheelTarget::Inspector)
        );
        assert_ne!(
            hovered_wheel_target(&default_app, area, probe_column, probe_row),
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
        assert!(!debug.contains("Demo"));
        assert!(!debug.contains("run unknown"));
        assert!(debug.contains("Preset deep · proxy/gpt-5.4"));
        assert!(!debug.contains("default/default"));
    }

    #[test]
    fn footer_hints_follow_keymap_overrides() {
        let mut app = AppState::new_live(None, false, None);
        app.apply_keybindings(
            [
                ("submit_prompt".to_string(), "ctrl+s".to_string()),
                ("insert_newline".to_string(), "ctrl+j".to_string()),
                ("toggle_details_drawer".to_string(), "d".to_string()),
                ("tab_help".to_string(), "g".to_string()),
                ("quit".to_string(), "x".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Ctrl+s send"));
        assert!(debug.contains("Ctrl+j nl"));
        assert!(debug.contains("d details"));
        assert!(debug.contains("g help"));
        assert!(debug.contains("x quit"));
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
        assert!(demo_debug.contains("Start a conversation to begin"));
        assert!(!demo_debug.contains("Demo mode · mock provider"));

        let mut mock = AppState::new_live(None, false, None);
        mock.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Mock"),
        );

        let mock_debug = render_debug(&mock, 100, 24);
        assert!(mock_debug.contains("Harness"));
        assert!(mock_debug.contains("Start a conversation to begin"));
        assert!(!mock_debug.contains("Mock mode · mock provider"));
        assert!(!mock_debug.contains("Preset worker · mock/model-1 · Mock"));
    }

    #[test]
    fn startup_shell_shows_profile_provider_and_model_chrome() {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
        );

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Harness"));
        assert!(debug.contains("Preset deep · proxy/gpt-5.4 · Demo"));
        assert!(debug.contains("Dispatch a new run"));
        assert!(debug.contains("New session"));
        assert!(debug.contains("Continue session"));
        assert!(debug.contains("Replay session"));
    }

    #[test]
    fn replay_prompt_pane_is_visibly_read_only() {
        let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());

        let debug = render_debug(&app, 100, 24);
        assert!(debug.contains("Replay archive · read-only"));
        assert!(debug.contains("Replay is read-only"));
        assert!(!debug.contains("Type a prompt for the next turn"));
    }

    #[test]
    fn help_surface_lists_active_bindings() {
        let mut app = AppState::new_live(None, false, None);
        app.active_tab = Tab::Help;
        app.apply_keybindings(
            [
                ("tab_events".to_string(), "e".to_string()),
                ("tab_diff".to_string(), "f".to_string()),
                ("tab_help".to_string(), "g".to_string()),
                ("toggle_follow".to_string(), "z".to_string()),
                ("submit_prompt".to_string(), "ctrl+s".to_string()),
                ("insert_newline".to_string(), "ctrl+j".to_string()),
            ]
            .into_iter()
            .collect(),
        );

        let debug = render_debug(&app, 100, 30);
        assert!(debug.contains("z"));
        assert!(debug.contains("Toggle follow mode"));
        assert!(debug.contains("e"));
        assert!(debug.contains("Open Events surface"));
        assert!(debug.contains("f"));
        assert!(debug.contains("Open Diff surface"));
        assert!(debug.contains("g"));
        assert!(debug.contains("Open Help surface"));
        assert!(debug.contains("Ctrl+s"));
        assert!(debug.contains("Submit prompt"));
        assert!(debug.contains("Ctrl+j"));
        assert!(debug.contains("Insert newline"));
        assert!(!debug.contains("4 / h"));
    }

    #[test]
    fn wheel_target_hits_transcript_when_hovered() {
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

    #[test]
    fn wheel_target_hits_inspector_inside_live_overlay() {
        let mut app = AppState::new_live(None, false, None);
        app.active_tab = Tab::Details;

        let area = Rect::new(0, 0, 140, 40);
        let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
        let inspector = hit_areas.inspector.expect("inspector area");
        let (column, row) = rect_center(inspector);

        assert_eq!(
            hovered_wheel_target(&app, area, column, row),
            Some(WheelTarget::Inspector)
        );
    }

    #[test]
    fn wheel_target_excludes_activity_portion_of_live_overlay() {
        let mut app = AppState::new_live(None, false, None);
        app.active_tab = Tab::Details;

        let area = Rect::new(0, 0, 140, 40);
        let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
        let overlay = hit_areas.overlay.expect("overlay area");

        assert_eq!(
            hovered_wheel_target(
                &app,
                area,
                overlay.x.saturating_add(1),
                overlay.y.saturating_add(1),
            ),
            None
        );
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
            }),
        ));

        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Char('i')));

        let request_debug = render_debug(&app, 140, 40);
        assert!(request_debug.contains("Request metadata:"));
        assert!(request_debug.contains("digest-tool-detail-request"));

        let inspector_screens = (0..32)
            .map(|scroll| {
                app.details_scroll = scroll;
                render_debug(&app, 140, 40)
            })
            .collect::<Vec<_>>();
        let tool_debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("Args:"))
            .expect("tool detail section should be reachable via scroll");
        assert!(tool_debug.contains("Tool calls:"));
        assert!(tool_debug.contains("Args:"));
        assert!(tool_debug.contains("State: succeeded"));
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("src/lib.rs") || debug.contains("\"path\":")),
            "expected inspector to expose tool args path"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("digest-tool-detail-args")),
            "expected inspector to expose args digest"
        );

        let output_debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("Result:"))
            .expect("tool output section should be reachable via scroll");
        assert!(output_debug.contains("Result:"));
        assert!(output_debug.contains("digest-tool-detail-output"));
        assert!(
            inspector_screens.iter().any(
                |debug| debug.contains("\"lines\"") || debug.contains("use std::path::PathBuf")
            ),
            "expected inspector to expose raw output payload"
        );
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

        let inspector_screens = (0..40)
            .map(|scroll| {
                app.details_scroll = scroll;
                render_debug(&app, 140, 40)
            })
            .collect::<Vec<_>>();
        let debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("Permission context:"))
            .expect("permission detail section should be reachable via scroll");
        assert!(debug.contains("Permission context:"));
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("perm_permission_detail")),
            "expected inspector to expose permission id"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("digest-permission-detail")),
            "expected inspector to expose permission digest"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("Default: deny")),
            "expected inspector to expose default decision"
        );
        assert!(
            inspector_screens
                .iter()
                .any(|debug| debug.contains("Resolved: deny")),
            "expected inspector to expose resolved decision"
        );

        let reason_debug = inspector_screens
            .iter()
            .find(|debug| debug.contains("operator denied in test"))
            .expect("permission reason should be reachable via scroll");
        assert!(reason_debug.contains("operator denied in test"));
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
            }),
        ));

        let transcript = transcript_debug(&app);
        assert!(transcript.contains("tool fs.read"));
        assert!(transcript.contains("args   limit=20, path=src/lib.rs, start_line=42"));
        assert!(transcript.contains("result 12 lines read"));
        assert!(transcript.contains("12 lines read"));
        assert!(transcript.contains("succeeded"));
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
            }),
        ));

        let transcript = transcript_debug(&app);
        assert!(transcript.contains("tool shell.run"));
        assert!(transcript.contains("args   cmd=false, cwd=/tmp/demo"));
        assert!(transcript.contains("error  exit code: 1 stderr: permission denied"));
        assert!(transcript.contains("exit code: 1 stderr: permission denied"));
        assert!(transcript.contains("failed"));
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
        assert!(debug.contains("live"));
        assert!(debug.contains("orch 0a 0q 0r 0s"));
        assert!(debug.contains("tool shell.run running"));
    }
}
