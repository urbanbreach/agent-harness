// SIZE_OK: Permission-modal regressions share private AppState fixtures and exact transitions.
use super::super::permission_prompt::PermissionPointerTarget;
use super::*;
use crate::UnwrapOrAbort;

pub(super) fn overlay_stack_orders_details_palette_permission() {
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
            tool_call_id: Some("tc_overlay_stack".into()),
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

pub(super) fn overlay_stack_orders_permission_above_commands_and_slash() {
    AppState::exact_test_overlay_stack_orders_permission_above_commands_and_slash();
}

pub(super) fn permission_modal_preempts_palette() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
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
            tool_call_id: Some("tc_overlay_preempt".into()),
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
    let intents = intents.lock().unwrap_or_abort();
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

pub(super) fn permission_modal_ignores_unmapped_chars_without_buffering() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.composer.prompt_buffer = "keep this draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_modal_quit",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_quit".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_quit".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-quit".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Char('q')));

    assert!(!app.should_quit);
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    let intents = intents.lock().unwrap_or_abort();
    assert!(intents.is_empty());
    assert!(app.active_permission().is_some());
}

pub(super) fn permission_modal_escape_rejects_without_hiding_pending_permission() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_modal_escape",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_escape".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_escape".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-escape".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Esc));

    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
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

pub(super) fn permission_modal_ctrl_n_emits_deny_intent_without_hiding_pending_permission() {
    // arrange
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.composer.prompt_buffer = "keep this draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_modal_ctrl_n",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_ctrl_n".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_ctrl_n".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-ctrl-n".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    // act
    app.handle_key(key_with_modifiers(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    ));

    // assert
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_modal_ctrl_n".to_string(),
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

pub(super) fn question_permission_modal_collects_answers_and_emits_reason_payload() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_question_modal",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_modal".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_question_modal".into()),
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
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_question_modal".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"]]".to_string()),
            grant_scope: None,
        }]
    );
}

pub(super) fn question_permission_modal_multi_question_uses_tabs_before_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_question_tabs",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_tabs".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_question_tabs".into()),
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
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_question_tabs".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"],[\"B\"]]".to_string()),
            grant_scope: None,
        }]
    );
}

pub(super) fn question_modal_ignores_digits_past_visible_choices() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_question_digit_bounds",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_digit_bounds".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_digit_bounds".into()),
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

pub(super) fn question_modal_multi_custom_selection_toggles_saved_custom_answer() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_question_multi_custom",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_multi_custom".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_multi_custom".into()),
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

pub(super) fn question_modal_submit_allows_unanswered_questions_on_confirm() {
    let intents = std::sync::Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = std::sync::Arc::clone(&intents);
        std::sync::Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_question_partial_submit",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_partial_submit".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_partial_submit".into()),
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

    let intents = intents.lock().unwrap_or_abort();
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

pub(super) fn permission_modal_allow_always_requests_durable_run_grant() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_modal_allow_always_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_allow_always_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_allow_always_1".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-allow-always".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

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

    assert!(
        app.always_approve_mode(),
        "confirming always-approve must engage session always-approve mode"
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_modal_allow_always_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Run),
        }]
    );
}

pub(super) fn always_approve_mode_auto_allows_subsequent_non_question_permission() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_always_mode_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_always_mode_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_always_mode_1".into()),
            summary: "first permission".to_string(),
            request_digest: "digest-always-mode-1".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.always_approve_mode());

    app.ingest_event(envelope(
        2,
        "req_always_mode_1",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_always_mode_1".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));
    intents.lock().unwrap_or_abort().clear();

    app.ingest_event(envelope(
        3,
        "req_always_mode_2",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_always_mode_2".to_string(),
            kind: "bash".to_string(),
            tool_call_id: Some("tc_always_mode_2".into()),
            summary: "second permission".to_string(),
            request_digest: "digest-always-mode-2".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_always_mode_2".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Run),
        }],
        "always-approve mode must auto-allow subsequent non-question permissions"
    );
}

pub(super) fn always_approve_mode_appends_composer_badge_suffix() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(crate::app::LaunchMetadata::new(
        "build",
        "test-provider",
        Some("model-tx".to_string()),
    ));
    app.ingest_event(envelope(
        1,
        "req_always_badge_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_always_badge_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_always_badge_1".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-always-badge".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.always_approve_mode());

    app.ingest_event(envelope(
        2,
        "req_always_badge_1",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_always_badge_1".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));
    assert!(app.active_permission_view().is_none());

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("always-approve"),
        "composer badge must show · always-approve when mode is engaged\n{debug}"
    );
}

pub(super) fn permission_modal_ctrl_o_opens_always_approve_confirm() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_modal_ctrl_o_always_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_ctrl_o_always_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_ctrl_o_always_1".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-ctrl-o-always".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(
        app.permission_modal_selection("perm_modal_ctrl_o_always_1"),
        PermissionModalSelection::AllowOnce
    );

    app.handle_key(key_with_modifiers(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.permission_modal_stage("perm_modal_ctrl_o_always_1"),
        PermissionModalStage::AlwaysConfirm
    );
}

pub(super) fn permission_modal_allow_session_requests_session_grant() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(envelope(
        1,
        "req_modal_allow_session_1",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_allow_session_1".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_allow_session_1".into()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-allow-session".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    // Default selection is AllowAlways; cycle once to AllowSession (freeze option 2).
    app.handle_key(key(KeyCode::Right));
    assert_eq!(
        app.permission_modal_selection("perm_modal_allow_session_1"),
        PermissionModalSelection::AllowSession
    );

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_modal_allow_session_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Session),
        }]
    );
}

