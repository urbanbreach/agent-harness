use super::*;
use crate::overlay::OverlayKind;
use crate::ui::WheelTarget;
use crate::view_model;
use crossterm::event::MouseEvent;
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, RunStartedEvent,
    TaskCompletedEvent, TaskLineageMetadata, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn opencode_navigation_keybindings() -> BTreeMap<String, String> {
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
        model: model.to_string(),
        variant: variant.map(str::to_string),
        display_label: Some(display_label.to_string()),
        token_window_label: None,
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        reasoning_effort: None,
        text_verbosity: None,
        recommended_for: None,
    }
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
        }),
    )
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
        }]
    );
}

#[test]
fn permission_modal_routes_q_to_quit_without_buffering() {
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

    assert!(app.should_quit);
    assert_eq!(app.prompt_buffer, "keep this draft");
    let intents = intents.lock().expect("lock intents");
    assert_eq!(intents.as_slice(), &[UiIntent::QuitRequested]);
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

    app.handle_key(key(KeyCode::Char('A')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_question_modal".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"]]".to_string()),
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
        "Current runtime: deep · GPT-5.4 Mini · Deterministic"
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
fn live_switch_model_labels_next_turn_only() {
    let launch_option = runtime_context_model_option(
        "deep",
        "default",
        "gpt-5.4-mini",
        Some("deterministic"),
        "GPT-5.4 Mini · Deterministic",
    );
    let next_turn_option = runtime_context_model_option(
        "writer",
        "default",
        "gpt-5.4-mini",
        Some("creative"),
        "GPT-5.4 Mini · Creative",
    );

    let mut live = AppState::new_live(None, false, None);
    live.apply_keybindings(opencode_navigation_keybindings());
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&launch_option)
            .with_available_models(vec![launch_option.clone(), next_turn_option.clone()]),
    );

    live.handle_key(key(KeyCode::Tab));

    let dock = live.control_dock_view_model();
    assert_eq!(
        dock.primary_summary,
        "Current runtime: deep · GPT-5.4 Mini · Deterministic"
    );
    assert_eq!(
        dock.summary_segment,
        Some(view_model::ControlDockSummarySegment {
            kind: view_model::ControlDockSummarySegmentKind::Orchestration,
            text: "Next turns: writer · GPT-5.4 Mini · Creative".to_string(),
            tone: view_model::ControlDockSummaryTone::Secondary,
        })
    );

    let mut replay = AppState::new_replay(
        PathBuf::from("/tmp/runtime-context-replay-switch"),
        Vec::new(),
    );
    replay.apply_keybindings(opencode_navigation_keybindings());
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
    set_pending_live_launch_metadata(
        LaunchMetadata::new("deep", "default", Some("gpt-5.4-mini".to_string()))
            .with_mode_label("Live"),
    );

    let app = AppState::new_live(None, false, None);

    assert!(!app.details_drawer_open());
    assert_eq!(app.focus, Focus::Prompt);
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
        Some(WheelTarget::Transcript),
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
        Some(WheelTarget::Transcript),
    );
    assert_eq!(app.transcript_scroll, 0);
    assert!(app.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
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
        Some(WheelTarget::Inspector),
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
        Some(WheelTarget::Inspector),
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
        None,
    );

    assert_eq!(app.details_scroll, 6);
    assert_eq!(app.transcript_scroll, 2);
    assert!(!app.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
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
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text } if text == "next")),
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
fn startup_prompt_enter_emits_submit_intent_and_quits_launcher() {
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
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "ship it".to_string(),
        }]
    );
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
        }]
    );
}

#[test]
fn tool_call_finished_plan_exit_handoff_emits_switch_model_then_submit_prompt() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    let expected_launch_metadata = LaunchMetadata::from_model_ref("build", "mock:model-2")
        .with_available_models(vec![
            ModelOption::from_model_ref("plan", "mock:model-1"),
            ModelOption::from_model_ref("build", "mock:model-2"),
        ])
        .with_mode_label("Demo");
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("plan", "mock:model-1")
            .with_available_models(expected_launch_metadata.available_models().to_vec())
            .with_mode_label("Demo"),
    );

    app.ingest_event(envelope(
        1,
        "req_plan_exit_handoff",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_plan_exit_handoff".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("plan exit handoff ready".to_string()),
            output_digest: Some("digest-plan-exit".to_string()),
            output_json: Some(serde_json::json!({
                "plan_exit_handoff": {
                    "source_profile": "plan",
                    "target_profile": "build",
                    "prompt": "The plan has been approved, you can now edit files. Execute the plan."
                }
            })),
            metadata: None,
        }),
    ));

    assert_eq!(app.active_profile(), "build");
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::SwitchModel {
                profile: "build".to_string(),
                launch_metadata: expected_launch_metadata,
            },
            UiIntent::SubmitPrompt {
                text: "The plan has been approved, you can now edit files. Execute the plan."
                    .to_string(),
            },
        ]
    );

    let _ = take_pending_live_launch_metadata();
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
fn child_session_navigation_keybinds_follow_opencode_contract() {
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

    let mut parent_app =
        AppState::new_live(Some(parent_dir.clone()), false, Some(Arc::clone(&sink)));
    parent_app.apply_keybindings(opencode_navigation_keybindings());
    for event in parent_events.clone() {
        parent_app.ingest_event(event);
    }
    parent_app.focus = Focus::Prompt;
    parent_app.handle_key(key_with_modifiers(
        KeyCode::Char(']'),
        KeyModifiers::CONTROL,
    ));
    assert!(parent_app.prompt_buffer.is_empty());

    let mut child_app =
        AppState::new_live(Some(child_a_dir.clone()), false, Some(Arc::clone(&sink)));
    child_app.apply_keybindings(opencode_navigation_keybindings());
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
    reverse_app.apply_keybindings(opencode_navigation_keybindings());
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
                run_id: "child_a".to_string(),
                run_dir: child_a_dir.clone(),
            },
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
    app.apply_keybindings(opencode_navigation_keybindings());
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
