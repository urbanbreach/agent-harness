use super::*;
use crate::UnwrapOrAbort;

pub(super) fn permission_modal_snapshot_renders_request() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &["Allow Edit", "always-approve", "No, reject", "Yes"],
    );
}

pub(super) fn overlay_stack_orders_details_palette_permission() {
    let mut app = app::AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[
            overlay::OverlayKind::DetailsDrawer,
            overlay::OverlayKind::CommandPalette,
        ]
    );

    app.ingest_event(permission_requested_event(
        1,
        "perm_stack_order",
        "tool_call_stack_order",
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[
            overlay::OverlayKind::DetailsDrawer,
            overlay::OverlayKind::PermissionModal,
        ]
    );
}

pub(super) fn permission_modal_preempts_palette() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette",
        "tool_call_preempt_palette",
    ));

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::PermissionModal)
    );

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_preempt_palette".to_string(),
            decision: harness_core::perm::PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }]
    );
}

pub(super) fn focus_returns_after_palette_close() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.composer.prompt_buffer = "keep prompt draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    assert!(app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.composer.prompt_buffer, "keep prompt draft");
    let open_debug = render_live_screen(&app, 120, 36);
    println!("PALETTE_OPEN\n{open_debug}");

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.composer.prompt_buffer, "keep prompt draft");
    assert_eq!(
        app.composer.prompt_cursor,
        "keep prompt draft".chars().count()
    );
    let closed_debug = render_live_screen(&app, 100, 24);
    println!("PALETTE_CLOSED\n{closed_debug}");
}

pub(super) fn live_status_strip_distinguishes_terminal_states() {
    let ready = app::AppState::new_live(None, false, None);
    assert_runtime_state(
        &ready,
        app::RuntimeStateKind::Ready,
        "ready for first turn",
        None,
        false,
        "Type a prompt for the next turn…",
    );

    let mut sending = app::AppState::new_live(None, false, None);
    for c in "hello".chars() {
        sending.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    sending.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_runtime_state(
        &sending,
        app::RuntimeStateKind::Sending,
        "turn 1 · response starting",
        None,
        false,
        "Draft the next prompt while the response starts…",
    );

    sending.ingest_event(envelope(
        1,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_phase".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "hello".to_string(),
                request_digest: "digest-phase".to_string(),
                metadata: None,
            },
        ),
    ));
    sending.ingest_event(envelope(
        2,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_phase".into(),
                delta: "streaming text".to_string(),
            },
        ),
    ));

    assert_runtime_state(
        &sending,
        app::RuntimeStateKind::Streaming,
        "turn 1 · response in progress",
        None,
        false,
        "Draft the next prompt while the response continues…",
    );

    sending.ingest_event(envelope(
        3,
        Some("req_phase"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_phase".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    assert_runtime_state(
        &sending,
        app::RuntimeStateKind::Success,
        "turn 1 · ready for next turn",
        None,
        false,
        "Type a prompt for the next turn…",
    );

    let mut cancelled = app::AppState::new_live(None, false, None);
    cancelled.ingest_event(envelope(
        1,
        Some("req_cancel"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_cancel".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "cancel".to_string(),
                request_digest: "digest-cancel".to_string(),
                metadata: None,
            },
        ),
    ));
    cancelled.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            failure: false,
            task_id: "req_cancel".to_string().into(),
            reason: "operator cancelled".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));
    let cancelled_debug = render_live_buffer(&cancelled, 80, 24);
    assert_runtime_state(
        &cancelled,
        app::RuntimeStateKind::Cancelled,
        "last turn cancelled · operator cancelled",
        Some("operator cancelled"),
        false,
        "Type a prompt to retry the cancelled turn…",
    );
    assert!(!cancelled_debug.contains("request_digest="));

    let mut errored = app::AppState::new_live(None, false, None);
    errored.ingest_event(envelope(
        1,
        Some("req_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_error".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "fail".to_string(),
                request_digest: "digest-error".to_string(),
                metadata: None,
            },
        ),
    ));
    errored.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "API rate limit exceeded".to_string(),
        }),
    ));
    let error_debug = render_live_buffer(&errored, 80, 24);
    assert!(error_debug.contains("API rate limit exceeded"));

    assert_permission_and_connection_status_strips();
    assert_tool_runtime_states();
}