fn mouse_event(kind: MouseEventKind, area: Rect) -> MouseEvent {
    MouseEvent {
        kind,
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    }
}

fn three_choice_question_event(permission_id: &str) -> EventEnvelopeV1 {
    envelope(
        1,
        permission_id,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_mouse".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Which color?",
                    "header": "Color",
                    "options": [
                        {"label": "A", "description": "Option A"},
                        {"label": "B", "description": "Option B"},
                        {"label": "C", "description": "Option C"}
                    ],
                    "multiple": false,
                    "custom": false
                }]
            })
            .to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn edit_permission_event(seq: u64, permission_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        permission_id,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

#[test]
fn permission_mouse_hit_regions_match_the_rendered_option_rows() {
    // Given: the four-choice permission dock at the primary parity viewport.
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(edit_permission_event(
        1,
        "perm_mouse_regions",
        "tool_call_mouse_regions",
    ));

    // When: the pointer map is derived from the active frame plan.
    let regions = app.permission_prompt_hit_regions_for_test(frame_area);

    // Then: only the four painted option rows are interactive.
    assert_eq!(
        regions,
        vec![
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::AllowAlways),
                Rect::new(5, 26, 112, 1),
            ),
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::AllowSession),
                Rect::new(5, 27, 112, 1),
            ),
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::AllowOnce),
                Rect::new(5, 28, 112, 1),
            ),
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::Reject),
                Rect::new(5, 29, 112, 1),
            ),
        ]
    );

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.permission_prompt_hit_regions_for_test(frame_area),
        vec![
            (
                PermissionPointerTarget::Confirm(PermissionConfirmSelection::Confirm),
                Rect::new(5, 26, 11, 1),
            ),
            (
                PermissionPointerTarget::Confirm(PermissionConfirmSelection::Cancel),
                Rect::new(17, 26, 10, 1),
            ),
        ]
    );
}

#[test]
fn question_mouse_hit_regions_match_the_rendered_option_rows() {
    // Given: a three-choice question dock at the primary parity viewport.
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(three_choice_question_event("question_mouse_regions"));

    // When: the pointer map is derived from question content packing.
    let regions = app.permission_prompt_hit_regions_for_test(frame_area);

    // Then: each painted question row has one full-width deterministic target.
    assert_eq!(
        regions,
        vec![
            (
                PermissionPointerTarget::QuestionChoice(0),
                Rect::new(5, 24, 111, 1),
            ),
            (
                PermissionPointerTarget::QuestionChoice(1),
                Rect::new(5, 25, 111, 1),
            ),
            (
                PermissionPointerTarget::QuestionChoice(2),
                Rect::new(5, 26, 111, 1),
            ),
        ]
    );
}

#[test]
fn permission_mouse_click_selects_before_emitting_only_a_resolution_intent() {
    // Given: a permission dock backed by an intent sink.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(edit_permission_event(
        1,
        "perm_mouse_select",
        "tool_call_mouse_select",
    ));
    let option_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::Decision(PermissionModalSelection::AllowOnce))
                .then_some(area)
        })
        .unwrap_or_abort();

    // When: pointer-down selects the row and pointer-up activates that same row.
    let pressed = app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: press feedback is local UI state and cannot resolve or execute anything.
    assert!(pressed);
    assert_eq!(
        app.permission_modal_selection("perm_mouse_select"),
        PermissionModalSelection::AllowOnce
    );
    assert!(intents.lock().unwrap_or_abort().is_empty());

    let released = app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );
    assert!(released);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_mouse_select".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }]
    );
    assert!(app.active_permission().is_some());
    assert!(app.permission_submission_pending("perm_mouse_select"));
}

#[test]
fn permission_always_mouse_requires_confirmation_before_emitting_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(edit_permission_event(
        1,
        "permission_mouse_always",
        "tool_call_mouse_always",
    ));
    let always_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::Decision(PermissionModalSelection::AllowAlways))
                .then_some(area)
        })
        .unwrap_or_abort();

    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), always_area),
        frame_area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), always_area),
        frame_area,
        None,
        None,
        None,
    );

    assert_eq!(
        app.permission_modal_stage("permission_mouse_always"),
        PermissionModalStage::AlwaysConfirm
    );
    assert!(intents.lock().unwrap_or_abort().is_empty());
    let confirm_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::Confirm(PermissionConfirmSelection::Confirm))
                .then_some(area)
        })
        .unwrap_or_abort();

    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), confirm_area),
        frame_area,
        None,
        None,
        None,
    );
    assert!(intents.lock().unwrap_or_abort().is_empty());
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), confirm_area),
        frame_area,
        None,
        None,
        None,
    );

    assert!(app.always_approve_mode());
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "permission_mouse_always".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Run),
        }]
    );
}

