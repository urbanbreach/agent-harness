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

pub(super) fn permission_modal_escape_parks_and_tab_restores_without_answering() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.composer.prompt_buffer = "preserved draft".to_string();
    app.ingest_event(provider_started(1, "review", "mock", "mock"));
    app.ingest_event(envelope(
        2,
        "review",
        EventV1::ProviderReasoningDelta(harness_core::event::ProviderReasoningDeltaEvent {
            request_id: "review".into(),
            delta: "Review the proposed change carefully.".into(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "review",
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "review".into(),
            delta: (0..80).fold(String::new(), |mut text, row| {
                use std::fmt::Write as _;
                writeln!(text, "Transcript review line {row}\n").unwrap_or_abort();
                text
            }),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "review",
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: "review".into(),
            finish_reason: "stop".into(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        5,
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

    let area = Rect::new(0, 0, 120, 40);
    app.set_frame_area(area);
    let screen = render_text(&app, area.width, area.height);
    assert!(screen.contains("Ctrl+e:collapse thinking"), "{screen}");
    assert!(!screen.contains("Ctrl+x:shortcuts"), "{screen}");
    app.handle_key(key_with_modifiers(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL,
    ));
    assert!(!app.transcript_thinking_visible());
    app.handle_key(key(KeyCode::PageUp));
    assert!(app.transcript_scroll_offset() > 0);
    assert!(!app.transcript_view.follow_mode);
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.transcript_scroll_offset(), 0);
    let transcript = crate::layout::FrameLayoutPlan::for_app(&app, area)
        .transcript
        .unwrap_or_abort();
    assert!(app.handle_mouse(
        mouse_event(MouseEventKind::ScrollUp, transcript),
        area,
        None,
        None,
        None
    ));
    assert!(app.transcript_scroll_offset() > 0);
    let parked_scroll = app.transcript_scroll_offset();
    for key_event in [
        key(KeyCode::Enter),
        key(KeyCode::Char('a')),
        key_with_modifiers(KeyCode::Char('x'), KeyModifiers::CONTROL),
        key_with_modifiers(KeyCode::Char('p'), KeyModifiers::CONTROL),
    ] {
        app.handle_key(key_event);
    }
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
    assert_eq!(app.transcript_scroll_offset(), parked_scroll);
    assert!(app.active_permission().is_some());
    assert!(!app.palette_visible);
    assert!(app.active_review_surface.is_none());
    assert!(intents.lock().unwrap_or_abort().is_empty());
    capture_parked_permission_review(&mut app);
    assert_parked_permission_focus_restores(&mut app);
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

fn assert_parked_permission_focus_restores(app: &mut AppState) {
    for restore_key in [KeyCode::Tab, KeyCode::Char(' ')] {
        app.handle_key(key(restore_key));
        assert_eq!(app.focus, Focus::Prompt);
        app.handle_key(key(KeyCode::Esc));
        assert_eq!(app.focus, Focus::List);
    }
    app.handle_key(key(KeyCode::Tab));

    assert_eq!(app.focus, Focus::Prompt);
}

fn capture_parked_permission_review(app: &mut AppState) {
    if let Some(directory) = std::env::var_os("HARNESS_TOOL_RUNTIME_HARNESS_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap_or_abort();
        for (width, height) in [(120, 40), (60, 20)] {
            let (bytes, _) =
                super::tool_runtime_capture_tests::draw(app, Rect::new(0, 0, width, height))
                    .unwrap_or_abort();
            std::fs::write(
                directory.join(format!(
                    "permission-parked-review-{width}x{height}-motion-0ms.ansi"
                )),
                bytes,
            )
            .unwrap_or_abort();
        }
    }
}
