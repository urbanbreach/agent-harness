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

pub(super) fn permission_modal_escape_parks_and_tab_restores_without_answering() {
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

    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert!(app.active_permission().is_some());
    assert_eq!(app.focus, Focus::List);

    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.focus, Focus::Prompt);
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn permission_modal_tab_walks_rows_and_modified_tab_is_inert() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(edit_permission_event(1, "perm_tab_walk", "tc_tab_walk"));

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(
        app.permission_modal_selection("perm_tab_walk"),
        PermissionModalSelection::AllowSession
    );
    app.handle_key(key_with_modifiers(KeyCode::Tab, KeyModifiers::SHIFT));
    assert_eq!(
        app.permission_modal_selection("perm_tab_walk"),
        PermissionModalSelection::AllowAlways
    );
    for (code, modifiers) in [
        (KeyCode::BackTab, KeyModifiers::NONE),
        (KeyCode::BackTab, KeyModifiers::SHIFT),
    ] {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(edit_permission_event(1, "perm_backtab", "tc_backtab"));
        app.handle_key(key_with_modifiers(code, modifiers));
        assert_eq!(
            app.permission_modal_selection("perm_backtab"),
            PermissionModalSelection::Reject,
            "{code:?}/{modifiers:?}"
        );
    }

    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
    ] {
        app.handle_key(key_with_modifiers(KeyCode::Tab, modifiers));
        assert_eq!(
            app.permission_modal_selection("perm_tab_walk"),
            PermissionModalSelection::AllowAlways
        );
    }
}

pub(super) fn permission_option_enter_accepts_every_modifier() {
    for modifiers in [
        KeyModifiers::NONE,
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
    ] {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink_intents = Arc::clone(&intents);
        let sink = Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
        let mut app = AppState::new_live(None, false, Some(sink));
        app.ingest_event(edit_permission_event(
            1,
            "perm_enter_modifiers",
            "tc_enter_modifiers",
        ));
        app.handle_key(key(KeyCode::Tab));
        app.handle_key(key(KeyCode::Tab));

        app.handle_key(key_with_modifiers(KeyCode::Enter, modifiers));

        assert_eq!(intents.lock().unwrap_or_abort().len(), 1, "{modifiers:?}");
    }
}

pub(super) fn permission_modal_ctrl_c_cancels_after_escape_only_parks() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink = Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(edit_permission_event(1, "perm_ctrl_c", "tc_ctrl_c"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(intents.lock().unwrap_or_abort().len(), 1);
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

pub(super) fn question_permission_modal_tabs_walk_rows_and_arrows_switch_questions() {
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

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.question_prompt_tab("perm_question_tabs"), 0);
    assert_eq!(app.question_prompt_selection("perm_question_tabs"), 1);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.question_prompt_selection("perm_question_tabs"), 1);
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.question_prompt_selection("perm_question_tabs"), 0);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.question_prompt_tab("perm_question_tabs"), 1);
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn question_option_enter_accepts_modifiers_while_modified_tab_is_inert() {
    for modifiers in [
        KeyModifiers::NONE,
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
    ] {
        let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
        let sink_intents = Arc::clone(&intents);
        let sink = Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
        let mut app = AppState::new_live(None, false, Some(sink));
        app.ingest_event(three_choice_question_event("question_enter_modifiers"));

        app.handle_key(key_with_modifiers(KeyCode::Enter, modifiers));

        assert_eq!(intents.lock().unwrap_or_abort().len(), 1, "{modifiers:?}");
    }

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(three_choice_question_event("question_tab_modifiers"));
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
    ] {
        app.handle_key(key_with_modifiers(KeyCode::Tab, modifiers));
        assert_eq!(app.question_prompt_selection("question_tab_modifiers"), 0);
    }
    for (code, modifiers) in [
        (KeyCode::BackTab, KeyModifiers::NONE),
        (KeyCode::BackTab, KeyModifiers::SHIFT),
        (KeyCode::Tab, KeyModifiers::SHIFT),
    ] {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(three_choice_question_event("question_backtab"));
        app.handle_key(key_with_modifiers(code, modifiers));
        assert_eq!(
            app.question_prompt_selection("question_backtab"),
            2,
            "{code:?}/{modifiers:?}"
        );
    }
}

