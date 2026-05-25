use super::*;
use crate::layout::FrameLayoutPlan;
use crate::overlay::OverlayKind;
use crate::theme::Theme;
use crate::ui::{
    render_app, reset_transcript_selection_cache_metrics_for_test, transcript_mouse_target,
    transcript_selection_cache_build_count_for_test, transcript_selection_cell,
    transcript_selection_debug_snapshot, TranscriptMouseTarget, TranscriptScrollbarHit,
    WheelTarget,
};
use crate::view_model;
use crossterm::event::{MouseButton, MouseEvent};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EditAppliedEvent, EventActor, EventEnvelopeV1, EventV1,
    ExecutionTimingMetadata, PermissionRequestedEvent, PermissionResolvedEvent,
    ProviderReasoningDeltaEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent,
    TaskTerminalScope, ToolCallFinishedEvent, ToolCallLifecycleState, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::proj::inspect_resume_plan;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::{backend::TestBackend, Terminal};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const TEST_FRAME_AREA: Rect = Rect::new(0, 0, 140, 40);

struct ClipboardModeGuard;

impl ClipboardModeGuard {
    fn disabled_copy_on_select() -> Self {
        crate::clipboard::set_copy_on_select_disabled_override(Some(true));
        Self
    }
}

impl Drop for ClipboardModeGuard {
    fn drop(&mut self) {
        crate::clipboard::set_copy_override(None);
        crate::clipboard::set_copy_on_select_disabled_override(None);
    }
}

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_app_{seq:04}"),
        seq,
        run_id: "run_app_tests".to_string(),
        mono_ms: seq,
        ts: Some("2026-02-03T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("app-tests".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn envelope_with_actor(
    seq: u64,
    request_id: &str,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    let mut event = envelope(seq, request_id, payload);
    event.actor = actor;
    event
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn render_debug(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw frame");
    format!("{:?}", terminal.backend().buffer())
}

#[test]
fn toggles_slash_command_opens_command_styled_menu() {
    let mut app = AppState::new();
    app.focus = Focus::Prompt;
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()])
            .with_available_models(vec![
                ModelOption::from_model_ref("build", "default:gpt-5.4-mini"),
                ModelOption::from_model_ref("explore", "default:gpt-5.4-mini"),
            ]),
    );

    for ch in "/toggles".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.toggles_menu_visible);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::TogglesMenu));
    let rendered = render_debug(&app, 100, 40);
    assert!(rendered.contains("Built-in dynamic prompts"));
    assert!(rendered.contains("YOLO mode"));
    assert!(rendered.contains("build"));
    assert!(rendered.contains("explore"));
}

#[test]
fn yolo_toggle_requires_confirmation_and_enables_entries() {
    let mut app = AppState::new();
    app.set_toggles_config(TogglesConfig {
        entries: vec![
            ToggleEntryConfig {
                kind: ToggleEntryKind::Agent {
                    name: "build".to_string(),
                },
                label: "build".to_string(),
                description: "Primary agent".to_string(),
                enabled: false,
            },
            ToggleEntryConfig {
                kind: ToggleEntryKind::YoloMode,
                label: "YOLO mode".to_string(),
                description: "Enable all session toggles".to_string(),
                enabled: false,
            },
        ],
    });
    app.open_toggles_menu();
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    assert!(app.toggles_yolo_confirmation_visible());
    assert!(render_debug(&app, 100, 28).contains("Confirm YOLO mode"));

    app.handle_key(key(KeyCode::Enter));
    assert!(!app.toggles_yolo_confirmation_visible());
    assert!(app.toggle_menu_rows().iter().all(|row| row.enabled));
}

#[test]
fn toggles_config_preserves_launch_metadata_entries() {
    let mut app = AppState::new();
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini")
            .with_switchable_profiles(vec!["build".to_string()])
            .with_available_models(vec![ModelOption::from_model_ref(
                "explore",
                "default:gpt-5.4-mini",
            )]),
    );

    app.set_toggles_config(TogglesConfig::default());

    let rows = app.toggle_menu_rows();
    assert!(rows
        .iter()
        .any(|row| row.label == "Built-in dynamic prompts"));
    assert!(rows.iter().any(|row| row.label == "build"));
    assert!(rows.iter().any(|row| row.label == "explore"));
}

#[test]
fn toggles_menu_sanitizes_config_derived_text() {
    let mut app = AppState::new();
    app.set_toggles_config(TogglesConfig {
        entries: vec![ToggleEntryConfig {
            kind: ToggleEntryKind::Hook {
                id: "hook\u{1b}".to_string(),
            },
            label: "hook\u{1b}[31m".to_string(),
            description: "first\nsecond".to_string(),
            enabled: true,
        }],
    });
    app.open_toggles_menu();

    let rendered = render_debug(&app, 140, 40);
    assert!(rendered.contains("hook[31m"));
    assert!(rendered.contains("first"));
    assert!(rendered.contains("second"));
    assert!(!rendered.contains('\u{1b}'));
    assert!(!rendered.contains("first\\nsecond"));
}

fn transcript_click_position(app: &AppState, needle: &str) -> (u16, u16) {
    transcript_click_position_in_area(app, TEST_FRAME_AREA, needle)
}

fn transcript_click_position_in_area(app: &AppState, area: Rect, needle: &str) -> (u16, u16) {
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw transcript frame");
    let buffer = terminal.backend().buffer();

    for y in 0..area.height {
        let row = (0..area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (u16::try_from(column + 1).expect("column fits"), y);
        }
    }

    panic!("expected row containing {needle:?}");
}

fn footer_click_position(app: &AppState, needle: &str) -> (u16, u16) {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw footer frame");
    let buffer = terminal.backend().buffer();

    for y in 0..TEST_FRAME_AREA.height {
        let row = (0..TEST_FRAME_AREA.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (u16::try_from(column + 1).expect("column fits"), y);
        }
    }

    panic!("expected footer row containing {needle:?}");
}

fn rendered_cell_bg(app: &AppState, column: u16, row: u16) -> Color {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw transcript frame");
    terminal.backend().buffer()[(column, row)].bg
}

fn default_navigation_keybindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("session_child_first".to_string(), "ctrl+]".to_string()),
        ("session_child_cycle".to_string(), "]".to_string()),
        ("session_child_cycle_reverse".to_string(), "[".to_string()),
        ("session_parent".to_string(), "ctrl+[".to_string()),
        ("variant_cycle".to_string(), "tab".to_string()),
    ])
}

fn runtime_context_model_option(
    profile: &str,
    provider: &str,
    model: &str,
    variant: Option<&str>,
    display_label: &str,
) -> ModelOption {
    ModelOption {
        profile: profile.to_string(),
        provider: provider.to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: model.to_string(),
        model_display_label: None,
        variant: variant.map(str::to_string),
        variant_display_label: None,
        display_label: Some(display_label.to_string()),
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
        text_verbosity: None,
        recommended_for: None,
    }
}

#[test]
fn mouse_click_toggles_transcript_tool_disclosure() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_tool_toggle",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_tool_toggle".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Toggle shell tool".to_string(),
            request_digest: "digest-tool-toggle".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_toggle",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_shell_toggle".to_string(),
            tool_id: "shell.run".to_string(),
            args_summary: r#"{"cmd":"false"}"#.to_string(),
            args_digest: "digest-tool-toggle-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_tool_toggle",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_shell_toggle".to_string(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: nope".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    let (column, row) = transcript_click_position(&app, "false");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(app.expanded_tool_outputs.contains("tc_shell_toggle"));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(!app.expanded_tool_outputs.contains("tc_shell_toggle"));
}

#[test]
fn mouse_click_on_task_inline_row_opens_subagent_session() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_click",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "inspect child".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));

    let (column, row) = transcript_click_position(&app, "Explore Task — inspect child");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    assert_ne!(
        rendered_cell_bg(&app, column, row),
        Theme::default().surface.panel_elevated
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(
        app.hovered_transcript_target(),
        Some(&TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    assert_ne!(
        rendered_cell_bg(&app, column, row),
        Theme::default().surface.panel_elevated,
        "Harness inline task rows keep a flat surface on hover"
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}

#[test]
fn live_subagent_hitbox_uses_rendered_transcript_area() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_offset",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));

    let compact_area = Rect::new(0, 0, 80, 24);
    let (column, row) = transcript_click_position_in_area(&app, compact_area, "Explore Task");
    assert_eq!(
        transcript_mouse_target(&app, compact_area, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    assert_eq!(
        transcript_mouse_target(
            &app,
            compact_area,
            FrameLayoutPlan::for_app(&app, compact_area)
                .transcript
                .expect("transcript area")
                .x
                .saturating_add(Theme::default().live_shell.rhythm.transcript_gutter_x)
                .saturating_sub(1),
            row
        ),
        None,
        "columns left of the rendered transcript pane must not activate the subagent hitbox"
    );
}

#[test]
fn disk_backed_child_navigation_stays_in_live_tui_stack() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    let child_path = run_dir.path().join("agent_child");
    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "build"),
        provider_started(3, "req_1", "mock", "model-parent"),
        envelope(
            4,
            "req_1",
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string(),
                state: TaskScheduleState::Started,
                queue_key: None,
            }),
        ),
    ];
    write_events_jsonl(&parent_path, &parent_events);
    write_events_jsonl(
        &child_path,
        &[
            run_started(1),
            agent_spawned(2, "agent_child", "explore"),
            provider_started(3, "req_child", "mock", "model-child"),
        ],
    );
    assert!(!inspect_resume_plan(&parent_path).is_resumable);

    let mut app = AppState::new_live(Some(parent_path.clone()), false, Some(intent_sink));
    for event in parent_events {
        app.ingest_event(event);
    }
    app.navigate_to_child_session_id("agent_child".to_string());

    assert!(!app.should_quit);
    assert_eq!(app.session_path.as_deref(), Some(child_path.as_path()));
    assert!(app.replay_mode);
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.session_path == parent_path && !snapshot.replay_mode));
    assert!(intents.lock().expect("lock intents").is_empty());

    app.navigate_to_parent_session();

    assert_eq!(app.session_path.as_deref(), Some(parent_path.as_path()));
    assert!(!app.replay_mode);
    assert!(!app.should_quit);
    app.handle_key(key(KeyCode::Char('x')));
    assert_eq!(app.prompt_buffer, "x");
    let intents = intents.lock().expect("lock intents");
    assert!(intents.is_empty());
}

#[test]
fn mouse_click_on_subagent_footer_navigates_parent_previous_and_next() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_a",
        "agent_child_a",
        "req_child_a",
    ));
    app.ingest_event(child_task_requested(
        5,
        "req_parent",
        "tc_child_b",
        "agent_child_b",
        "req_child_b",
    ));
    app.ingest_event(child_agent_spawned(6, "agent_child_a", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        7,
        "req_child_a",
        EventActor::new(ActorKind::Worker, Some("agent_child_a".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_a".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child-a".to_string(),
            prompt_summary: "inspect child a".to_string(),
            request_digest: "digest-child-a-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(child_agent_spawned(8, "agent_child_b", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        9,
        "req_child_b",
        EventActor::new(ActorKind::Worker, Some("agent_child_b".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_b".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child-b".to_string(),
            prompt_summary: "inspect child b".to_string(),
            request_digest: "digest-child-b-prompt".to_string(),
            metadata: None,
        }),
    ));

    app.navigate_to_child_session_id("agent_child_a".to_string());
    assert_eq!(app.current_session_id(), Some("agent_child_a"));
    assert!(app.replay_mode, "inline child sessions open read-only");
    assert!(render_debug(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height).contains("Next ]"));
    assert!(
        !render_debug(&app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height).contains("▼ MCP"),
        "subagent chat should use the main transcript shell without the replay sidebar"
    );

    let (next_column, next_row) = footer_click_position(&app, "Next");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: next_column,
            row: next_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(app.current_session_id(), Some("agent_child_b"));

    let (previous_column, previous_row) = footer_click_position(&app, "Prev");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: previous_column,
            row: previous_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(app.current_session_id(), Some("agent_child_a"));

    let (parent_column, parent_row) = footer_click_position(&app, "Parent");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: parent_column,
            row: parent_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(app.current_session_id(), Some("parent_run"));
    assert!(!app.replay_mode);
}

#[test]
fn mouse_click_on_task_inline_row_uses_task_row_child_session() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_child_click_task_row".to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: "digest-child-click-task-row".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_tool".to_string(),
            result_summary: "child completed".to_string(),
            result_digest: "digest-child-tool-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_click_task_row".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let (column, row) = transcript_click_position(&app, "Explore Task — inspect child");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}

