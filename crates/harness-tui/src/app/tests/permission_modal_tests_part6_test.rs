#[test]
fn permission_feedback_paste_inserts_literal_single_line_text_at_the_cursor() {
    // Given a focused reject choice and an isolated composer draft.
    let (mut app, intents) = permission_feedback_fixture();
    app.handle_key(key(KeyCode::Left));

    // When paste starts feedback, shortcut characters are literal text.
    app.handle_paste("jk14👩💻!");
    assert_eq!(
        app.permission_feedback("perm_feedback")
            .and_then(|feedback| feedback.reason()),
        Some("jk14👩💻!".to_string())
    );
    app.handle_key(key(KeyCode::Home));
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Right));
    }
    app.new_worktree_dialog.visible = true;
    app.new_worktree_dialog.input = "worktree".to_string();
    app.new_worktree_dialog.cursor = 8;
    app.handle_paste(
        "\u{200d}終e\u{301}\u{1b}[31mZ\u{1b}[0m\r\n\t尾\u{2028}a\u{2029}b\u{85}c\u{0}\u{7}\u{7f}\u{1b}]52;c;discarded\u{7}",
    );
    app.handle_key(key(KeyCode::Char('?')));

    // Then bulk insertion preserves grapheme order without answering or leaking input.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
    assert_eq!(app.composer.prompt_cursor, 3);
    assert_eq!(app.new_worktree_dialog.input, "worktree");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_feedback".to_string(),
            decision: PermissionDecision::Deny,
            reason: Some("jk14👩\u{200d}終e\u{301}Z  尾 a b c?💻!".to_string()),
            grant_scope: None,
        }]
    );
}

#[test]
fn permission_feedback_paste_respects_input_ownership_and_submission_guards() {
    #[derive(Debug)]
    enum Boundary {
        OtherChoice,
        Confirmation,
        Parked,
        Submitted,
        AuthDialog,
        TrustPrompt,
        Question,
        Replay,
    }

    for boundary in [
        Boundary::OtherChoice,
        Boundary::Confirmation,
        Boundary::Parked,
        Boundary::Submitted,
        Boundary::AuthDialog,
        Boundary::TrustPrompt,
        Boundary::Question,
        Boundary::Replay,
    ] {
        // Given saved feedback with an input owner that cannot accept rejection text.
        let (mut app, intents) = permission_feedback_fixture();
        app.handle_key(key(KeyCode::Left));
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_key(key(KeyCode::Esc));
        match boundary {
            Boundary::OtherChoice => app.handle_key(key(KeyCode::Right)),
            Boundary::Confirmation => app.handle_key(key(KeyCode::Char('1'))),
            Boundary::Parked => app.handle_key(key(KeyCode::Esc)),
            Boundary::Submitted => app.handle_key(key(KeyCode::Enter)),
            Boundary::AuthDialog => app.connect_dialog.visible = true,
            Boundary::TrustPrompt => app.trust_folder_prompt_visible = true,
            Boundary::Question => {
                app.replace_events(vec![custom_question_event("perm_feedback", false)]);
                app.handle_key(key(KeyCode::BackTab));
                app.handle_key(key(KeyCode::Enter));
                app.handle_key(key(KeyCode::Char('v')));
            }
            Boundary::Replay => app.replay_mode = true,
        }
        let feedback_before = app
            .permission_prompt
            .feedback
            .as_ref()
            .and_then(|feedback| feedback.reason());
        let question_before = app.question_prompt.answer_buffer.clone();
        let focus_before = app.focus;
        let intents_before = intents.lock().unwrap_or_abort().clone();

        // When a paste containing both decision and navigation shortcuts arrives.
        app.handle_paste("1234jk\nignored");

        // Then neither feedback, other inputs, focus, nor pending decisions change.
        assert_eq!(
            app.permission_prompt
                .feedback
                .as_ref()
                .and_then(|feedback| feedback.reason()),
            feedback_before,
            "{boundary:?}"
        );
        assert_eq!(app.question_prompt.answer_buffer, question_before, "{boundary:?}");
        assert_eq!(app.focus, focus_before, "{boundary:?}");
        assert_eq!(app.composer.prompt_buffer, "preserved draft", "{boundary:?}");
        assert_eq!(app.composer.prompt_cursor, 3, "{boundary:?}");
        assert_eq!(
            intents.lock().unwrap_or_abort().as_slice(),
            intents_before.as_slice(),
            "{boundary:?}"
        );
    }
}

#[test]
fn permission_feedback_paste_with_no_safe_text_does_not_enter_editing() {
    for text in ["", "\u{0}\u{7}\u{7f}\u{1b}[31m\u{1b}]52;c;discarded\u{7}"] {
        // Given a reject choice that has not entered feedback editing.
        let (mut app, intents) = permission_feedback_fixture();
        app.handle_key(key(KeyCode::Left));

        // When nothing remains after paste sanitization.
        app.handle_paste(text);

        // Then no editor is opened and the next numbered choice still denies.
        assert!(app.permission_feedback("perm_feedback").is_none());
        app.handle_key(key(KeyCode::Char('4')));
        assert_eq!(
            intents.lock().unwrap_or_abort().as_slice(),
            &[UiIntent::ResolvePermission {
                permission_id: "perm_feedback".to_string(),
                decision: PermissionDecision::Deny,
                reason: None,
                grant_scope: None,
            }]
        );
    }
}