pub(super) fn question_escape_ladder_clears_then_parks_and_cancel_chords_deny() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink = Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.composer.prompt_buffer = "keep this draft".to_string();
    app.ingest_event(three_choice_question_event("question_esc_ladder"));
    app.handle_key(key_with_modifiers(KeyCode::Char(' '), KeyModifiers::NONE));

    app.handle_key(key(KeyCode::Esc));
    assert!(app.question_prompt_answers("question_esc_ladder")[0].is_empty());
    assert_eq!(app.focus, Focus::Prompt);
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::List);
    assert!(intents.lock().unwrap_or_abort().is_empty());
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Prompt);
    app.handle_key(key_with_modifiers(KeyCode::Char('X'), KeyModifiers::SHIFT));
    assert_eq!(intents.lock().unwrap_or_abort().len(), 1);
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
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

pub(super) fn question_modal_multi_custom_answer_coexists_with_fixed_options() {
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

    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.question_prompt_editing("perm_question_multi_custom"));

    app.handle_key(key(KeyCode::Char('x')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    assert!(!app.question_prompt_editing("perm_question_multi_custom"));
    assert_eq!(
        app.question_prompt_answers("perm_question_multi_custom"),
        vec![vec!["A".to_string()]]
    );

    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('x')));
    app.handle_key(key(KeyCode::Enter));

    assert!(!app.question_prompt_editing("perm_question_multi_custom"));
    assert_eq!(
        app.question_prompt_answers("perm_question_multi_custom"),
        vec![vec!["A".to_string(), "x".to_string()]]
    );
}

pub(super) fn question_text_enter_modifiers_route_commit_newline_or_inert() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_question_text_modifiers",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_text_modifiers".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tc_question_text_modifiers".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Explain",
                    "header": "Text",
                    "options": [],
                    "custom": true
                }]
            })
            .to_string(),
            request_digest: "digest-question-text-modifiers".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('a')));

    app.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::SHIFT));
    app.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::ALT));
    app.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::CONTROL));
    app.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::SUPER));

    assert_eq!(
        app.question_answer_preview("perm_question_text_modifiers"),
        "a\n\n█"
    );
}

pub(super) fn permission_queue_restores_only_original_focus_and_preserved_draft() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    app.composer.prompt_buffer = "queued draft".to_string();
    app.ingest_event(edit_permission_event(
        1,
        "perm_queue_first",
        "tc_queue_first",
    ));
    app.ingest_event(edit_permission_event(
        2,
        "perm_queue_second",
        "tc_queue_second",
    ));
    assert_eq!(app.focus, Focus::Prompt);

    app.ingest_event(envelope(
        3,
        "req_queue_first",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_queue_first".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));
    assert_eq!(app.focus, Focus::Prompt);
    app.ingest_event(envelope(
        4,
        "req_queue_second",
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_queue_second".to_string(),
            decision: harness_core::event::PermissionDecision::Deny,
            reason: None,
        }),
    ));

    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.composer.prompt_buffer, "queued draft");
}

pub(super) fn question_ctrl_c_cancels_answered_questions() {
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
    assert_eq!(app.question_prompt_tab("perm_question_partial_submit"), 1);
    assert!(intents.lock().unwrap_or_abort().is_empty());
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0],
        UiIntent::ResolvePermission {
            permission_id: "perm_question_partial_submit".to_string(),
            decision: PermissionDecision::Deny,
            reason: None,
            grant_scope: None,
        }
    );
}

pub(super) fn question_y_copies_the_focused_option_label_and_description() {
    // Given: a focused question and an observable clipboard hook.
    let _clipboard_guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let copied_hook = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        copied_hook.lock().unwrap_or_abort().push(text.to_string());
        Ok(())
    })));
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(three_choice_question_event("question_copy_option"));

    // When: the operator focuses the second option and presses y.
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char('y')));

    // Then: Grok's label-newline-description payload reaches the clipboard.
    assert_eq!(copied.lock().unwrap_or_abort().as_slice(), ["B\nOption B"]);
}

include!("permission_modal_tests_part2_test.rs");
include!("permission_modal_tests_part3_test.rs");