#[test]
fn mouse_up_on_completed_general_task_row_opens_child_session() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_general_child".to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"Subagent functionality smoke test"}"#.to_string(),
            args_digest: "digest-general-child".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "general", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_general_child".to_string(),
            result_summary: "child completed".to_string(),
            result_digest: "digest-general-child-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_general_child".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(TaskTerminalScope::ToolCall),
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(6),
                    finished_mono_ms: Some(22),
                    elapsed_ms: Some(16),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Subagent functionality smoke test".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        8,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_general_child".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child completed".to_string()),
            output_digest: Some("digest-general-child-result".to_string()),
            output_json: Some(serde_json::json!({
                "description": "Subagent functionality smoke test",
                "status": "completed",
                "child_tool_call_count": 0,
                "duration_ms": 16,
                "child_session_id": "agent_child",
                "child_request_id": "req_child"
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_general_child".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    let (column, row) =
        transcript_click_position(&app, "General Task — Subagent functionality smoke test");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );
    let (_, detail_row) = transcript_click_position(&app, "└ 0 toolcalls · 16ms");
    assert_eq!(detail_row, row + 1);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert_eq!(
        app.hovered_transcript_target(),
        Some(&TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(
        app.replay_mode,
        "mouse-up opens inline child sessions read-only"
    );
}

#[test]
fn mouse_click_on_task_row_uses_harness_session_metadata() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_harness_child".to_string(),
            tool_id: "task".to_string(),
            args_summary:
                r#"{"description":"Smoke test subagent dispatch","subagent_type":"plan"}"#
                    .to_string(),
            args_digest: "digest-harness-child".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "plan", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Smoke test subagent dispatch".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        7,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_harness_child".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child completed".to_string()),
            output_digest: Some("digest-harness-child-result".to_string()),
            output_json: Some(serde_json::json!({
                "description": "Smoke test subagent dispatch",
                "metadata": {
                    "sessionId": "agent_child",
                    "requestId": "req_child"
                },
                "duration_ms": 1700,
                "child_tool_call_count": 0
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    let (column, row) = transcript_click_position(&app, "Plan Task — Smoke test subagent dispatch");
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::SubagentSession {
            session_id: "agent_child".to_string(),
        })
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode, "inline child sessions open read-only");
}

#[test]
fn mouse_click_on_subagent_hint_opens_first_child_session() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_hint",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "inspect child".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));

    let hint = format!(
        "{} view subagents",
        app.keymap.get_binding_str(Action::SessionChildFirst)
    );
    let (column, row) = transcript_click_position(&app, &hint);
    assert_eq!(
        transcript_mouse_target(&app, TEST_FRAME_AREA, column, row),
        Some(TranscriptMouseTarget::FirstSubagentSession)
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(
        app.replay_mode,
        "hint opens inline child sessions read-only"
    );
}

#[test]
fn slash_exit_from_inline_subagent_restores_parent_before_quit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let run_dir = tempfile::tempdir().expect("create run dir");
    let parent_path = run_dir.path().join("parent_run");
    fs::create_dir_all(&parent_path).expect("create parent run dir");

    let mut app = AppState::new_live(Some(parent_path), false, Some(intent_sink));
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(child_task_requested(
        4,
        "req_parent",
        "tc_child_exit",
        "agent_child",
        "req_child",
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    app.ingest_event(envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "inspect child".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));

    app.navigate_to_child_session_id("agent_child".to_string());
    assert_eq!(app.current_session_id(), Some("agent_child"));
    assert!(app.replay_mode);

    app.execute_slash_command("exit", None);

    assert!(app.should_quit);
    assert_eq!(app.current_session_id(), Some("parent_run"));
    assert!(!app.replay_mode);
    assert!(!app.current_subagent_session_present());
    let intents = intents.lock().expect("lock intents");
    assert!(intents
        .iter()
        .any(|intent| matches!(intent, UiIntent::QuitRequested)));
}

#[test]
fn mouse_click_toggles_apply_patch_file_disclosure() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    fs::write(
        artifacts_dir.join("apply-a.diff"),
        "@@ -1,1 +1,1 @@\n-old a\n+new a\n",
    )
    .expect("write apply patch diff");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    app.ingest_event(envelope(
        1,
        "req_patch_toggle",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_patch_toggle".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Toggle patch file".to_string(),
            request_digest: "digest-patch-toggle".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_patch_toggle",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_patch_toggle".to_string(),
            tool_id: "apply_patch".to_string(),
            args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
            args_digest: "digest-patch-toggle-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_patch_toggle",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_patch_toggle".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("Success. Updated the following files".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "files": ["M notes/a.md", "M notes/b.md"],
                "edits": [
                    {
                        "edit_id": "apply-patch-a",
                        "path": "notes/a.md",
                        "summary": "apply patch update notes/a.md",
                        "deleted": false,
                        "diff_rel_path": "artifacts/apply-a.diff",
                        "diff_digest": "digest-apply-a"
                    },
                    {
                        "edit_id": "apply-patch-b",
                        "path": "notes/b.md",
                        "summary": "apply patch update notes/b.md",
                        "deleted": false
                    }
                ]
            })),
            metadata: None,
        }),
    ));

    assert!(app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));
    let (column, row) = transcript_click_position(&app, "a.md · notes");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(!app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    assert!(app.patch_file_output_expanded("tc_patch_toggle", "notes/a.md"));
}

#[test]
fn apply_patch_default_expansion_skips_deleted_files() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_patch_defaults",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_patch_defaults".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Seed patch defaults".to_string(),
            request_digest: "digest-patch-defaults".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_patch_defaults",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_patch_defaults".to_string(),
            tool_id: "apply_patch".to_string(),
            args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
            args_digest: "digest-patch-defaults-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_patch_defaults",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_patch_defaults".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("Success. Updated the following files".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "files": ["M notes/a.md", "D notes/old.md"],
                "edits": [
                    {
                        "edit_id": "apply-patch-a",
                        "path": "notes/a.md",
                        "summary": "apply patch update notes/a.md",
                        "deleted": false
                    },
                    {
                        "edit_id": "apply-patch-old",
                        "path": "notes/old.md",
                        "summary": "apply patch delete notes/old.md",
                        "deleted": true
                    }
                ]
            })),
            metadata: None,
        }),
    ));

    assert!(app.patch_file_output_expanded("tc_patch_defaults", "notes/a.md"));
    assert!(!app.patch_file_output_expanded("tc_patch_defaults", "notes/old.md"));
}

fn metadata_model_option(
    profile: &str,
    profile_description: Option<&str>,
    provider: &str,
    provider_display_label: Option<&str>,
    model: &str,
    display_label: &str,
) -> ModelOption {
    ModelOption {
        profile: profile.to_string(),
        provider: provider.to_string(),
        provider_display_label: provider_display_label.map(str::to_string),
        provider_backend_label: Some("OpenAI".to_string()),
        model: model.to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant: Some("high".to_string()),
        variant_display_label: Some("High".to_string()),
        display_label: Some(display_label.to_string()),
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: profile_description.map(str::to_string),
        reasoning_effort: Some("high".to_string()),
        text_verbosity: None,
        recommended_for: None,
    }
}

#[test]
fn question_modal_ignores_digits_past_visible_choices() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_question_digit_bounds",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_digit_bounds".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_digit_bounds".to_string()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [
                        {"label": "A", "description": "Option A"},
                        {"label": "B", "description": "Option B"},
                        {"label": "C", "description": "Option C"}
                    ],
                    "custom": false
                }]
            })
            .to_string(),
            request_digest: "digest-question-digit-bounds".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Down));
    assert_eq!(
        app.question_prompt_selection("perm_question_digit_bounds"),
        1
    );

    app.handle_key(key(KeyCode::Char('9')));

    assert_eq!(
        app.question_prompt_selection("perm_question_digit_bounds"),
        1
    );
    assert!(app.question_prompt_answers("perm_question_digit_bounds")[0].is_empty());
}

#[test]
fn question_modal_multi_custom_selection_toggles_saved_custom_answer() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_question_multi_custom",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_multi_custom".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_multi_custom".to_string()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick any",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                    "multiple": true
                }]
            })
            .to_string(),
            request_digest: "digest-question-multi-custom".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.question_prompt_editing("perm_question_multi_custom"));

    app.handle_key(key(KeyCode::Char('x')));
    app.handle_key(key(KeyCode::Enter));

    assert!(!app.question_prompt_editing("perm_question_multi_custom"));
    assert_eq!(
        app.question_prompt_answers("perm_question_multi_custom"),
        vec![vec!["x".to_string()]]
    );

    app.handle_key(key(KeyCode::Enter));

    assert!(!app.question_prompt_editing("perm_question_multi_custom"));
    assert!(app.question_prompt_answers("perm_question_multi_custom")[0].is_empty());
}

#[test]
fn question_modal_submit_allows_unanswered_questions_on_confirm() {
    let intents = std::sync::Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = std::sync::Arc::clone(&intents);
        std::sync::Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_question_partial_submit",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_partial_submit".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_partial_submit".to_string()),
            summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    },
                    {
                        "question": "Optional second",
                        "header": "Mode",
                        "options": [{"label": "B", "description": "Option B"}],
                    }
                ]
            })
            .to_string(),
            request_digest: "digest-question-partial-submit".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0],
        UiIntent::ResolvePermission {
            permission_id: "perm_question_partial_submit".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"],[]]".to_string()),
            grant_scope: None,
        }
    );
}

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir).expect("create run dir");
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
}

fn transcript_selection_test_app_with_text(transcript_text: &str) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "req_copy_select".to_string(),
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(UserMessageSubmittedEvent {
            request_id: "req_copy_select".to_string(),
            text: "Select this".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: transcript_text.to_string(),
        usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
    }]);
    app.selected_activity_index = 0;
    app
}

fn transcript_selection_test_app_with_reasoning(
    thinking_text: &str,
    transcript_text: &str,
) -> AppState {
    let mut app = transcript_selection_test_app_with_text(transcript_text);
    app.activities[0].thinking_text = thinking_text.to_string();
    app
}

fn transcript_selection_test_app() -> AppState {
    transcript_selection_test_app_with_text("Copy this exact reply")
}

fn operator_sidebar_selection_test_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_sidebar_copy",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_sidebar_copy".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-sidebar".to_string(),
            prompt_summary: "sidebar copy".to_string(),
            request_digest: "digest-sidebar-copy".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_sidebar_copy",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_sidebar_todo".to_string(),
            tool_id: "todo.write".to_string(),
            args_summary: "update todo list".to_string(),
            args_digest: "digest-sidebar-todo-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_sidebar_copy",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_sidebar_todo".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("todo list updated".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({
                "todos": [
                    {"content": "Copy sidebar task", "status": "in_progress", "priority": "high"},
                    {"content": "Keep existing sidebar clicks", "status": "pending", "priority": "medium"}
                ]
            })),
            metadata: None,
        }),
    ));
    app
}

fn transcript_selection_text_position(app: &AppState, needle: &str) -> (u16, u16) {
    let snapshot = transcript_selection_debug_snapshot(app, TEST_FRAME_AREA)
        .expect("transcript selection snapshot");
    for (row_idx, row) in snapshot.rows.iter().enumerate() {
        if let Some(column_idx) = row.find(needle) {
            return (
                snapshot.viewport.x + u16::try_from(column_idx).expect("column fits"),
                snapshot.viewport.y + u16::try_from(row_idx).expect("row fits"),
            );
        }
    }

    panic!("missing transcript text: {needle}");
}

fn transcript_selection_text_bounds(app: &AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row) = transcript_selection_text_position(app, needle);
    (
        column,
        row,
        u16::try_from(needle.chars().count()).expect("needle width fits"),
    )
}

