use super::*;
use crate::UnwrapOrAbort;

use ratatui::{backend::TestBackend, layout::Rect, Terminal};

fn rect_center(area: Rect) -> (u16, u16) {
    (
        area.x.saturating_add(area.width.saturating_sub(1) / 2),
        area.y.saturating_add(area.height.saturating_sub(1) / 2),
    )
}

pub(crate) fn exact_test_startup_shell_keeps_no_default_tab_chrome_after_runtime_context_addition()
{
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_option(
        &crate::app::ModelOption {
            profile: "deep".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: Some("Deep work".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            thinking: None,
            recommended_for: None,
        },
    ));

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    let debug = format!("{:?}", terminal.backend().buffer());

    assert!(!debug.contains("Launch: deep · GPT-5.4 Mini · Deterministic"));
    assert!(!debug.contains("Provider default"));
    assert!(debug.contains("Ask anything... \"What is the tech stack of this project?\""));
    assert!(!debug.contains("Tabs"));
    assert!(!debug.contains("Actions:"));
    assert!(!debug.contains("Enter select"));
}

pub(crate) fn exact_test_replay_prompt_pane_is_visibly_read_only() {
    let mut app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_option(
        &crate::app::ModelOption {
            profile: "archive".to_string(),
            provider: "default".to_string(),
            provider_display_label: Some("default".to_string()),
            provider_backend_label: Some("OpenAI".to_string()),
            model: "gpt-5.4-mini".to_string(),
            model_display_label: Some("GPT-5.4 Mini".to_string()),
            variant: Some("deterministic".to_string()),
            variant_display_label: Some("Deterministic".to_string()),
            display_label: Some("GPT-5.4 Mini · Deterministic".to_string()),
            token_window_label: None,
            context_window_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            description: None,
            profile_description: Some("Archive".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            thinking: None,
            recommended_for: None,
        },
    ));

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(debug.contains("Replay · read-only"));
    assert!(debug.contains("▼ MCP"));
    assert!(debug.contains("▼ LSP"));
    assert!(debug.contains("▶ Modified Files"));
    assert!(debug.contains("Replay is read-only"));
    assert!(!debug.contains("Type a prompt for the next turn"));
    assert!(!debug.contains("Recent context for the next turn"));
}

pub(crate) fn exact_test_wheel_target_hits_transcript_when_hovered() {
    let app = AppState::new_live(None, false, None);
    let area = Rect::new(0, 0, 140, 40);
    let hit_areas = FrameLayoutPlan::for_app(&app, area).wheel_hit_areas;
    let transcript = hit_areas.transcript.unwrap_or_abort();
    let (column, row) = rect_center(transcript);

    assert_eq!(
        hovered_wheel_target(&app, area, column, row),
        Some(WheelTarget::Transcript)
    );
}

pub(crate) fn exact_test_wheel_target_hits_inspector_inside_live_overlay() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    let area = Rect::new(0, 0, 140, 40);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let rail = plan.operator_sidebar.unwrap_or_abort();
    let hit_areas = plan.wheel_hit_areas;
    let (column, row) = rect_center(rail);

    assert_eq!(hit_areas.overlay, None);
    assert_eq!(hit_areas.inspector, None);
    assert_eq!(hovered_wheel_target(&app, area, column, row), None);
}

pub(crate) fn exact_test_wheel_target_excludes_activity_portion_of_live_overlay() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    let area = Rect::new(0, 0, 140, 40);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let rail = plan.operator_sidebar.unwrap_or_abort();
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

pub(crate) fn exact_test_compact_operator_rail_does_not_capture_wheel() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let app = AppState::new_live(None, false, None);
    let area = Rect::new(0, 0, 140, 40);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let rail = plan.operator_sidebar.unwrap_or_abort();
    let (column, row) = rect_center(rail);

    assert_eq!(plan.wheel_hit_areas.overlay, None);
    assert_eq!(plan.wheel_hit_areas.inspector, None);
    assert_eq!(hovered_wheel_target(&app, area, column, row), None);
}

pub(crate) fn exact_test_persistent_operator_sidebar_uses_panel_gutter() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_sidebar_panel".to_string(),
        seq: 1,
        run_id: "run_sidebar_panel".into(),
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
    let sidebar = plan.operator_sidebar.unwrap_or_abort();
    let transcript = plan.transcript.unwrap_or_abort();

    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();

    let buffer = terminal.backend().buffer();
    let boundary_x = transcript.x.saturating_add(transcript.width);
    let sample_y = sidebar.y.saturating_add(1);

    assert_eq!(boundary_x, sidebar.x);
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
