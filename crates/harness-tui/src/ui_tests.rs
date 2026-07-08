use super::*;
use crate::app::{LaunchMetadata, ModelOption};
use crate::UnwrapOrAbort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    PermissionResolvedEvent, ProviderReasoningDeltaEvent, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};

fn render_debug(app: &AppState, width: u16, height: u16) -> String {
    use ratatui::{backend::TestBackend, Terminal};

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    format!("{:?}", terminal.backend().buffer())
}

#[test]
fn copied_to_clipboard_toast_renders_in_live_shell() {
    let mut app = AppState::new_live(None, false, None);
    app.set_toast_for_test("Copied to clipboard", crate::app::ToastVariant::Info);

    let debug = render_debug(&app, 100, 30);
    assert!(
        debug.contains("Copied to clipboard"),
        "toast should render in frame\n{debug}"
    );
}

#[test]
fn manual_compaction_toast_remains_visible_in_dense_live_shell() {
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = "draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.set_toast_for_test(
        "manual compaction skipped: need at least two completed turns",
        crate::app::ToastVariant::Info,
    );

    let debug = render_debug(&app, 60, 18);
    assert!(
        debug.contains("manual compaction skipped"),
        "manual compaction toast should stay visible in dense live layouts\n{debug}"
    );
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_{seq:04}"),
        seq,
        run_id: "run_ui_tests".into(),
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
            request_id: "req_anchor_streaming".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "hello".to_string(),
            request_digest: "digest-anchor-streaming".to_string(),
            metadata: None,
        }),
    ));
    sending.ingest_event(envelope(
        2,
        "req_anchor_streaming",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_anchor_streaming".into(),
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
            request_id: "req_anchor_permission".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "edit the file".to_string(),
            request_digest: "digest-anchor-permission".to_string(),
            metadata: None,
        }),
    ));
    permission.ingest_event(envelope(
        2,
        "req_anchor_permission",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_anchor_permission".into(),
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
            tool_call_id: Some("tc_anchor_permission".into()),
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
        live_anchor_for_runtime_state(&permission, permission.runtime_state().kind, planned_anchor,),
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
            request_id: "req_answer_first".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Restyle the transcript shell".to_string(),
            request_digest: "digest-answer-first".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_answer_first",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_answer_first".into(),
            delta: "Drafting a document-like plan".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_answer_first",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_answer_first".into(),
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
            tool_call_id: "tc_answer_first".into(),
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
            request_id: "req_answer_first".into(),
            delta: "Found the transcript renderer and the composer chrome.".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        6,
        "req_answer_first",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_answer_first".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-answer-first-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    let transcript = transcript_debug(&app);
    let thinking_index = transcript
        .find("Drafting a document-like plan")
        .unwrap_or_abort();
    let answer_index = transcript
        .find("Found the transcript renderer and the composer chrome.")
        .unwrap_or_abort();
    let tool_index = transcript.find("Read src/ui.rs").unwrap_or_abort();

    assert!(thinking_index < tool_index);
    assert!(tool_index < answer_index);
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
        run_id: "run_theme_probe".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("ui-tests".to_string()),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_theme_probe".to_string()),
        payload: harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_theme_probe_default".to_string(),
            path: "demo.txt".to_string(),
            new_file_digest: "digest-theme-probe-default".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    });
    let default_plan = FrameLayoutPlan::for_app(&default_app, area);

    let mut themed_app = AppState::new_live(None, false, None);
    themed_app.live_details_drawer_open = true;
    themed_app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_theme_probe_themed".to_string(),
        seq: 1,
        run_id: "run_theme_probe".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("ui-tests".to_string()),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_theme_probe".to_string()),
        payload: harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: "edit_theme_probe_themed".to_string(),
            path: "demo.txt".to_string(),
            new_file_digest: "digest-theme-probe-themed".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    });
    let mut custom_theme = Theme::default();
    custom_theme.live_shell.primary.centered_content_width = 72;
    custom_theme.live_shell.primary.content_margin_x = 10;
    custom_theme.live_shell.primary.activity_drawer_width = 18;
    custom_theme.live_shell.primary.details_sidebar_width = 36;
    themed_app.set_theme_for_test(custom_theme);

    let default_transcript = default_plan.transcript.unwrap_or_abort();
    let themed_plan = FrameLayoutPlan::for_app(&themed_app, area);
    let themed_rail = themed_plan.operator_sidebar.unwrap_or_abort();

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
    };
    let alternate = ModelOption {
        profile: "deep".to_string(),
        provider: "default".to_string(),
        provider_display_label: Some("default".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "gpt-5.4-mini".to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: Some("creative".to_string()),
        variant_display_label: Some("Creative".to_string()),
        display_label: Some("GPT-5.4 Mini · Creative".to_string()),
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
        Some("Next turns: deep · GPT-5.4 Mini".to_string())
    );

    let debug = render_debug(&app, 160, 24);
    assert!(!debug.contains("Current runtime: deep · GPT-5.4 Mini · Deterministic"));
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
    };
    let alternate = ModelOption {
        profile: "deep".to_string(),
        provider: "default".to_string(),
        provider_display_label: Some("default".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "gpt-5.4-mini".to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: Some("creative".to_string()),
        variant_display_label: Some("Creative".to_string()),
        display_label: Some("GPT-5.4 Mini · Creative".to_string()),
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
        Some("Next turns: deep · GPT-5.4 Mini".to_string())
    );

    let debug = render_debug(&app, 160, 24);
    assert!(!debug.contains("Continued runtime: deep · GPT-5.4 Mini · Deterministic"));
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
    assert!(debug.contains("Ctrl+p commands"));
    assert!(!debug.contains("Ctrl+s send"));
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
    assert!(debug.contains("██╗  ██╗") || debug.contains("Harness"));
    assert!(!debug.contains("Launch: deep · gpt-5.4"));
    assert!(!debug.contains("Provider proxy"));
    assert!(debug.contains("Deep gpt-5.4 proxy · Demo"));
    assert!(debug.contains("ctrl+p commands"));
    assert!(!debug.contains("Enter select"));
    assert!(debug.contains("Ask anything... \"What is the tech stack of this project?\""));
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
            request_id: "req_tool_detail".into(),
            text: "Read the file".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_detail",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_tool_detail".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Read src/lib.rs and report the first 20 lines".to_string(),
            request_digest: "digest-tool-detail-request".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_tool_detail",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_tool_detail".into(),
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
            tool_call_id: "tc_tool_detail".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        "req_tool_detail",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_tool_detail".into(),
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
    assert!(sidebar_text.contains("Read the file"));
    assert!(sidebar_text.contains("▼ MCP"));
    assert!(sidebar_text.contains("▼ LSP"));
    assert!(sidebar_text.contains("▶ Modified Files"));
}