fn operator_sidebar_text_bounds(app: &AppState, needle: &str) -> (u16, u16, u16) {
    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw app frame");
    let buffer = terminal.backend().buffer();
    let sidebar = FrameLayoutPlan::for_app(app, TEST_FRAME_AREA)
        .operator_sidebar
        .expect("operator sidebar visible");

    for y in sidebar.y..sidebar.bottom() {
        let row = (sidebar.x..sidebar.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(column) = row.find(needle) {
            return (
                sidebar.x.saturating_add(
                    u16::try_from(row[..column].chars().count()).expect("column fits"),
                ),
                y,
                u16::try_from(needle.chars().count()).expect("needle width fits"),
            );
        }
    }

    panic!("missing rendered text: {needle}");
}

fn drag_transcript_selection_range(app: &mut AppState, start: (u16, u16), end: (u16, u16)) {
    let (start_column, start_row) = start;
    let (end_column, end_row) = end;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: start_column,
            row: start_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: end_column,
            row: end_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: end_column,
            row: end_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
}

fn drag_transcript_selection(app: &mut AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row, width) = transcript_selection_text_bounds(app, needle);
    drag_transcript_selection_range(app, (column, row), (column + width.saturating_sub(1), row));
    (column, row, width)
}

fn drag_operator_sidebar_selection(app: &mut AppState, needle: &str) -> (u16, u16, u16) {
    let (column, row, width) = operator_sidebar_text_bounds(app, needle);
    drag_transcript_selection_range(app, (column, row), (column + width.saturating_sub(1), row));
    (column, row, width)
}

fn run_started(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_run_started",
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".to_string(),
            workspace_root: "/tmp/workspace".to_string(),
        }),
    )
}

fn agent_spawned(seq: u64, agent_id: &str, profile: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )
}

fn child_agent_spawned(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent_agent_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        "req_agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: Some(parent_agent_id.to_string()),
        }),
    )
}

fn provider_started(seq: u64, request_id: &str, provider: &str, model: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.to_string(),
            provider_id: provider.to_string(),
            model_id: model.to_string(),
            prompt_summary: "prompt summary".to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

fn shell_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    args_summary: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "bash".to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}-args"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("bash".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn shell_finished(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.to_string(),
            status,
            output_summary: Some("shell output summary".to_string()),
            output_digest: Some(format!("digest-{tool_call_id}-output")),
            output_json: Some(output_json),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("bash".to_string()),
                timing: Some(ExecutionTimingMetadata {
                    elapsed_ms: Some(250),
                    ..ExecutionTimingMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn shell_test_events(
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            "req_shell_panel",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_shell_panel".to_string(),
                text: "Run a shell command".to_string(),
            }),
        ),
        provider_started(2, "req_shell_panel", "default", "model-1"),
        shell_requested(
            3,
            "req_shell_panel",
            "tc_shell_panel",
            r#"{"command":"cargo test -p harness-tui","description":"run TUI tests"}"#,
        ),
        envelope(
            4,
            "req_shell_panel",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_panel".to_string(),
            }),
        ),
        shell_finished(5, "req_shell_panel", "tc_shell_panel", status, output_json),
    ]
}

fn shell_run_test_events(
    status: ToolCallStatus,
    output_json: serde_json::Value,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            "req_shell_run_panel",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_shell_run_panel".to_string(),
                text: "Run shell.run".to_string(),
            }),
        ),
        provider_started(2, "req_shell_run_panel", "default", "model-1"),
        envelope(
            3,
            "req_shell_run_panel",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shell_run_panel".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"bash","args":["-lc","printf shell-run"],"cwd":"."}"#
                    .to_string(),
                args_digest: "digest-tc-shell-run-args".to_string(),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("shell.run".to_string()),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            4,
            "req_shell_run_panel",
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_run_panel".to_string(),
            }),
        ),
        envelope(
            5,
            "req_shell_run_panel",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shell_run_panel".to_string(),
                status,
                output_summary: Some("shell-run".to_string()),
                output_digest: Some("digest-tc-shell-run-output".to_string()),
                output_json: Some(output_json),
                metadata: Some(ToolCallMetadata {
                    canonical_tool_id: Some("shell.run".to_string()),
                    timing: Some(ExecutionTimingMetadata {
                        elapsed_ms: Some(42),
                        ..ExecutionTimingMetadata::default()
                    }),
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
    ]
}

fn child_link_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: Option<&str>,
    parent_session_id: Option<&str>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "agent.spawn".to_string(),
            args_summary: "{}".to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_session_id: parent_session_id.map(str::to_string),
                    child_session_id: child_session_id.map(str::to_string),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

fn child_task_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    child_session_id: &str,
    child_request_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        request_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some(tool_call_id.to_string()),
                    parent_request_id: Some(request_id.to_string()),
                    child_session_id: Some(child_session_id.to_string()),
                    child_request_id: Some(child_request_id.to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

#[test]
fn tool_call_entries_prefer_resolved_identity_and_lifecycle_contract() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_contract",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_contract".to_string(),
            text: "Check tool contract".to_string(),
        }),
    ));
    app.ingest_event(provider_started(2, "req_contract", "default", "model-1"));
    app.ingest_event(envelope(
        3,
        "req_contract",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_contract".to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"check tool contract","subagent_type":"researcher"}"#
                .to_string(),
            args_digest: "digest-contract".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("agent.spawn".to_string()),
                alias_source_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.invoked_tool_id(), "task");
    assert_eq!(tool_call.effective_tool_id(), "agent.spawn");
    assert_eq!(tool_call.resolved_canonical_tool_id(), Some("agent.spawn"));
    assert_eq!(tool_call.resolved_alias_source_tool_id(), Some("task"));
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Pending);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);

    app.ingest_event(envelope(
        4,
        "req_contract",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_contract".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_contract".to_string()),
            summary: "Need confirmation".to_string(),
            request_digest: "digest-perm-contract".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Pending);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::PendingPermission);

    app.ingest_event(envelope(
        5,
        "req_contract",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_contract".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Pending);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);

    app.ingest_event(envelope(
        6,
        "req_contract",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_contract".to_string(),
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(tool_call.lifecycle_state(), ToolCallLifecycleState::Running);
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Running);

    app.ingest_event(envelope(
        7,
        "req_contract",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_contract".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child completed".to_string()),
            output_digest: Some("digest-contract-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    let tool_call = &app.activities[0].tool_calls[0];
    assert_eq!(
        tool_call.lifecycle_state(),
        ToolCallLifecycleState::Completed
    );
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Succeeded);
}

#[test]
fn terminal_panel_is_hidden_by_default_and_toggles_from_keybinding() {
    let mut app = AppState::new_live(None, false, None);
    assert!(!app.terminal_panel_visible());
    assert!(
        crate::layout::FrameLayoutPlan::for_app(&app, TEST_FRAME_AREA)
            .terminal_panel
            .is_none()
    );

    app.handle_key(key(KeyCode::Char('4')));

    assert!(app.terminal_panel_visible());
    assert_eq!(
        app.focus,
        Focus::Prompt,
        "toggle should not steal composer focus"
    );
    assert!(
        crate::layout::FrameLayoutPlan::for_app(&app, TEST_FRAME_AREA)
            .terminal_panel
            .is_some()
    );

    app.handle_key(key(KeyCode::Char('4')));

    assert!(!app.terminal_panel_visible());
}

#[test]
fn terminal_panel_stays_hidden_for_live_bash_until_explicit_toggle() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "pwd",
            "status": 0,
            "success": true,
            "stdout": "/home/urbanbreach/code/accela/agent-harness\n",
            "stderr": "",
            "truncated": false
        }),
    ) {
        app.ingest_event(event);
    }

    assert!(!app.terminal_panel_visible());
    assert_eq!(
        app.focus,
        Focus::Prompt,
        "shell output should stay inline without stealing composer focus"
    );
    assert!(
        crate::layout::FrameLayoutPlan::for_app(&app, TEST_FRAME_AREA)
            .terminal_panel
            .is_none(),
        "live shell commands should not create a duplicate terminal panel above the composer"
    );

    app.handle_key(key(KeyCode::Char('4')));
    assert!(app.terminal_panel_visible());

    app.handle_key(key(KeyCode::Char('4')));
    assert!(!app.terminal_panel_visible());

    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "pwd",
            "status": 0,
            "success": true,
            "stdout": "/srv/samba/code/accela/agent-harness\n",
            "stderr": "",
            "truncated": false
        }),
    ) {
        let mut event = event;
        event.seq += 10;
        app.ingest_event(event);
    }

    assert!(
        !app.terminal_panel_visible(),
        "later shell commands should also remain inline unless the user toggles the panel"
    );
}

#[test]
fn terminal_panel_extracts_successful_bash_command_output() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "cargo test -p harness-tui",
            "workdir": ".",
            "status": 0,
            "success": true,
            "stdout": "ok\nall tests passed\n",
            "stderr": "",
            "truncated": false
        }),
    ) {
        app.ingest_event(event);
    }

    let entries = app.terminal_panel_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "cargo test -p harness-tui");
    assert_eq!(entries[0].stdout.as_deref(), Some("ok\nall tests passed\n"));
    assert_eq!(entries[0].stderr, None);
    assert_eq!(entries[0].exit_code, Some(0));
    assert_eq!(entries[0].duration_ms, Some(250));

    assert!(!app.terminal_panel_visible());
    app.handle_key(key(KeyCode::Char('4')));
    assert!(app.terminal_panel_visible());
    let debug = render_debug(&app, 140, 40);
    assert!(debug.contains("Terminal"));
    assert!(debug.contains("$ cargo test -p harness-tui"));
    assert!(debug.contains("stdout> ok"));
    assert!(debug.contains("exit 0"));
}

#[test]
fn terminal_panel_renders_failed_command_stderr_and_exit_status() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Failed,
        serde_json::json!({
            "command": "cargo test -p harness-tui",
            "status": 101,
            "success": false,
            "stdout": "",
            "stderr": "test failed\nassertion failed\n",
            "truncated": true,
            "output_artifact": {"path": "artifacts/toolcalls/tc_shell_panel/shell.output.txt"}
        }),
    ) {
        app.ingest_event(event);
    }
    assert!(!app.terminal_panel_visible());
    app.handle_key(key(KeyCode::Char('4')));
    assert!(app.terminal_panel_visible());

    let debug = render_debug(&app, 140, 40);
    assert!(debug.contains("failed"));
    assert!(debug.contains("exit 101"));
    assert!(debug.contains("stderr> test failed"));
    assert!(debug.contains("output truncated"));
}

#[test]
fn terminal_panel_extracts_shell_run_direct_command_schema() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_run_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "cmd": "bash",
            "args": ["-lc", "printf shell-run"],
            "cwd": ".",
            "status": 0,
            "success": true,
            "stdout": "shell-run",
            "stderr": "",
            "truncated": false
        }),
    ) {
        app.ingest_event(event);
    }

    assert!(!app.terminal_panel_visible());
    let entries = app.terminal_panel_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "bash -lc printf shell-run");
    assert_eq!(entries[0].cwd.as_deref(), Some("."));
    assert_eq!(entries[0].stdout.as_deref(), Some("shell-run"));
    assert_eq!(entries[0].duration_ms, Some(42));
}

#[test]
fn terminal_panel_replay_reconstructs_from_events_without_execution() {
    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/terminal-panel-replay"),
        shell_test_events(
            ToolCallStatus::Succeeded,
            serde_json::json!({
                "command": "printf replay",
                "status": 0,
                "success": true,
                "stdout": "replay\n",
                "stderr": "",
                "truncated": false
            }),
        ),
    );

    assert_eq!(replay.terminal_panel_entries().len(), 1);
    assert_eq!(replay.terminal_panel_entries()[0].command, "printf replay");
    replay.handle_key(key(KeyCode::Char('4')));

    let debug = render_debug(&replay, 140, 40);
    assert!(debug.contains("Replay · read-only"));
    assert!(debug.contains("$ printf replay"));
    assert!(debug.contains("stdout> replay"));
}

#[test]
fn terminal_panel_focus_scrolls_independently_from_transcript() {
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(key(KeyCode::Char('4')));
    app.focus = Focus::Terminal;
    app.last_terminal_panel_max_scroll.set(20);

    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.terminal_panel_scroll(), 10);
    assert!(!app.terminal_panel_follow());
    assert_eq!(app.transcript_scroll, 0);

    app.handle_key(key(KeyCode::End));
    assert_eq!(app.terminal_panel_scroll(), 0);
    assert!(app.terminal_panel_follow());
}

#[test]
fn overlay_stack_orders_details_palette_permission() {
    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[OverlayKind::DetailsDrawer, OverlayKind::CommandPalette]
    );

    app.ingest_event(envelope(
        1,
        "req_overlay_stack",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_overlay_stack".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_overlay_stack".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-overlay-stack".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    assert_eq!(
        app.overlay_stack().ordered(),
        &[OverlayKind::DetailsDrawer, OverlayKind::PermissionModal]
    );
}

