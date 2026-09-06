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