fn assert_permission_and_connection_status_strips() {
    let mut permission_blocked = app::AppState::new_live(None, false, None);
    permission_blocked.ingest_event(permission_requested_event(1, "perm_blocked", "tool_call_1"));
    assert_runtime_state(&permission_blocked, app::RuntimeStateKind::PermissionBlocked, "decision required · Apply hashline edit to demo.txt", Some("Apply hashline edit to demo.txt"), false, "Draft preserved under the checkpoint — deny stays fail-closed; allow once only after review.");
    let permission_blocked_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_blocked_debug.contains("Allow Edit"));
    assert!(permission_blocked_debug.contains("always-approve"));
    assert!(permission_blocked_debug.contains("No, reject"));
    assert!(
        permission_blocked_debug.contains("always-approve")
            || permission_blocked_debug.contains("enter")
            || permission_blocked_debug.contains("confirm")
            || permission_blocked_debug.contains("←→")
    );

    permission_blocked.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_runtime_state(&permission_blocked, app::RuntimeStateKind::PermissionPending, "decision submitted · awaiting confirmation · Apply hashline edit to demo.txt", Some("Apply hashline edit to demo.txt"), true, "Draft preserved while Harness records the decision. Wait for confirmation before sending again.");
    let permission_pending_debug = render_live_buffer(&permission_blocked, 80, 24);
    assert!(permission_pending_debug.contains("decision sent"));
    // Pending dock actions copy uses "waiting for confirmation" (summary string uses awaiting).
    assert!(permission_pending_debug.contains("waiting for confirmation"));

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    let degraded_debug = render_live_buffer(&degraded, 80, 24);
    assert!(degraded_debug.contains("Degraded"));
    assert!(degraded_debug.contains("replaying from seq 1"));
    assert!(!degraded_debug.contains("Composer ·"));
    assert!(!degraded_debug.contains("Draft preserved locally"));
    assert!(degraded_debug.contains("Draft locally until recovery completes."));
    assert!(degraded_debug.contains("Recovery in progress"));

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    let disconnected_debug = render_live_buffer(&disconnected, 80, 24);
    assert!(disconnected_debug.contains("Disconnected"));
    assert!(!disconnected_debug.contains("Composer ·"));
    assert!(!disconnected_debug.contains("Draft preserved locally"));
    assert!(disconnected_debug.contains("Reopen the TUI, then continue from the transcript."));
}

fn assert_runtime_state(
    app: &app::AppState,
    kind: app::RuntimeStateKind,
    summary: &str,
    detail: Option<&str>,
    disabled: bool,
    hint: &str,
) {
    assert_eq!(
        app.runtime_state(),
        app::RuntimeState {
            kind,
            summary: summary.to_string(),
            detail: detail.map(str::to_string),
            composer_disabled: disabled,
            composer_hint: hint.to_string(),
        }
    );
    assert_eq!(app.composer_disabled(), disabled);
}

fn assert_tool_runtime_states() {
    use harness_core::event::{EventV1, ToolCallMetadata, ToolCallStatus};
    for (count, failed, kind, summary, detail, hint) in [
        (
            7,
            false,
            app::RuntimeStateKind::Streaming,
            "tool queued · agent.spawn",
            Some("inspect"),
            "Draft the next prompt while the queued tool waits to start…",
        ),
        (
            8,
            false,
            app::RuntimeStateKind::Streaming,
            "tool running · agent.spawn",
            Some("inspect"),
            "Draft the next prompt while the tool runs…",
        ),
        (
            9,
            false,
            app::RuntimeStateKind::Streaming,
            "tool finished · waiting for final response · agent.spawn",
            None,
            "Draft the next prompt while the assistant finishes after the tool result…",
        ),
        (
            9,
            true,
            app::RuntimeStateKind::Failure,
            "tool failed · agent.spawn",
            Some("tool output"),
            "After review, adjust the draft, then retry or continue.",
        ),
    ] {
        let mut app = app::AppState::new_live(None, false, None);
        for mut event in session_view_events().into_iter().take(count) {
            match &mut event.payload {
                EventV1::ToolCallRequested(tool) => {
                    tool.tool_id = "task".to_string();
                    tool.args_summary = "inspect".to_string();
                    tool.metadata = Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: Some("task".to_string()),
                        ..ToolCallMetadata::default()
                    });
                }
                EventV1::ToolCallFinished(tool) if failed => tool.status = ToolCallStatus::Failed,
                _ => {}
            }
            app.ingest_event(event);
        }
        // Provider progress keeps the turn active while its tool advances.
        app.ingest_event(envelope(
            20,
            Some("req_001"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_001".into(),
                delta: "Checking the result".to_string(),
            }),
        ));
        assert_runtime_state(&app, kind, summary, detail, false, hint);
    }
}