#[test]
fn overlay_stack_orders_permission_above_commands_and_slash() {
    AppState::exact_test_overlay_stack_orders_permission_above_commands_and_slash();
}

#[test]
fn permission_modal_preempts_palette() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Char('d')));

    app.ingest_event(envelope(
        1,
        "req_overlay_preempt",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_overlay_preempt".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_overlay_preempt".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-overlay-preempt".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    ));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal)
    );
    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_overlay_preempt".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }]
    );
}

#[test]
fn permission_modal_ignores_unmapped_chars_without_buffering() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.prompt_buffer = "keep this draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_modal_quit",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_quit".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_quit".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-quit".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Char('q')));

    assert!(!app.should_quit);
    assert_eq!(app.prompt_buffer, "keep this draft");
    let intents = intents.lock().expect("lock intents");
    assert!(intents.is_empty());
    assert!(app.active_permission().is_some());
}

#[test]
fn permission_modal_escape_rejects_without_hiding_pending_permission() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_modal_escape",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_escape".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_escape".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-escape".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Esc));

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_modal_escape".to_string(),
            decision: PermissionDecision::Deny,
            reason: None,
            grant_scope: None,
        }]
    );
    assert!(app.active_permission().is_some());
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal)
    );
}

#[test]
fn question_permission_modal_collects_answers_and_emits_reason_payload() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_question_modal",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_modal".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_question_modal".to_string()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                }]
            })
            .to_string(),
            request_digest: "digest-question-modal".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_question_modal".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"]]".to_string()),
            grant_scope: None,
        }]
    );
}

#[test]
fn question_permission_modal_multi_question_uses_tabs_before_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_question_tabs",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_tabs".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_question_tabs".to_string()),
            summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}]
                    },
                    {
                        "question": "Pick another",
                        "header": "Second",
                        "options": [{"label": "B", "description": "Option B"}]
                    }
                ]
            })
            .to_string(),
            request_digest: "digest-question-tabs".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.question_prompt_tab("perm_question_tabs"), 1);
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.question_prompt_tab("perm_question_tabs"), 2);
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_question_tabs".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"],[\"B\"]]".to_string()),
            grant_scope: None,
        }]
    );
}

#[test]
fn permission_modal_allow_always_requests_durable_run_grant() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_modal_allow_always_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_allow_always_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_allow_always_1".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-allow-always".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Right));
    assert_eq!(
        app.permission_modal_selection("perm_modal_allow_always_1"),
        PermissionModalSelection::AllowAlways
    );

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.permission_modal_stage("perm_modal_allow_always_1"),
        PermissionModalStage::AlwaysConfirm
    );

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_modal_allow_always_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Run),
        }]
    );
}

#[test]
fn runtime_context_labels_distinguish_live_continue_and_replay() {
    let launch_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("deterministic"),
        "GPT-5.4 Mini · Deterministic",
    );

    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(LaunchMetadata::from_model_option(&launch_option));
    let startup_dock = startup.control_dock_view_model();
    assert_eq!(
        startup_dock.primary_summary,
        "Launch: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(startup_dock.summary_segment, None);
    assert_eq!(startup_dock.runtime_context.as_deref(), Some("default"));

    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(LaunchMetadata::from_model_option(&launch_option));
    let live_dock = live.control_dock_view_model();
    assert_eq!(
        live_dock.primary_summary,
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(live_dock.summary_segment, None);
    assert_eq!(live_dock.runtime_context.as_deref(), Some("default"));

    let mut continued = AppState::new_live(None, false, None);
    continued.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option).with_mode_label("Continued"),
    );
    let continued_dock = continued.control_dock_view_model();
    assert_eq!(
        continued_dock.primary_summary,
        "Continued runtime: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(continued_dock.summary_segment, None);
    assert_eq!(continued_dock.runtime_context.as_deref(), Some("default"));

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/runtime-context-replay"), Vec::new());
    replay.set_launch_metadata(LaunchMetadata::from_model_option(&launch_option));
    let replay_dock = replay.control_dock_view_model();
    assert_eq!(
        replay_dock.primary_summary,
        "Recorded runtime · read-only: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay_dock.summary_segment, None);
    assert_eq!(replay_dock.runtime_context.as_deref(), Some("default"));
    assert!(replay_dock.composer_disabled);
}

#[test]
fn composer_metadata_prefers_short_agent_name_and_configured_source_label() {
    let option = metadata_model_option(
        "build",
        Some("Deep Agent"),
        "default",
        Some("CLIProxyAPI"),
        "gpt-5.4-mini",
        "GPT-5.4 Mini · High",
    );

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_option(&option));

    assert_eq!(app.current_agent_label().as_deref(), Some("Build"));
    assert_eq!(
        app.current_source_label().as_deref(),
        Some("CLIProxyAPI (OpenAI)")
    );
}

#[test]
fn composer_metadata_deduplicates_provider_backend_source_label() {
    let openai_option = metadata_model_option(
        "build",
        Some("Deep Agent"),
        "openai",
        Some("OpenAI"),
        "gpt-5.4-mini",
        "GPT-5.4 Mini · High",
    );
    let configured_suffix_option = metadata_model_option(
        "build",
        Some("Deep Agent"),
        "default",
        Some("CLIProxyAPI (OpenAI)"),
        "gpt-5.4-mini",
        "GPT-5.4 Mini · High",
    );

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(LaunchMetadata::from_model_option(&openai_option));
    assert_eq!(app.current_source_label().as_deref(), Some("OpenAI"));

    app.set_launch_metadata(LaunchMetadata::from_model_option(&configured_suffix_option));
    assert_eq!(
        app.current_source_label().as_deref(),
        Some("CLIProxyAPI (OpenAI)")
    );
}

#[test]
fn live_switch_model_labels_next_turn_only() {
    let launch_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("deterministic"),
        "GPT-5.4 Mini · Deterministic",
    );
    let next_turn_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("creative"),
        "GPT-5.4 Mini · Creative",
    );

    let mut live = AppState::new_live(None, false, None);
    live.apply_keybindings(default_navigation_keybindings());
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option)
            .with_available_models(vec![launch_option.clone(), next_turn_option.clone()]),
    );

    live.handle_key(key(KeyCode::Tab));

    let dock = live.control_dock_view_model();
    assert_eq!(
        dock.primary_summary,
        "Context: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(
        dock.summary_segment,
        Some(view_model::ControlDockSummarySegment {
            kind: view_model::ControlDockSummarySegmentKind::Orchestration,
            text: "Next turns: deep · GPT-5.4 Mini".to_string(),
            tone: view_model::ControlDockSummaryTone::Secondary,
        })
    );

    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/runtime-context-replay-switch"),
        Vec::new(),
    );
    replay.apply_keybindings(default_navigation_keybindings());
    replay.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option)
            .with_available_models(vec![launch_option, next_turn_option]),
    );

    replay.handle_key(key(KeyCode::Tab));

    let replay_dock = replay.control_dock_view_model();
    assert_eq!(
        replay_dock.primary_summary,
        "Recorded runtime · read-only: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(replay_dock.summary_segment, None);
    assert_eq!(replay.current_model_label(), "GPT-5.4 Mini · Deterministic");
    assert_eq!(replay.active_profile(), "deep");
}

#[test]
fn tab_cycles_build_and_plan_primary_agents() {
    let build_option =
        runtime_context_model_option("build", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");
    let plan_option =
        runtime_context_model_option("plan", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, None);
    app.on_ui_intent = Some(sink);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&build_option)
            .with_available_models(vec![build_option.clone(), plan_option])
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.active_profile(), "plan");
    assert_eq!(app.current_agent_label().as_deref(), Some("Plan"));
    {
        let intents = intents.lock().expect("lock intents");
        let [UiIntent::SwitchModel {
            profile,
            launch_metadata,
        }] = intents.as_slice()
        else {
            panic!("expected one switch-model intent: {intents:?}");
        };
        assert_eq!(profile, "plan");
        assert_eq!(launch_metadata.profile(), "plan");
        assert_eq!(launch_metadata.switchable_profiles(), &["build", "plan"]);
    }

    app.handle_key(key(KeyCode::BackTab));

    assert_eq!(app.active_profile(), "build");
    let intents = intents.lock().expect("lock intents");
    let Some(UiIntent::SwitchModel {
        profile,
        launch_metadata,
    }) = intents.get(1)
    else {
        panic!("expected second switch-model intent: {intents:?}");
    };
    assert_eq!(profile, "build");
    assert_eq!(launch_metadata.profile(), "build");
}

#[test]
fn switching_agent_after_submit_keeps_existing_turn_footer_agent() {
    let build_option =
        runtime_context_model_option("build", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");
    let plan_option =
        runtime_context_model_option("plan", "default", "gpt-5.4-mini", None, "GPT-5.4 Mini");

    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_option(&build_option)
            .with_available_models(vec![build_option, plan_option])
            .with_switchable_profiles(vec!["build".to_string(), "plan".to_string()]),
    );

    for ch in "keep footer agent".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.active_profile(), "plan");
    let rendered = render_debug(&app, 100, 32);
    assert!(
        rendered.contains("Build · active"),
        "submitted turn footer should keep its original agent after switching\n{rendered}"
    );
    assert!(
        !rendered.contains("Plan · active"),
        "submitted turn footer must not follow the newly selected agent\n{rendered}"
    );
}

#[test]
fn focus_returns_after_palette_close() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.palette_visible);
    assert_eq!(app.focus, Focus::Details);
}

#[test]
fn details_drawer_toggles_without_stealing_transcript_state() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_a",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_a".to_string(),
            text: "First".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_a".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "First".to_string(),
            request_digest: "digest-a".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_b",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_b".to_string(),
            text: "Second".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_b",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_b".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Second".to_string(),
            request_digest: "digest-b".to_string(),
            metadata: None,
        }),
    ));

    app.follow_mode = false;
    app.focus = Focus::Details;
    app.selected_activity_index = 0;
    app.details_scroll = 7;

    app.handle_key(key(KeyCode::Char('i')));
    assert!(app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.follow_mode);
    assert_eq!(app.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);

    app.handle_key(key(KeyCode::Char('i')));
    assert!(!app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.follow_mode);
    assert_eq!(app.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);
}

#[test]
fn config_backed_live_launch_starts_in_session_shell_without_details_drawer() {
    set_pending_live_launch_metadata(LaunchMetadata::new(
        "deep",
        "default",
        Some("gpt-5.4-mini".to_string()),
    ));

    let app = AppState::new_live(None, false, None);

    assert!(!app.details_drawer_open());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.launch_mode_label(), None);
    assert_eq!(app.current_model_reasoning_label(), None);
}

#[test]
fn mouse_wheel_scrolls_transcript_without_stealing_focus() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert!(!app.follow_mode);
    assert_eq!(app.transcript_scroll, 3);
    assert_eq!(app.focus, Focus::Prompt);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert_eq!(app.transcript_scroll, 0);
    assert!(app.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn transcript_navigation_keys_match_scroll_expectations() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;
    app.last_transcript_max_scroll.set(42);

    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.transcript_scroll, 10);
    assert!(!app.follow_mode);

    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.transcript_scroll, 0);
    assert!(app.follow_mode);

    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.transcript_scroll, 42);
    assert!(!app.follow_mode);

    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.transcript_scroll, 32);
    assert!(!app.follow_mode);

    app.handle_key(key(KeyCode::End));
    assert_eq!(app.transcript_scroll, 0);
    assert!(app.follow_mode);
}

#[test]
fn mouse_wheel_scrolls_inspector_when_hovered() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    app.details_scroll = 2;
    app.transcript_scroll = 4;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Inspector),
        None,
        None,
    );
    assert_eq!(app.details_scroll, 5);
    assert_eq!(app.transcript_scroll, 4);
    assert_eq!(app.focus, Focus::List);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Inspector),
        None,
        None,
    );
    assert_eq!(app.details_scroll, 2);
    assert_eq!(app.transcript_scroll, 4);
    assert_eq!(app.focus, Focus::List);
}