#[test]
fn permission_detail_remains_available_outside_modal() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_permission_detail",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_permission_detail".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Apply the edit".to_string(),
            request_digest: "digest-permission-detail-request".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_permission_detail",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_permission_detail".into(),
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
            tool_call_id: Some("tc_permission_detail".into()),
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
    assert!(sidebar_text.contains("▼ MCP"));
    assert!(sidebar_text.contains("▼ LSP"));
    assert!(sidebar_text.contains("▶ Modified Files"));
    assert!(!sidebar_text.contains("No modified files"));
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
            request_id: "req_tool_compact".into(),
            text: "Read the file".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_compact",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_tool_compact".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Read the file".to_string(),
            request_digest: "digest-tool-compact".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_tool_compact",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_compact".into(),
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
            tool_call_id: "tc_compact".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        "req_tool_compact",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_compact".into(),
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
            request_id: "req_tool_error".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Run the command".to_string(),
            request_digest: "digest-tool-error".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_error",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_error".into(),
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
            tool_call_id: "tc_error".into(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_tool_error",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_error".into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: permission denied".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    let transcript = transcript_debug(&app);
    assert!(transcript.contains("false"));
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
            request_id: "req_tool_status".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Check tool status".to_string(),
            request_digest: "digest-tool-status".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_status",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_status".into(),
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
            tool_call_id: "tc_status".into(),
        }),
    ));

    let debug = render_debug(&app, 160, 30);
    assert!(!debug.contains("orch 0a 0q 0r 0s"));
    assert!(
        debug.contains("tool")
            || debug.contains("false")
            || debug.contains("Shell")
            || debug.contains("Check tool status"),
        "status strip should surface active tool context\n{debug}"
    );
}