#[test]
fn question_mouse_click_preserves_shell_state_and_emits_only_answer_intent() {
    // Given: a detached transcript, parked list focus, and preserved composer draft.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::List;
    app.composer.prompt_buffer = "preserved question draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.transcript_view.transcript_scroll = 4;
    app.transcript_view.follow_mode = false;
    let composer_before = FrameLayoutPlan::for_app(&app, frame_area).composer;
    app.ingest_event(three_choice_question_event("question_mouse_select"));
    let option_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionChoice(1)).then_some(area)
        })
        .unwrap_or_abort();

    // When: the B row is pressed and released in place.
    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: pointer-down changes selection but emits no permission decision.
    assert_eq!(app.question_prompt_selection("question_mouse_select"), 1);
    assert!(intents.lock().unwrap_or_abort().is_empty());

    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "question_mouse_select".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"B\"]]".to_string()),
            grant_scope: None,
        }]
    );
    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.composer.prompt_buffer, "preserved question draft");
    assert_eq!(app.transcript_view.transcript_scroll, 4);
    assert!(!app.transcript_view.follow_mode);
    let composer_after = FrameLayoutPlan::for_app(&app, frame_area).composer;
    assert_eq!(
        composer_after.map(|area| area.y),
        composer_before.map(|area| area.y.saturating_add(1))
    );
    assert_eq!(
        composer_after.map(|area| area.height),
        composer_before.map(|area| area.height)
    );
    assert_eq!(
        composer_after.map(|area| area.width),
        composer_before.map(|area| area.width)
    );
}

pub(super) fn permission_modal_restores_focus_after_authoritative_resolution() {
    // Given: a permission modal that preempts list focus.
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    app.ingest_event(envelope(
        1,
        "req_permission_focus_restore",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_focus_restore".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tool_focus_restore".into()),
            summary: "edit requires permission".to_string(),
            request_digest: "digest-focus-restore".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal)
    );

    // When: an unrelated local transition changes focus before coordinator resolution.
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        2,
        "req_permission_focus_restore",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_focus_restore".to_string(),
            decision: harness_core::event::PermissionDecision::Deny,
            reason: Some("operator denied".to_string()),
        }),
    ));

    // Then: closing the modal restores the owner that was focused before preemption.
    assert_eq!(app.overlay_stack().top(), None);
    assert_eq!(app.focus, Focus::List);
}

#[test]
fn permission_decision_waits_for_resolution_then_resumes_and_settles_tool() {
    // Given: a running request paused at a permission gate with parked shell state.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::List;
    app.composer.prompt_buffer = "stable permission draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.transcript_view.transcript_scroll = 3;
    app.transcript_view.follow_mode = false;
    let composer_before = FrameLayoutPlan::for_app(&app, frame_area).composer;
    app.ingest_event(envelope(
        1,
        "req_permission_lifecycle",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_permission_lifecycle".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "permission lifecycle".to_string(),
            request_digest: "digest-permission-lifecycle".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_permission_lifecycle",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool_call_permission_lifecycle".into(),
            tool_id: "edit".to_string(),
            args_summary: "edit demo.txt".to_string(),
            args_digest: "digest-permission-lifecycle-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(edit_permission_event(
        3,
        "perm_mouse_lifecycle",
        "tool_call_permission_lifecycle",
    ));
    let option_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::Decision(PermissionModalSelection::AllowOnce))
                .then_some(area)
        })
        .unwrap_or_abort();

    // When: the user clicks allow-once, only the intent is emitted initially.
    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: the tool remains paused until coordinator-owned events advance it.
    assert_eq!(
        app.activities[0].tool_calls[0].status,
        ToolCallDisplayStatus::PendingPermission
    );
    app.ingest_event(envelope(
        4,
        "req_permission_lifecycle",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_mouse_lifecycle".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));
    assert_eq!(
        app.activities[0].tool_calls[0].status,
        ToolCallDisplayStatus::Queued
    );
    app.ingest_event(envelope(
        5,
        "req_permission_lifecycle",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool_call_permission_lifecycle".into(),
        }),
    ));
    assert_eq!(
        app.activities[0].tool_calls[0].status,
        ToolCallDisplayStatus::Running
    );
    app.ingest_event(envelope(
        6,
        "req_permission_lifecycle",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool_call_permission_lifecycle".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edit complete".to_string()),
            output_digest: Some("digest-permission-lifecycle-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    assert_eq!(
        app.activities[0].tool_calls[0].status,
        ToolCallDisplayStatus::Succeeded
    );
    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.composer.prompt_buffer, "stable permission draft");
    assert_eq!(app.transcript_view.transcript_scroll, 3);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(
        FrameLayoutPlan::for_app(&app, frame_area).composer,
        composer_before
    );
}