#[test]
fn mouse_wheel_ignores_non_scrollable_areas() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.details_scroll = 6;
    app.transcript_scroll = 2;
    app.follow_mode = false;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 2,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.details_scroll, 6);
    assert_eq!(app.transcript_scroll, 2);
    assert!(!app.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn mouse_click_toggles_operator_sidebar_section_without_stealing_focus() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.details_scroll = 6;

    assert!(app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        Some(OperatorSidebarSection::ModifiedFiles),
        None,
    );

    assert!(!app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));
    assert_eq!(app.details_scroll, 0);
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn edit_applied_auto_opens_modified_files_section() {
    let mut app = AppState::new_live(None, false, None);
    assert!(app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));

    app.ingest_event(envelope(
        1,
        "req_edit_open",
        EventV1::EditApplied(EditAppliedEvent {
            edit_id: "edit-1".to_string(),
            path: "src/ui.rs".to_string(),
            new_file_digest: "digest-1".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));

    assert!(!app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));
}

#[test]
fn dragging_transcript_scrollbar_updates_scroll_position() {
    let mut app = AppState::new_live(None, false, None);
    app.last_transcript_max_scroll.set(100);
    app.follow_mode = false;
    app.transcript_scroll = 50;

    let scrollbar = TranscriptScrollbarHit {
        lane: Rect::new(72, 1, 2, 20),
        track: Rect::new(72, 2, 2, 18),
        thumb: Rect::new(72, 6, 2, 4),
        max_scroll: 100,
    };

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 72,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        Some(scrollbar),
    );
    assert!(app.transcript_scrollbar_dragging());

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 72,
            row: 14,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );

    assert!(!app.follow_mode);
    assert_eq!(app.transcript_scroll, 21);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 72,
            row: 17,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert!(!app.transcript_scrollbar_dragging());
}

#[test]
fn clicking_transcript_scrollbar_track_without_thumb_does_not_start_drag() {
    let mut app = AppState::new_live(None, false, None);
    app.last_transcript_max_scroll.set(80);

    let scrollbar = TranscriptScrollbarHit {
        lane: Rect::new(72, 1, 2, 20),
        track: Rect::new(72, 2, 2, 18),
        thumb: Rect::new(72, 6, 2, 4),
        max_scroll: 80,
    };

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 72,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        Some(scrollbar),
    );

    assert!(!app.transcript_scrollbar_dragging());
    assert!(app.follow_mode);
    assert_eq!(app.transcript_scroll, 0);
}

#[cfg(not(windows))]
#[test]
fn mouse_drag_copy_on_select_copies_transcript_text_and_clears_selection() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().expect("lock copied text") = Some(text.to_string());
        Ok(())
    })));

    let mut app = transcript_selection_test_app();
    drag_transcript_selection(&mut app, "Copy this exact reply");

    assert_eq!(
        copied.lock().expect("lock copied text").clone(),
        Some("Copy this exact reply".to_string())
    );
    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );

    crate::clipboard::set_copy_override(None);
}

#[cfg(not(windows))]
#[test]
fn mouse_drag_copy_on_select_copies_operator_sidebar_text() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().expect("lock copied text") = Some(text.to_string());
        Ok(())
    })));

    let mut app = operator_sidebar_selection_test_app();
    drag_operator_sidebar_selection(&mut app, "Copy sidebar task");

    assert_eq!(
        copied.lock().expect("lock copied text").clone(),
        Some("Copy sidebar task".to_string())
    );
    assert!(app.operator_sidebar_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );

    crate::clipboard::set_copy_override(None);
}

#[test]
fn disabled_copy_on_select_keeps_operator_sidebar_selection_until_right_click_copy() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().expect("lock copied text") = Some(text.to_string());
        Ok(())
    })));

    let mut app = operator_sidebar_selection_test_app();
    let (column, row, _) = drag_operator_sidebar_selection(&mut app, "Copy sidebar task");

    assert!(app.operator_sidebar_selection().is_some());
    assert!(copied.lock().expect("lock copied text").is_none());

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(
        copied.lock().expect("lock copied text").clone(),
        Some("Copy sidebar task".to_string())
    );
    assert!(app.operator_sidebar_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );
}

#[test]
fn mouse_drag_copy_on_select_surfaces_error_toast_when_copy_fails() {
    crate::clipboard::set_copy_override(Some(Box::new(|_| {
        Err(std::io::Error::other("simulated clipboard failure"))
    })));

    let mut app = transcript_selection_test_app();
    drag_transcript_selection(&mut app, "Copy this exact reply");

    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some((
            "clipboard copy failed: simulated clipboard failure",
            ToastVariant::Error,
        ))
    );

    crate::clipboard::set_copy_override(None);
}

#[test]
fn mouse_drag_copy_on_select_preserves_multiline_text_without_render_padding() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().expect("lock copied text") = Some(text.to_string());
        Ok(())
    })));

    let expected = [
        "Done.",
        "",
        "Changed:",
        "• docs/rust-language.md",
        "",
        "What I changed:",
        "• Tightened the opening description to mention reliable software and compile-time guarantees.",
    ]
    .join("\n");
    let mut app = transcript_selection_test_app_with_text(&expected);
    let start = transcript_selection_text_position(&app, "Done.");
    let (end_column, end_row, end_width) = transcript_selection_text_bounds(&app, "guarantees.");
    drag_transcript_selection_range(
        &mut app,
        start,
        (end_column + end_width.saturating_sub(1), end_row),
    );

    assert_eq!(
        copied.lock().expect("lock copied text").clone(),
        Some(expected)
    );
    assert!(app.transcript_selection().is_none());

    crate::clipboard::set_copy_override(None);
}

#[test]
fn disabled_copy_on_select_keeps_selection_until_right_click_copy() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().expect("lock copied text") = Some(text.to_string());
        Ok(())
    })));

    let mut app = transcript_selection_test_app();
    let (column, row, _) = drag_transcript_selection(&mut app, "Copy this exact reply");

    assert!(app.transcript_selection().is_some());
    assert!(copied.lock().expect("lock copied text").is_none());

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(
        copied.lock().expect("lock copied text").clone(),
        Some("Copy this exact reply".to_string())
    );
    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );
}

#[test]
fn disabled_copy_on_select_supports_ctrl_c_and_escape() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        sink.lock()
            .expect("lock copied text")
            .push(text.to_string());
        Ok(())
    })));

    let mut copy_app = transcript_selection_test_app();
    drag_transcript_selection(&mut copy_app, "Copy this exact reply");
    assert!(copy_app.transcript_selection().is_some());

    copy_app.set_frame_area(TEST_FRAME_AREA);
    copy_app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        copied.lock().expect("lock copied text").as_slice(),
        ["Copy this exact reply"]
    );
    assert!(copy_app.transcript_selection().is_none());

    let mut escape_app = transcript_selection_test_app();
    drag_transcript_selection(&mut escape_app, "Copy this exact reply");
    assert!(escape_app.transcript_selection().is_some());

    escape_app.handle_key(key(KeyCode::Esc));

    assert!(escape_app.transcript_selection().is_none());
    assert_eq!(
        copied.lock().expect("lock copied text").as_slice(),
        ["Copy this exact reply"]
    );
}

#[cfg(not(windows))]
#[test]
fn mouse_drag_copy_on_select_keeps_body_rows_aligned_after_reasoning_gap() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().expect("lock copied text") = Some(text.to_string());
        Ok(())
    })));

    let mut app = transcript_selection_test_app_with_reasoning(
        "Trace the exact rows first",
        "Copy this exact reply",
    );
    drag_transcript_selection(&mut app, "Copy this exact reply");

    assert_eq!(
        copied.lock().expect("lock copied text").clone(),
        Some("Copy this exact reply".to_string())
    );
    assert!(app.transcript_selection().is_none());

    crate::clipboard::set_copy_override(None);
}

#[test]
fn transcript_selection_hit_testing_reuses_cached_snapshot_during_drag() {
    let app = transcript_selection_test_app();
    let (column, row, width) = transcript_selection_text_bounds(&app, "Copy this exact reply");

    reset_transcript_selection_cache_metrics_for_test();

    for offset in 0..width {
        assert!(transcript_selection_cell(&app, TEST_FRAME_AREA, column + offset, row,).is_some());
    }

    assert_eq!(transcript_selection_cache_build_count_for_test(), 1);
}

#[test]
fn mouse_wheel_does_not_build_transcript_selection_snapshot() {
    let mut app = transcript_selection_test_app();

    reset_transcript_selection_cache_metrics_for_test();

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );

    assert_eq!(transcript_selection_cache_build_count_for_test(), 0);
}

#[test]
fn transcript_selection_render_reuses_cached_snapshot() {
    let mut app = transcript_selection_test_app();
    let (column, row, width) = transcript_selection_text_bounds(&app, "Copy this exact reply");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column + width.saturating_sub(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    reset_transcript_selection_cache_metrics_for_test();

    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw selection frame");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw selection frame again");

    assert_eq!(transcript_selection_cache_build_count_for_test(), 1);
}

#[test]
fn transcript_selection_render_stays_aligned_after_large_reasoning_block() {
    let thinking = (0..30)
        .map(|idx| format!("Reasoning line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = transcript_selection_test_app_with_reasoning(&thinking, "Target answer line");
    let (column, row, width) = transcript_selection_text_bounds(&app, "Target answer line");

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column + width.saturating_sub(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw selection frame");

    let buffer = terminal.backend().buffer();
    let highlight = crate::theme::Theme::default().status.info;
    assert_eq!(buffer[(column, row)].bg, highlight);

    let far_above_row = row.saturating_sub(20);
    if far_above_row != row {
        assert_ne!(buffer[(column, far_above_row)].bg, highlight);
    }
}

#[test]
fn transcript_render_key_is_cached_across_selection_drag_path() {
    let mut app = transcript_selection_test_app();

    AppState::reset_transcript_render_key_metrics_for_test();

    let (column, row, width) = transcript_selection_text_bounds(&app, "Copy this exact reply");

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column + width.saturating_sub(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw selection frame");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw selection frame again");

    assert_eq!(AppState::transcript_render_key_build_count_for_test(), 1);
}

#[test]
fn historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = intents.clone();
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(envelope(
        1,
        "req_resume_1",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_resume_1".to_string(),
            text: "previous question".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_resume_1",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_resume_1".to_string(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "previous question".to_string(),
            request_digest: "digest-resume-1".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_resume_1",
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: "req_resume_1".to_string(),
            delta: "previous answer".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_resume_1",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000123".to_string(),
            result_summary: "previous answer".to_string(),
            result_digest: "digest-task-123".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);

    for c in "next".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert!(
        intents
            .iter()
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text, .. } if text == "next")),
        "historical streaming residue should not block first resumed submit"
    );
}

#[test]
fn historical_terminal_events_stay_in_session_shell_after_live_finish() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume")),
        true,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        "req_resume_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "previous run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert!(!app.completed_session_shell_active());
    assert!(!app.should_quit);
    assert_eq!(app.events.len(), 1);

    app.ingest_event(envelope(
        2,
        "req_live_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "live run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(app.should_quit);
}

#[test]
fn continued_quiescent_bootstrap_stays_in_session_shell_without_handoff() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Continued"),
    );
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
        false,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        "req_resume_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "previous run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Prompt);
    assert!(!app.composer_disabled());
}

#[test]
fn startup_prompt_enter_echoes_prompt_and_selects_new_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));

    for c in "ship it".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit, "startup submit should leave the launcher");
    assert!(!app.startup_shell_visible());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.prompt_buffer, "");
    assert_eq!(app.prompt_history, vec!["ship it".to_string()]);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    let next_live = AppState::new_live(None, false, None);
    assert!(
        matches!(
            intents.lock().expect("lock intents").as_slice(),
            [UiIntent::NewSession]
        ),
        "startup submit should select a fresh session after the local prompt echo"
    );
    assert_eq!(next_live.prompt_history, vec!["ship it".to_string()]);
    assert_eq!(
        next_live
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
}

#[test]
fn slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    for ch in "/new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.startup_shell_visible());

    app.clear_prompt_input();
    for ch in "fresh run".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert!(!app.startup_shell_visible());
    assert!(
        matches!(
            intents.lock().expect("lock intents").as_slice(),
            [UiIntent::NewSession]
        ),
        "/new startup handoff must select a fresh session, not submit to the old live run"
    );

    let relaunched = AppState::new_live(None, false, None);
    assert_eq!(relaunched.prompt_buffer, "");
    assert_eq!(relaunched.prompt_history, vec!["fresh run".to_string()]);
    assert_eq!(
        relaunched
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("fresh run")
    );
}

