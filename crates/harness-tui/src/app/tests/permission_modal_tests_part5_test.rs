fn permission_feedback_fixture() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&intents);
    let sink = Arc::new(move |intent| captured.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.composer.prompt_buffer = "preserved draft".to_string();
    app.composer.prompt_cursor = 3;
    app.ingest_event(edit_permission_event(1, "perm_feedback", "tc_feedback"));
    (app, intents)
}

#[test]
fn permission_modal_number_shortcuts_use_guarded_decisions() {
    for (number, decision, grant_scope) in [
        (
            '2',
            PermissionDecision::Allow,
            Some(harness_core::perm::PermissionGrantScope::Session),
        ),
        ('3', PermissionDecision::Allow, None),
        ('4', PermissionDecision::Deny, None),
    ] {
        // Given a focused permission with no submitted decision.
        let (mut app, intents) = permission_feedback_fixture();

        // When a numbered choice is activated, then repeated before acknowledgement.
        app.handle_key(key(KeyCode::Char(number)));
        app.handle_key(key(KeyCode::Char(number)));
        app.handle_key(key(KeyCode::Enter));

        // Then exactly one scoped decision is sent and the card remains pending.
        assert_eq!(
            intents.lock().unwrap_or_abort().as_slice(),
            &[UiIntent::ResolvePermission {
                permission_id: "perm_feedback".to_string(),
                decision,
                reason: None,
                grant_scope,
            }],
            "option {number}"
        );
        assert!(app.active_permission().is_some());
        assert_eq!(app.composer.prompt_buffer, "preserved draft");
    }
}

#[test]
fn permission_modal_number_one_requires_always_confirmation() {
    // Given an unsubmitted permission and an unrelated selected row.
    let (mut app, intents) = permission_feedback_fixture();
    app.handle_key(key(KeyCode::Right));

    // When option one is activated.
    app.handle_key(key(KeyCode::Char('1')));

    // Then it opens confirmation without granting permission or changing mode.
    assert_eq!(
        app.permission_modal_stage("perm_feedback"),
        PermissionModalStage::AlwaysConfirm
    );
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert!(!app.always_approve_mode());
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(
        app.permission_modal_stage("perm_feedback"),
        PermissionModalStage::Decision
    );
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

#[test]
fn permission_modal_vertical_navigation_preserves_modifier_gates() {
    for (code, expected) in [
        (KeyCode::Up, PermissionModalSelection::Reject),
        (KeyCode::Char('k'), PermissionModalSelection::Reject),
        (KeyCode::Down, PermissionModalSelection::AllowSession),
        (KeyCode::Char('j'), PermissionModalSelection::AllowSession),
    ] {
        // Given the default permission choice.
        let (mut app, intents) = permission_feedback_fixture();

        // When a vertical navigation key is used.
        app.handle_key(key(code));

        // Then it walks the same choices without resolving anything.
        assert_eq!(
            app.permission_modal_selection("perm_feedback"),
            expected,
            "{code:?}"
        );
        assert!(intents.lock().unwrap_or_abort().is_empty());
    }
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
    ] {
        let (mut app, intents) = permission_feedback_fixture();
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('1'),
            KeyCode::Char('4'),
        ] {
            app.handle_key(key_with_modifiers(code, modifiers));
        }
        assert_eq!(
            app.permission_modal_selection("perm_feedback"),
            PermissionModalSelection::AllowAlways
        );
        assert_eq!(
            app.permission_modal_stage("perm_feedback"),
            PermissionModalStage::Decision
        );
        assert!(intents.lock().unwrap_or_abort().is_empty());
    }
}

#[test]
fn permission_feedback_edits_graphemes_and_submits_one_deny_reason() {
    // Given the reject row and an isolated composer draft.
    let (mut app, intents) = permission_feedback_fixture();
    app.handle_key(key(KeyCode::Left));

    // When feedback is typed and edited, navigation letters and digits are text.
    for character in "先e\u{301}👩\u{200d}💻jk4".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Home));
    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::End));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
    ));

    // Then whole graphemes were edited and one reason crosses the guarded boundary.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_feedback".to_string(),
            decision: PermissionDecision::Deny,
            reason: Some("e\u{301}j4".to_string()),
            grant_scope: None,
        }]
    );
    assert!(app.permission_submission_pending("perm_feedback"));
    assert!(app.active_permission().is_some());
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
    assert_eq!(app.composer.prompt_cursor, 3);
}

#[test]
fn permission_feedback_escape_leaves_editor_before_parking_and_keeps_reason() {
    // Given feedback on the reject row.
    let (mut app, intents) = permission_feedback_fixture();
    app.handle_key(key(KeyCode::Left));
    for character in "change scope".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }

    // When Escape is used at each ownership layer, then focus is restored to submit.
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Prompt);
    assert!(intents.lock().unwrap_or_abort().is_empty());
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::List);
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(key(KeyCode::Enter));

    // Then leaving the editor neither denies early nor loses the eventual reason.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_feedback".to_string(),
            decision: PermissionDecision::Deny,
            reason: Some("change scope".to_string()),
            grant_scope: None,
        }]
    );
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
}

#[test]
fn permission_feedback_does_not_leak_through_resolution_or_history_replacement() {
    for replace_history in [false, true] {
        // Given an unfinished rejection draft.
        let (mut app, intents) = permission_feedback_fixture();
        app.handle_key(key(KeyCode::Left));
        app.handle_key(key(KeyCode::Char('x')));
        let next_id = if replace_history {
            "perm_feedback"
        } else {
            "perm_next"
        };

        // When authoritative state replaces the prompt, even reusing its identity.
        if replace_history {
            app.replace_events(vec![edit_permission_event(1, next_id, "tc_next")]);
        } else {
            app.ingest_event(edit_permission_event(2, next_id, "tc_next"));
            app.ingest_event(envelope(
                3,
                "req_feedback_done",
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_feedback".to_string(),
                    decision: harness_core::event::PermissionDecision::Deny,
                    reason: None,
                }),
            ));
        }
        app.handle_key(key(KeyCode::Left));
        app.handle_key(key(KeyCode::Enter));

        // Then the new rejection cannot inherit the previous prompt's feedback.
        assert_eq!(
            intents.lock().unwrap_or_abort().as_slice(),
            &[UiIntent::ResolvePermission {
                permission_id: next_id.to_string(),
                decision: PermissionDecision::Deny,
                reason: None,
                grant_scope: None,
            }]
        );
    }
}

#[test]
fn permission_feedback_render_keeps_cursor_near_long_unicode_input() {
    // Given feedback longer than the reject row in every tested terminal width.
    let (mut app, _) = permission_feedback_fixture();
    app.handle_key(key(KeyCode::Left));
    for character in format!("{}終e\u{301}", "界".repeat(160)).chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }

    // When the real renderer draws the focused row at narrow and wide sizes.
    for width in [40, 80, 140] {
        let rendered = render_text(&app, width, 30);

        // Then the tail at the caret remains visible, not clipped off to the right.
        assert!(rendered.contains("終"), "width {width}: {rendered}");
        assert!(rendered.contains("e\u{301}"), "width {width}: {rendered}");
    }
}
