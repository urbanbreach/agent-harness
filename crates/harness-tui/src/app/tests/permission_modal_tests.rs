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
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_modal_allow_always_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: Some(harness_core::perm::PermissionGrantScope::Run),
        }]
    );
}