#[test]
fn provider_reasoning_delta_populates_thinking_stream_without_overwriting_answer_text() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(provider_started(
        1,
        "req_reasoning",
        "default",
        "gpt-4o-mini",
    ));
    app.ingest_event(envelope(
        2,
        "req_reasoning",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_reasoning".to_string(),
            delta: "Drafting a careful answer.".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_reasoning",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_reasoning".to_string(),
            delta: "Hello world".to_string(),
        }),
    ));

    let activity = app.activities.back().expect("streaming activity");
    assert_eq!(activity.thinking_text, "Drafting a careful answer.");
    assert_eq!(activity.transcript_text, "Hello world");
}

#[test]
fn provider_request_finished_keeps_activity_streaming_until_turn_task_completes() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_turn_task",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_turn_task".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_turn_task",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "provider_req_turn_task".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Investigate the harness".to_string(),
            request_digest: "digest-turn-task".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_turn_task",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "provider_req_turn_task".to_string(),
            delta: "Looking into the turn loop".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_turn_task",
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: "provider_req_turn_task".to_string(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-turn-task-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(app.active_turn_in_progress());

    app.ingest_event(envelope(
        5,
        "req_turn_task",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_turn_task".to_string(),
            result_summary: "Final answer".to_string(),
            result_digest: "digest-turn-task-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.back().expect("completed activity exists");
    assert_eq!(activity.status, ActivityStatus::Done);
    assert!(!app.active_turn_in_progress());
}

#[test]
fn task_cancelled_marks_matching_activity_as_error() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_cancelled_turn",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_cancelled_turn".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_cancelled_turn",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_cancelled_turn".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Edit the docs".to_string(),
            request_digest: "digest-cancelled-turn".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_cancelled_turn",
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req_cancelled_turn".to_string(),
            delta: "Still thinking".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_cancelled_turn",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_cancelled_turn".to_string(),
            reason: "agent turn exceeded profile max_iters=24".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));

    let activity = app.activities.back().expect("cancelled activity exists");
    assert_eq!(activity.status, ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_deref(),
        Some("agent turn exceeded profile max_iters=24")
    );
    assert!(!app.active_turn_in_progress());
}

#[test]
fn child_tool_task_completed_does_not_finish_parent_turn_activity() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_child_task_completed",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_parent_turn".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_child_task_completed",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_task_completed".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Inspect a file".to_string(),
            request_digest: "digest-child-task-completed".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_child_task_completed",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_tool".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("tool:read".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_child_task_completed",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_tool".to_string(),
            result_summary: "24 lines read".to_string(),
            result_digest: "digest-child-tool-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_read".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.transcript_text.is_empty());
    assert!(app.active_turn_in_progress());
}

#[test]
fn child_tool_task_cancelled_does_not_mark_parent_turn_activity_error() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_child_task_cancelled",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_parent_turn".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_child_task_cancelled",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child_task_cancelled".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Inspect a file".to_string(),
            request_digest: "digest-child-task-cancelled".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_child_task_cancelled",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_tool".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("tool:read".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_child_task_cancelled",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_child_tool".to_string(),
            reason: "tool request timed out".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.error_message.is_none());
    assert!(app.active_turn_in_progress());
}

#[test]
fn terminal_only_turn_completion_scope_marks_activity_done_without_task_row() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_terminal_only_done",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_terminal_only_done".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Explain the fix".to_string(),
            request_digest: "digest-terminal-only-done".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_terminal_only_done",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_terminal_only_done".to_string(),
            result_summary: "Final answer".to_string(),
            result_digest: "digest-terminal-only-result".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Done);
    assert_eq!(activity.transcript_text, "Final answer");
    assert!(!app.active_turn_in_progress());
}

#[test]
fn terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_terminal_only_cancel",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_terminal_only_cancel".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Explain the fix".to_string(),
            request_digest: "digest-terminal-only-cancel".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_terminal_only_cancel",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_terminal_only_cancel".to_string(),
            reason: "agent turn exceeded profile max_iters=24".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_deref(),
        Some("agent turn exceeded profile max_iters=24")
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Cancelled);
    assert!(!app.active_turn_in_progress());
}

#[test]
fn terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_terminal_only_tool_cancel",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_terminal_only_tool_cancel".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "Read the file".to_string(),
            request_digest: "digest-terminal-only-tool-cancel".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_terminal_only_tool_cancel",
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "task_terminal_only_tool_cancel".to_string(),
            reason: "tool request timed out".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
        }),
    ));

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.error_message.is_none());
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Sending);
}

#[test]
fn replay_terminal_only_turn_completion_scope_marks_activity_done_without_task_row() {
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-terminal-only-done"),
        vec![
            envelope(
                1,
                "req_replay_terminal_only_done",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_replay_terminal_only_done".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Explain the fix".to_string(),
                    request_digest: "digest-replay-terminal-only-done".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                2,
                "req_replay_terminal_only_done",
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_replay_terminal_only_done".to_string(),
                    result_summary: "Final answer".to_string(),
                    result_digest: "digest-replay-terminal-only-result".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
        ],
    );

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Done);
    assert_eq!(activity.transcript_text, "Final answer");
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
}

#[test]
fn replay_terminal_only_turn_cancellation_scope_marks_activity_error_without_task_row() {
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-terminal-only-cancel"),
        vec![
            envelope(
                1,
                "req_replay_terminal_only_cancel",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_replay_terminal_only_cancel".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Explain the fix".to_string(),
                    request_digest: "digest-replay-terminal-only-cancel".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                2,
                "req_replay_terminal_only_cancel",
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_replay_terminal_only_cancel".to_string(),
                    reason: "agent turn exceeded profile max_iters=24".to_string(),
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                }),
            ),
        ],
    );

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_deref(),
        Some("agent turn exceeded profile max_iters=24")
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Cancelled);
}

#[test]
fn replay_terminal_only_tool_cancellation_scope_does_not_fail_activity_or_runtime_state() {
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-terminal-only-tool-cancel"),
        vec![
            envelope(
                1,
                "req_replay_terminal_only_tool_cancel",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_replay_terminal_only_tool_cancel".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Read the file".to_string(),
                    request_digest: "digest-replay-terminal-only-tool-cancel".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                2,
                "req_replay_terminal_only_tool_cancel",
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_replay_terminal_only_tool_cancel".to_string(),
                    reason: "tool request timed out".to_string(),
                    task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                }),
            ),
        ],
    );

    let activity = app.activities.back().expect("activity exists");
    assert_eq!(activity.status, ActivityStatus::Streaming);
    assert!(activity.error_message.is_none());
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Sending);
}

#[test]
fn ctrl_j_inserts_newline_without_submitting() {
    let mut app = AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    ));
    for c in "world".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "hello\nworld");
    assert_eq!(app.prompt_history.len(), 0);
}

#[test]
fn paste_multiline_text_inserts_newlines_without_submitting() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));

    app.handle_paste("alpha\r\n\r\nbeta\rgamma");

    assert_eq!(app.prompt_buffer, "alpha\n\nbeta\ngamma");
    assert_eq!(app.prompt_cursor, app.prompt_buffer.chars().count());
    assert!(app.prompt_history.is_empty());
    assert!(intents.lock().expect("lock intents").is_empty());
}

#[test]
fn multiline_history_keys_move_cursor_before_recalling_history() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_history = vec!["older prompt".to_string()];
    app.prompt_buffer = "alpha\nbeta".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.prompt_buffer, "alpha\nbeta");
    assert_eq!(app.prompt_cursor, 4);
    assert_eq!(app.prompt_history_index, None);

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.prompt_cursor, 4);
    assert_eq!(app.prompt_history_index, None);

    app.prompt_cursor = 0;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.prompt_buffer, "older prompt");
    assert_eq!(app.prompt_history_index, Some(0));

    app.handle_key(key(KeyCode::Down));
    assert!(app.prompt_buffer.is_empty());
    assert_eq!(app.prompt_history_index, None);
}

#[test]
fn typing_at_opens_file_mention_menu_with_directories() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/lib.rs"), "lib").expect("write lib");
    std::fs::write(tempdir.path().join("README.md"), "readme").expect("write readme");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;

    app.handle_key(key(KeyCode::Char('@')));

    assert!(app.file_mention_overlay_should_render());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::FileMentions));
    assert_eq!(app.file_mention_entries[0].display, "src/");
}

#[test]
fn file_mention_tab_expands_directory_without_closing_menu() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tempdir.path().join("src/bin")).expect("create nested dir");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('@')));

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.prompt_buffer, "@src/");
    assert!(app.file_mention_overlay_should_render());
    assert!(app
        .file_mention_entries
        .iter()
        .any(|entry| entry.display == "src/main.rs"));
}

#[test]
fn file_mention_enter_inserts_selected_file_with_space() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.prompt_buffer, "@src/main.rs ");
    assert_eq!(app.file_mention_tags.len(), 1);
    assert_eq!(app.file_mention_tags[0].start, 0);
    assert_eq!(app.file_mention_tags[0].end, "@src/main.rs".chars().count());
    let selected = app.selected_file_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].path, "src/main.rs");
    assert_eq!(selected[0].filename, "src/main.rs");
    assert_eq!(selected[0].mime, "text/plain");
    assert!(selected[0].url.ends_with("/src/main.rs"));
    assert!(!app.file_mention_overlay_should_render());
}

#[test]
fn file_mentions_use_injected_scanner_workspace_and_clock() {
    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_collaborators_for_test(
        PathBuf::from("/virtual/workspace"),
        vec!["docs/main.rs".to_string(), "src/main.rs".to_string()],
        123,
    );
    app.focus = Focus::Prompt;

    for ch in "@src/main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    let selected = app.selected_file_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].path, "src/main.rs");
    assert_eq!(selected[0].url, "file:///virtual/workspace/src/main.rs");
    assert_eq!(
        app.file_mention_frecency_for_test("src/main.rs"),
        Some((1, 123))
    );

    app.clear_prompt_input();
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.file_mention_entries[0].display, "src/main.rs");
}

#[test]
fn submitting_selected_file_mention_emits_structured_file_part() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| intents.lock().expect("lock intents").push(intent))
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    let [UiIntent::SubmitPrompt {
        text,
        selected_file_tags,
        ..
    }] = intents.as_slice()
    else {
        panic!("expected one submit prompt intent: {intents:?}");
    };
    assert_eq!(text, "@src/main.rs ");
    assert_eq!(selected_file_tags.len(), 1);
    assert_eq!(selected_file_tags[0].path, "src/main.rs");
    assert_eq!(selected_file_tags[0].source.value, "@src/main.rs");
}

#[test]
fn file_mention_picker_selects_agent_parts_from_launch_metadata() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-1").with_available_models(vec![
            ModelOption::from_model_ref("build", "mock:model-1"),
            ModelOption::from_model_ref("plan", "mock:model-1"),
            ModelOption::from_model_ref(
                harness_core::session_title::TITLE_AGENT_NAME,
                "mock:model-1",
            ),
        ]),
    );
    app.focus = Focus::Prompt;

    for ch in "@pla".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.prompt_buffer, "@plan ");
    assert!(app.selected_file_tags().is_empty());
    assert!(app.selected_resource_tags().is_empty());
    let selected = app.selected_agent_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "plan");
    assert_eq!(selected[0].source.value, "@plan");
}

#[test]
fn file_mention_picker_selects_mcp_resource_parts_from_launch_metadata() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-1").with_mcp_resources(vec![
            McpResourceOption {
                name: "Docs Guide".to_string(),
                uri: "mcp://docs/guide".to_string(),
                mime: "text/markdown".to_string(),
                description: Some("Documentation index".to_string()),
            },
        ]),
    );
    app.focus = Focus::Prompt;

    for ch in "@guide".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.prompt_buffer, "@mcp://docs/guide ");
    assert!(app.selected_file_tags().is_empty());
    assert!(app.selected_agent_tags().is_empty());
    let selected = app.selected_resource_tags();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "Docs Guide");
    assert_eq!(selected[0].uri, "mcp://docs/guide");
    assert_eq!(selected[0].mime, "text/markdown");
    assert_eq!(
        selected[0].description.as_deref(),
        Some("Documentation index")
    );
    assert_eq!(selected[0].source.value, "@mcp://docs/guide");
}

#[test]
fn file_mention_tag_is_removed_when_user_edits_inside_it() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;
    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    app.prompt_cursor = 2;
    app.handle_key(key(KeyCode::Char('x')));

    assert!(app.file_mention_tags.is_empty());
}

#[test]
fn live_bootstrap_auto_submit_echoes_and_emits_first_prompt() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new();
    app.focus = Focus::Prompt;
    app.on_ui_intent = Some(sink);

    app.apply_pending_live_prompt(PendingLivePrompt {
        text: "boot prompt".to_string(),
        auto_submit: true,
    });

    assert!(app.prompt_buffer.is_empty());
    assert_eq!(app.prompt_history, vec!["boot prompt".to_string()]);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("boot prompt")
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "boot prompt".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: LaunchMetadata::default(),
        }]
    );
}

#[test]
fn submit_prompt_while_turn_streams_echoes_as_queued_and_emits_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".to_string(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.prompt_buffer = "next prompt".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.submit_prompt();

    assert!(app.prompt_buffer.is_empty());
    assert_eq!(app.activities.len(), 2);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("next prompt")
    );
    assert_eq!(
        app.activities.back().map(|activity| activity.status),
        Some(ActivityStatus::Queued)
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "next prompt".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: LaunchMetadata::default(),
        }]
    );
}

#[test]
fn queued_turn_schedule_keeps_activity_queued_until_provider_starts() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".to_string(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_queued",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_queued".to_string(),
            text: "queued".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_queued",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_queued".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));

    let queued = app
        .activities
        .iter()
        .find(|activity| activity.request_id == "req_queued")
        .expect("queued activity");
    assert_eq!(queued.status, ActivityStatus::Queued);
    assert!(app.active_turn_in_progress());

    app.ingest_event(envelope(
        5,
        "req_queued",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_queued".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "queued".to_string(),
            request_digest: "digest-queued".to_string(),
            metadata: None,
        }),
    ));

    let queued = app
        .activities
        .iter()
        .find(|activity| activity.request_id == "req_queued")
        .expect("queued activity");
    assert_eq!(queued.status, ActivityStatus::Streaming);
}

#[test]
fn parent_transcript_hides_child_prompt_before_task_tool_finishes() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(agent_spawned(1, "parent", "build"));
    app.ingest_event(envelope(
        2,
        "req_parent",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_parent".to_string(),
            text: "Start parent work".to_string(),
        }),
    ));
    app.ingest_event(provider_started(3, "req_parent", "default", "model-parent"));
    app.ingest_event(envelope(
        4,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_child_pending".to_string(),
            tool_id: "task".to_string(),
            args_summary: r#"{"description":"inspect child","subagent_type":"explore"}"#
                .to_string(),
            args_digest: "digest-child-pending".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                ..ToolCallMetadata::default()
            }),
        }),
    ));
    app.ingest_event(child_agent_spawned(5, "agent_child", "explore", "parent"));
    let mut child_prompt = envelope_with_actor(
        6,
        "req_child",
        EventActor::new(ActorKind::Supervisor, None),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_child".to_string(),
            text: "Inspect child prompt that belongs to the subagent".to_string(),
        }),
    );
    child_prompt.stream_key = Some("agent:agent_child".to_string());
    app.ingest_event(child_prompt);

    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "parent transcript should hide the child prompt immediately after submission: {parent_debug}"
    );

    app.ingest_event(envelope_with_actor(
        7,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_child_turn".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:model-child".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        8,
        "req_child",
        EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_child".to_string(),
            provider_id: "default".to_string(),
            model_id: "model-child".to_string(),
            prompt_summary: "Inspect child prompt that belongs to the subagent".to_string(),
            request_digest: "digest-child-prompt".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        9,
        "req_parent",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_child_pending".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("child task scheduled".to_string()),
            output_digest: Some("digest-child-output".to_string()),
            output_json: Some(serde_json::json!({
                "child_session_id": "agent_child",
                "child_request_id": "req_child",
            })),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_child_pending".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_child".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    ));

    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    let parent_debug = render_debug(&app, 140, 40);
    assert!(
        !parent_debug.contains("Inspect child prompt that belongs to the subagent"),
        "parent transcript should hide child prompts before the task tool finishes: {parent_debug}"
    );

    let child_dir = tempfile::tempdir().expect("create child dir");
    let child_path = child_dir.path().join("agent_child");
    fs::create_dir_all(&child_path).expect("create child path");
    app.session_path = Some(child_path);
    let child_debug = render_debug(&app, 140, 40);
    assert!(
        child_debug.contains("Inspect child prompt that belongs to the subagent"),
        "the inline child session should still render its own prompt: {child_debug}"
    );
}

#[test]
fn replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());

    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Details);

    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.prompt_buffer.is_empty());
}

#[test]
fn child_session_navigation_keybinds_follow_default_contract() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let parent_dir = run_dir.path().join("parent");
    let child_a_dir = run_dir.path().join("child_a");
    let child_b_dir = run_dir.path().join("child_b");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_link_requested(4, "req_parent", "tc_child_a", Some("child_a"), None),
        child_link_requested(5, "req_parent", "tc_child_b", Some("child_b"), None),
    ];
    let child_a_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
        child_link_requested(4, "req_child_a", "tc_parent_a", None, Some("child_a")),
    ];
    let child_b_events = vec![
        run_started(1),
        agent_spawned(2, "child_b", "worker-b"),
        provider_started(3, "req_child_b", "mock", "model-child-b"),
        child_link_requested(4, "req_child_b", "tc_parent_b", None, Some("child_b")),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_a_dir, &child_a_events);
    write_events_jsonl(&child_b_dir, &child_b_events);
    for child_dir in [&child_a_dir, &child_b_dir] {
        fs::write(
            child_dir.join("meta.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "run_id": child_dir.file_name().and_then(|name| name.to_str()).unwrap(),
                "run_name": "child",
                "workspace_root": run_dir.path().display().to_string(),
                "config_digest": "digest",
                "harness_version": "test",
                "harness_lineage": {
                    "relationship": "task_child_session",
                    "parent_run_id": "parent",
                    "parent_session_id": "parent",
                }
            }))
            .expect("serialize child meta"),
        )
        .expect("write child meta");
    }

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut parent_app =
        AppState::new_live(Some(parent_dir.clone()), false, Some(Arc::clone(&sink)));
    parent_app.apply_keybindings(default_navigation_keybindings());
    for event in parent_events.clone() {
        parent_app.ingest_event(event);
    }
    parent_app.focus = Focus::Prompt;
    parent_app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));
    assert!(parent_app.prompt_buffer.is_empty());
    assert_eq!(
        parent_app.session_path.as_deref(),
        Some(child_a_dir.as_path())
    );
    assert!(parent_app.replay_mode);
    parent_app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        parent_app.session_path.as_deref(),
        Some(parent_dir.as_path())
    );
    assert!(!parent_app.replay_mode);

    let mut child_app =
        AppState::new_live(Some(child_a_dir.clone()), false, Some(Arc::clone(&sink)));
    child_app.apply_keybindings(default_navigation_keybindings());
    for event in child_a_events {
        child_app.ingest_event(event);
    }
    child_app.focus = Focus::Prompt;
    child_app.handle_key(key(KeyCode::Char(']')));
    child_app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));
    assert!(child_app.prompt_buffer.is_empty());

    let mut reverse_app = AppState::new_live(Some(child_b_dir.clone()), false, Some(sink));
    reverse_app.apply_keybindings(default_navigation_keybindings());
    for event in child_b_events {
        reverse_app.ingest_event(event);
    }
    reverse_app.focus = Focus::Prompt;
    reverse_app.handle_key(key(KeyCode::Char('[')));
    assert!(reverse_app.prompt_buffer.is_empty());

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::ReplaySession {
                run_id: "child_b".to_string(),
                run_dir: child_b_dir.clone(),
            },
            UiIntent::ReplaySession {
                run_id: "parent".to_string(),
                run_dir: parent_dir.clone(),
            },
            UiIntent::ReplaySession {
                run_id: "child_a".to_string(),
                run_dir: child_a_dir,
            },
        ]
    );
}

#[test]
fn replay_child_navigation_does_not_emit_live_intents() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let parent_dir = run_dir.path().join("parent");
    let child_a_dir = run_dir.path().join("child_a");
    let child_b_dir = run_dir.path().join("child_b");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_link_requested(4, "req_parent", "tc_child_a", Some("child_a"), None),
        child_link_requested(5, "req_parent", "tc_child_b", Some("child_b"), None),
    ];
    let child_a_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
        child_link_requested(4, "req_child_a", "tc_parent_a", None, Some("parent")),
    ];
    let child_b_events = vec![
        run_started(1),
        agent_spawned(2, "child_b", "worker-b"),
        provider_started(3, "req_child_b", "mock", "model-child-b"),
        child_link_requested(4, "req_child_b", "tc_parent_b", None, Some("parent")),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_a_dir, &child_a_events);
    write_events_jsonl(&child_b_dir, &child_b_events);

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_replay(parent_dir.clone(), parent_events);
    app.on_ui_intent = Some(sink);
    app.apply_keybindings(default_navigation_keybindings());
    app.set_launch_metadata(LaunchMetadata::new(
        "planner",
        "mock",
        Some("model-parent".to_string()),
    ));
    app.focus = Focus::Prompt;

    app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.session_path.as_deref(), Some(child_a_dir.as_path()));
    assert_eq!(app.active_profile(), "worker-a");
    assert!(app.replay_mode);
    assert!(app.prompt_buffer.is_empty());

    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(app.session_path.as_deref(), Some(child_b_dir.as_path()));
    assert_eq!(app.active_profile(), "worker-b");

    app.handle_key(key(KeyCode::Char('[')));
    assert_eq!(app.session_path.as_deref(), Some(child_a_dir.as_path()));
    assert_eq!(app.active_profile(), "worker-a");

    app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert_eq!(app.active_profile(), "planner");
    assert!(intents.lock().expect("lock intents").is_empty());
}

#[test]
fn replay_handoff_parent_navigation_continues_resumable_parent_session() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let parent_dir = run_dir.path().join("parent");
    let child_dir = run_dir.path().join("child_a");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_1", "mock", "model-parent"),
        envelope(
            4,
            "req_1",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "ready to continue".to_string(),
            }),
        ),
    ];
    let child_events = vec![
        run_started(1),
        agent_spawned(2, "child_a", "worker-a"),
        provider_started(3, "req_child_a", "mock", "model-child-a"),
    ];

    write_events_jsonl(&parent_dir, &parent_events);
    write_events_jsonl(&child_dir, &child_events);
    fs::write(
        child_dir.join("meta.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "run_id": "child_a",
            "run_name": "child",
            "workspace_root": run_dir.path().display().to_string(),
            "config_digest": "digest",
            "harness_version": "test",
            "harness_lineage": {
                "relationship": "task_child_session",
                "parent_run_id": "parent",
                "parent_session_id": "parent",
            }
        }))
        .expect("serialize child meta"),
    )
    .expect("write child meta");

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_replay(child_dir.clone(), child_events);
    app.enable_replay_navigation_handoff(Arc::clone(&sink));
    app.apply_keybindings(default_navigation_keybindings());

    app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));

    assert!(app.should_quit);
    assert_eq!(app.session_path.as_deref(), Some(child_dir.as_path()));
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::ContinueSession {
            run_id: "parent".to_string(),
            run_dir: parent_dir,
        }]
    );
}

#[test]
fn task_child_navigation_opens_inline_subagent_view_without_child_run_dir() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).expect("create parent dir");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "agent_child", "req_child"),
        agent_spawned(5, "agent_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".to_string(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        envelope(
            8,
            "req_child",
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_child".to_string(),
                delta: "child subagent transcript is visible only in child view".to_string(),
            }),
        ),
        envelope(
            9,
            "req_child",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_child".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-child-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            10,
            "req_parent",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_parent".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-parent-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ];

    let mut app = AppState::new_replay(parent_dir.clone(), parent_events);
    app.apply_keybindings(default_navigation_keybindings());
    assert!(!render_debug(&app, 140, 40)
        .contains("child subagent transcript is visible only in child view"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        app.session_path.as_deref(),
        Some(run_dir.path().join("agent_child").as_path())
    );
    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    assert!(render_debug(&app, 140, 40)
        .contains("child subagent transcript is visible only in child view"));
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.session_path == parent_dir && snapshot.replay_mode));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('['),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert!(app.replay_mode);
    assert!(!render_debug(&app, 140, 40)
        .contains("child subagent transcript is visible only in child view"));
}

#[test]
fn parent_child_navigation_ignores_nested_subagents_hidden_from_parent_transcript() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).expect("create parent dir");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "z_child", "req_child"),
        agent_spawned(5, "z_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".to_string(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        child_task_requested(
            8,
            "req_child",
            "tc_grandchild",
            "a_grandchild",
            "req_grandchild",
        ),
        agent_spawned(9, "a_grandchild", "explore"),
    ];

    let mut app = AppState::new_replay(parent_dir, parent_events);
    app.apply_keybindings(default_navigation_keybindings());

    app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        app.session_path.as_deref(),
        Some(run_dir.path().join("z_child").as_path()),
        "parent ctrl+] should open the direct child, not a nested grandchild"
    );
    assert!(app
        .activities
        .iter()
        .any(|activity| activity.request_id == "req_child"));
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.child_session_ids == vec!["z_child".to_string()]));
}

#[test]
fn live_inline_child_navigation_restores_live_parent_mode() {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    let parent_dir = run_dir.path().join("parent");
    fs::create_dir_all(&parent_dir).expect("create parent dir");

    let parent_events = vec![
        run_started(1),
        agent_spawned(2, "parent", "planner"),
        provider_started(3, "req_parent", "mock", "model-parent"),
        child_task_requested(4, "req_parent", "tc_child", "agent_child", "req_child"),
        agent_spawned(5, "agent_child", "explore"),
        envelope(
            6,
            "req_child",
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_child".to_string(),
                text: "Inspect child".to_string(),
            }),
        ),
        provider_started(7, "req_child", "mock", "model-child"),
        envelope(
            8,
            "req_child",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_child".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-child-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            "req_parent",
            EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_parent".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-parent-finished".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ];

    let mut app = AppState::new_live(Some(parent_dir.clone()), false, None);
    app.apply_keybindings(default_navigation_keybindings());
    for event in parent_events {
        app.ingest_event(event);
    }

    app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        app.session_path.as_deref(),
        Some(run_dir.path().join("agent_child").as_path())
    );
    assert!(app.replay_mode);
    assert!(app
        .session_navigation_stack
        .last()
        .is_some_and(|snapshot| snapshot.session_path == parent_dir && !snapshot.replay_mode));

    app.handle_key(key(KeyCode::Esc));

    assert_eq!(app.session_path.as_deref(), Some(parent_dir.as_path()));
    assert!(!app.replay_mode);
    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('x')));
    assert_eq!(app.prompt_buffer, "x");
}

#[test]
fn slash_menu_closes_after_whitespace() {
    let mut app = AppState::new_startup(Vec::new(), None);

    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(app.slash_visible);

    app.handle_key(key(KeyCode::Char(' ')));

    assert!(!app.slash_visible);
    assert_eq!(app.prompt_buffer, "/n ");
}

#[test]
fn slash_menu_resets_selection_when_filter_changes() {
    let mut app = AppState::new_startup(Vec::new(), None);

    app.handle_key(key(KeyCode::Char('/')));
    app.slash_selected = 2;
    assert_eq!(app.slash_selected, 2);

    app.handle_key(key(KeyCode::Char('r')));
    app.handle_key(key(KeyCode::Char('e')));
    app.handle_key(key(KeyCode::Char('p')));

    assert_eq!(app.slash_filtered, vec!["replay".to_string()]);
    assert_eq!(app.slash_selected, 0);
}

#[test]
fn slash_menu_matches_descriptions_and_boosts_prefixes() {
    let mut app = AppState::new_startup(Vec::new(), None);

    for ch in "/saved".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(&app.slash_filtered[..2], ["replay", "resume"]);

    app.clear_prompt_input();
    for ch in "/re".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(&app.slash_filtered[..2], ["replay", "resume"]);
    assert!(app.slash_filtered.iter().any(|command| command == "new"));

    app.clear_prompt_input();
    for ch in "/nw".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.slash_filtered.first().map(String::as_str), Some("new"));

    app.clear_prompt_input();
    for ch in "/continue".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(
        app.slash_filtered.first().map(String::as_str),
        Some("resume")
    );
}

#[test]
fn slash_alias_executes_matching_command_without_menu() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for ch in "/quit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::QuitRequested]
    );
}

#[test]
fn slash_escape_clears_token_or_restores_prior_draft() {
    let mut fresh = AppState::new_startup(Vec::new(), None);
    for ch in "/re".chars() {
        fresh.handle_key(key(KeyCode::Char(ch)));
    }

    fresh.handle_key(key(KeyCode::Esc));

    assert_eq!(fresh.prompt_buffer, "");
    assert_eq!(fresh.prompt_cursor, 0);
    assert!(!fresh.slash_visible);

    let mut with_draft = AppState::new_startup(Vec::new(), None);
    with_draft.prompt_buffer = "draft".to_string();
    with_draft.prompt_cursor = 0;
    with_draft.handle_key(key(KeyCode::Char('/')));

    with_draft.handle_key(key(KeyCode::Esc));

    assert_eq!(with_draft.prompt_buffer, "draft");
    assert_eq!(with_draft.prompt_cursor, "draft".chars().count());
    assert!(!with_draft.slash_visible);
}

#[test]
fn slash_exit_matches_quit_requested_behavior() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for ch in "/exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::QuitRequested]
    );
}

#[test]
fn slash_menu_supports_mouse_selection() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(key(KeyCode::Char('/')));

    let frame = Rect::new(0, 0, 100, 24);
    let overlay = crate::layout::FrameLayoutPlan::for_app(&app, frame)
        .slash_overlay
        .expect("slash overlay");
    let list_area = crate::layout::slash_command_overlay_content_area(overlay);
    let target_index = app
        .slash_filtered
        .iter()
        .position(|command| command == "new")
        .expect("new slash command visible");
    let target_row = list_area
        .y
        .saturating_add(u16::try_from(target_index).expect("target row fits in u16"));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: list_area.x.saturating_add(1),
            row: target_row,
            modifiers: KeyModifiers::NONE,
        },
        frame,
        None,
        None,
        None,
    );
    assert_eq!(app.slash_selected, target_index);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: list_area.x.saturating_add(1),
            row: target_row,
            modifiers: KeyModifiers::NONE,
        },
        frame,
        None,
        None,
        None,
    );

    assert!(app.startup_shell_visible());
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::NewSession
    );
}

#[test]
fn slash_menu_exposes_model_switcher_when_models_are_configured() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
            vec![ModelOption::from_model_ref("build", "default:gpt-5.4-mini")],
        ),
    );

    app.handle_key(key(KeyCode::Char('/')));

    assert!(app.slash_filtered.iter().any(|command| command == "model"));
}

#[test]
fn startup_mode_uses_pending_launch_metadata() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let app = AppState::new_startup(Vec::new(), None);

    assert_eq!(app.active_profile(), "worker");
    assert_eq!(app.active_provider(), "mock");
    assert_eq!(app.current_model_label(), "model-1");
    assert_eq!(app.launch_mode_label(), Some("Demo"));
}

#[test]
fn lifecycle_shell_state_transitions() {
    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.prompt_buffer = "draft prompt".to_string();

    assert_eq!(
        startup.lifecycle_shell_state(),
        LifecycleShellState::Startup
    );
    assert!(startup.startup_shell_visible());
    assert!(!startup.post_run_handoff_visible());
    assert!(startup.lifecycle_shell_actions_visible());
    assert_eq!(startup.runtime_state().summary, "startup ready");

    let live = AppState::new_live(None, false, None);

    assert_eq!(live.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!live.startup_shell_visible());
    assert!(!live.post_run_handoff_visible());
    assert!(!live.lifecycle_shell_actions_visible());

    let mut finished = AppState::new_live(Some(PathBuf::from("/tmp/live-finished")), false, None);
    finished.ingest_event(envelope(
        1,
        "req_lifecycle_finished",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    assert_eq!(finished.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!finished.startup_shell_visible());
    assert!(!finished.post_run_handoff_visible());
    assert!(!finished.lifecycle_shell_actions_visible());
    assert!(finished.completed_session_shell_active());
    assert!(!finished.composer_disabled());

    let mut failed = AppState::new_live(Some(PathBuf::from("/tmp/live-failed")), false, None);
    failed.ingest_event(envelope(
        1,
        "req_lifecycle_failed",
        EventV1::RunFailed(RunFailedEvent {
            error: "boom".to_string(),
        }),
    ));

    assert_eq!(failed.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!failed.post_run_handoff_visible());
    assert!(!failed.lifecycle_shell_actions_visible());
    assert!(failed.completed_session_shell_active());

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut missing_session_path = AppState::new_live(None, false, Some(fallback_sink));
    missing_session_path.ingest_event(envelope(
        1,
        "req_lifecycle_missing_path",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done without persisted path".to_string(),
        }),
    ));

    assert_eq!(
        missing_session_path.lifecycle_shell_state(),
        LifecycleShellState::None
    );
    assert!(!missing_session_path.post_run_handoff_visible());
    assert!(missing_session_path.completed_session_shell_active());
    assert!(!missing_session_path.composer_disabled());

    let mut terminal_without_routing = AppState::new_live(None, false, None);
    terminal_without_routing.ingest_event(envelope(
        1,
        "req_lifecycle_without_routing",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done without lifecycle routing".to_string(),
        }),
    ));

    assert_eq!(
        terminal_without_routing.lifecycle_shell_state(),
        LifecycleShellState::None
    );
    assert!(!terminal_without_routing.post_run_handoff_visible());
    assert!(terminal_without_routing.completed_session_shell_active());
    assert!(!terminal_without_routing.composer_disabled());
}

#[test]
fn default_shell_registry_exposes_home_and_session_shell_only() {
    let live_registry = default_shell_registry(false);
    assert_eq!(
        live_registry,
        &[
            ShellDescriptor {
                kind: ShellKind::Home,
                label: "Home",
                read_only: false,
            },
            ShellDescriptor {
                kind: ShellKind::Session,
                label: "Session",
                read_only: false,
            },
        ]
    );

    let replay_registry = default_shell_registry(true);
    assert_eq!(
        replay_registry,
        &[
            ShellDescriptor {
                kind: ShellKind::Home,
                label: "Home",
                read_only: false,
            },
            ShellDescriptor {
                kind: ShellKind::Session,
                label: "Replay",
                read_only: true,
            },
        ]
    );
}

#[test]
fn post_run_handoff_ignores_completed_turns_without_terminal_event() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_completed_turn",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_completed_turn".to_string(),
            text: "status?".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_completed_turn",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_completed_turn".to_string(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "status?".to_string(),
            request_digest: "digest-completed-turn".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_completed_turn",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_completed_turn".to_string(),
            result_summary: "all done".to_string(),
            result_digest: "digest-task-completed-turn".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.startup_shell_visible());
    assert!(!app.post_run_handoff_visible());
    assert!(!app.lifecycle_shell_actions_visible());
}

#[test]
fn tool_task_completion_does_not_copy_tool_output_into_activity_transcript() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_tool_completion_transcript",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_tool_completion_transcript".to_string(),
            text: "Inspect tokio docs".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_tool_completion_transcript",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_tool_completion_transcript".to_string(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "Inspect tokio docs".to_string(),
            request_digest: "digest-tool-completion-transcript".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_tool_completion_transcript",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_docs_tokio".to_string(),
            tool_id: "mcp.docs-rs.search_in_crate".to_string(),
            args_summary: r#"{"crate_name":"tokio","query":"spawn"}"#.to_string(),
            args_digest: "digest-docs-tokio-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_tool_completion_transcript",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_docs_tokio".to_string(),
            result_summary: "fn spawn\nstruct JoinHandle".to_string(),
            result_digest: "digest-task-docs-tokio".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_docs_tokio".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let activity = app.activities.front().expect("activity exists");
    assert!(
        activity.transcript_text.is_empty(),
        "tool task completion should not become assistant transcript text"
    );
    assert_eq!(activity.tool_calls.len(), 1);
    assert_eq!(activity.tool_calls[0].tool_call_id, "tc_docs_tokio");
}

#[test]
fn replay_mode_never_reports_lifecycle_shell_actions() {
    let replay = AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            "req_replay_terminal",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(replay.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(!replay.lifecycle_shell_actions_visible());
}
