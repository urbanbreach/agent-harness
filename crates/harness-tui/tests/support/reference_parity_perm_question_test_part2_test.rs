#[test]
fn shell_question_wraps_inactive_descriptions_without_clipping() {
    // arrange
    // Given: two long options whose descriptions must wrap at 60 columns.
    let mut app = live_app();
    app.ingest_event(question_event_with_summary(
        1,
        "question_wrapped_inactive",
        "tool_question_wrapped_inactive",
        serde_json::json!({
            "questions": [{
                "question": "Pick a deployment target",
                "header": "Target",
                "options": [
                    {"label": "Primary", "description": "first description reaches the preserved ending ALPHA"},
                    {"label": "Secondary", "description": "inactive description reaches the preserved ending OMEGA"}
                ],
                "custom": false
            }]
        }),
    ));
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    // When: the full-screen overflow fallback renders with the first option selected.
    let rendered = render_at(&app, 60, 20);

    // act
    // Then: both active and inactive descriptions remain readable.
    // assert
    assert!(
        rendered.contains("ALPHA"),
        "active description clipped\n{rendered}"
    );
    assert!(
        rendered.contains("OMEGA"),
        "inactive description clipped instead of wrapping\n{rendered}"
    );
}

#[test]
fn shell_question_many_options_keeps_selected_row_and_scrollbar_reachable() {
    // arrange
    // Given: more wrapped options than the embedded question viewport can show.
    let options = (1..=12)
        .map(|index| {
            serde_json::json!({
                "label": format!("Option {index}"),
                "description": format!("long option description {index} with terminal-safe wrapping")
            })
        })
        .collect::<Vec<_>>();
    let mut app = live_app();
    app.ingest_event(question_event_with_summary(
        1,
        "question_many_options",
        "tool_question_many_options",
        serde_json::json!({
            "questions": [{
                "question": "Choose the final reachable option",
                "header": "Choice",
                "options": options,
                "custom": false
            }]
        }),
    ));

    // When: keyboard navigation reaches the last option.
    for _ in 1..12 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    let rendered = render_at(&app, 60, 20);

    // act
    // Then: allocation/render scrolling keeps the selected row and scrollbar visible.
    // assert
    assert!(
        rendered.contains("Option 12"),
        "selected row unreachable\n{rendered}"
    );
    assert!(
        rendered.contains('█'),
        "overflow scrollbar missing\n{rendered}"
    );
}

#[test]
fn shell_question_page_keys_keep_the_jump_target_reachable() {
    // arrange
    // Given: an overflowing question with twelve answer rows.
    let options = (1..=12)
        .map(|index| serde_json::json!({"label": format!("Choice {index}"), "description": "details"}))
        .collect::<Vec<_>>();
    let mut app = live_app();
    app.ingest_event(question_event_with_summary(
        1,
        "question_page_keys",
        "tool_question_page_keys",
        serde_json::json!({
            "questions": [{
                "question": "Page through all choices",
                "header": "Choice",
                "options": options,
                "custom": false
            }]
        }),
    ));

    // When: PageDown advances by five rows and PageUp reverses the jump.
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    let advanced = render_at(&app, 60, 20);
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let returned = render_at(&app, 60, 20);

    // act
    // Then: the jump target is visible and the selection returns to the first row.
    // assert
    assert!(
        advanced.contains("Choice 6"),
        "page target hidden\n{advanced}"
    );
    assert!(
        returned.contains("Choice 1"),
        "first row hidden\n{returned}"
    );
}

#[test]
fn shell_question_ctrl_f_uses_full_screen_fallback_for_overflow() {
    // arrange
    // Given: a long question that is capped in embedded mode.
    let options = (1..=14)
        .map(|index| serde_json::json!({"label": format!("Choice {index}"), "description": "wrapped details"}))
        .collect::<Vec<_>>();
    let mut app = live_app();
    app.ingest_event(question_event_with_summary(
        1,
        "question_fullscreen",
        "tool_question_fullscreen",
        serde_json::json!({
            "questions": [{
                "question": "Review every available answer before confirming",
                "header": "Choice",
                "options": options,
                "custom": true
            }]
        }),
    ));
    let embedded = plan_at(&app, 120, 40)
        .status
        .expect("embedded question status")
        .height;

    // When: Ctrl-F toggles the reference fullscreen fallback.
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    let fullscreen = plan_at(&app, 120, 40)
        .status
        .expect("fullscreen question status")
        .height;

    // act
    // Then: the cap is lifted while staying within the terminal.
    // assert
    assert!(fullscreen > embedded, "fullscreen fallback did not expand");
    assert!(fullscreen <= 40, "fullscreen overflowed the terminal");
}

#[test]
fn shell_question_confirm_with_many_prompts_keeps_footer_visible_at_60x20() {
    // arrange
    // Given: many prompt tabs and long answers in the final Confirm page.
    let questions = (1..=8)
        .map(|index| {
            serde_json::json!({
                "question": format!("Question {index}"),
                "header": format!("VeryLongPromptTab{index}"),
                "options": [{"label": format!("Answer {index}"), "description": "description"}],
                "custom": false
            })
        })
        .collect::<Vec<_>>();
    let mut app = live_app();
    app.ingest_event(question_event_with_summary(
        1,
        "question_many_confirm",
        "tool_question_many_confirm",
        serde_json::json!({"questions": questions}),
    ));
    for _ in 0..8 {
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    }

    // When: Confirm renders at the smallest measured viewport.
    let rendered = render_at(&app, 60, 20);

    // act
    // Then: the active long tab and sticky confirmation action survive compacting.
    // assert
    assert!(
        rendered.contains("VeryLongPromptTab8"),
        "active long tab clipped\n{rendered}"
    );
    assert!(
        rendered.contains("Enter:submit"),
        "sticky confirm footer clipped\n{rendered}"
    );
}

/// SHELL-PERM state machine: cycling moves the radio marker between options
/// (h/l or arrows), preserving the draft; the default marker starts on
/// always-approve and moves to session grant after one cycle-forward.
#[test]
fn shell_perm_selection_cycling_moves_option_marker() {
    // arrange
    let mut app = live_app();
    app.composer.prompt_buffer = "cycling draft".to_string();
    app.composer.prompt_cursor = "cycling draft".chars().count();
    app.ingest_event(permission_requested_event(
        1,
        "perm_cycle_parity",
        "tool_call_cycle",
    ));
    let initial = render(&app);

    // act
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let cycled = render(&app);

    // assert — marker moves option 1 -> option 2; draft preserved
    assert!(
        initial.contains("(●) Yes, and don't ask again for anything"),
        "SHELL-PERM: default marker on always-approve\n{initial}"
    );
    assert!(
        cycled.contains("(○) Yes, and don't ask again for anything"),
        "SHELL-PERM: marker leaves option 1 after cycle\n{cycled}"
    );
    assert!(
        cycled.contains("(●) Yes, allow all edits during this session"),
        "SHELL-PERM: marker lands on session grant after cycle\n{cycled}"
    );
    assert_eq!(app.composer.prompt_buffer, "cycling draft");
}

/// SHELL-PERM / OVL-PERM fail-closed recovery: Esc dismisses the dock as
/// Deny — DismissModal emits a ResolvePermission{Deny} intent to the
/// coordinator and keeps the draft; the modal view then waits for the
/// coordinator resolution event.
#[test]
fn shell_perm_esc_parks_without_answering_and_keeps_draft() {
    // arrange — live app with an intent sink; active permission dock + draft
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    let draft = "draft under esc-reject";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = Focus::Prompt;
    app.ingest_event(permission_requested_event(
        1,
        "perm_esc_parity",
        "tool_call_esc",
    ));
    assert!(
        app.active_permission().is_some(),
        "precondition: permission dock active before Esc"
    );

    // act — Esc parks the card so scrollback can take focus.
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // assert — the request remains unanswered and the draft remains intact.
    let emitted = intents.lock().unwrap_or_abort();
    assert!(emitted.is_empty(), "SHELL-PERM: Esc must not answer");
    assert_eq!(app.focus, Focus::List, "scrollback owns parked focus");
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "SHELL-PERM: draft preserved across Esc reject"
    );
}

/// SHELL-QUESTION / OVL-QUESTION answer submission: digit keys select an
/// option and Enter submits the answer as an Allow intent whose JSON reason
/// carries the selected option label.
#[test]
fn shell_question_digit_select_then_enter_submits_answer_intent() {
    // arrange — live app with intent sink; three-option question dock
    let (mut app, intents) = question_live_app_with_sink();
    app.ingest_event(three_option_question_event(
        1,
        "question_answer_parity",
        "tool_call_question_answer",
    ));
    assert!(
        app.active_permission().is_some(),
        "precondition: question dock active"
    );

    // act — digit '2' selects option B, Enter activates/submits the answer
    app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // assert — Allow intent submitted with the selected option in the reason
    let emitted = intents.lock().unwrap_or_abort();
    let answer = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            reason,
            ..
        } if permission_id == "question_answer_parity"
            && *decision == PermissionDecision::Allow =>
        {
            reason.clone()
        }
        _ => None,
    });
    let answer = answer.expect("SHELL-QUESTION: Enter must submit an Allow answer intent");
    assert!(
        answer.contains("\"B\""),
        "SHELL-QUESTION: answer reason must carry option B: {answer}"
    );
}

/// SHELL-QUESTION / OVL-QUESTION fail-closed cancel: Esc dismisses the
/// question dock as Deny (ResolvePermission{Deny} intent emitted).
#[test]
fn shell_question_ctrl_c_cancels_with_fail_closed_deny_intent() {
    // arrange — live app with intent sink; question dock with draft
    let (mut app, intents) = question_live_app_with_sink();
    let draft = "draft under question cancel";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = Focus::Prompt;
    app.ingest_event(three_option_question_event(
        1,
        "question_cancel_parity",
        "tool_call_question_cancel",
    ));

    // act — Ctrl-C is the explicit fail-closed cancel chord.
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    // assert — Deny intent emitted; draft preserved
    let emitted = intents.lock().unwrap_or_abort();
    let deny = emitted.iter().find_map(|intent| match intent {
        UiIntent::ResolvePermission {
            permission_id,
            decision,
            ..
        } if permission_id == "question_cancel_parity" => Some(*decision),
        _ => None,
    });
    assert_eq!(
        deny,
        Some(PermissionDecision::Deny),
        "SHELL-QUESTION: Esc must resolve as Deny (fail-closed)"
    );
    assert_eq!(app.composer.prompt_buffer, draft);
}

/// SHELL-QUESTION / OVL-QUESTION state machine: Tab switches the active prompt
/// in a multi-question dock (question 1 -> question 2 visible in the dock).
#[test]
fn shell_question_horizontal_navigation_switches_active_prompt() {
    // arrange — two-question dock (multi-tab)
    let (mut app, _intents) = question_live_app_with_sink();
    app.ingest_event(multi_question_event(
        1,
        "question_tabs_parity",
        "tool_call_question_tabs",
    ));
    let initial = render(&app);

    // act — Right moves to the second question prompt
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let after_tab = render(&app);

    // assert — dock switches from question 1 to question 2
    assert!(
        initial.contains("First parity question"),
        "SHELL-QUESTION: first prompt visible on tab 0\n{initial}"
    );
    assert!(
        after_tab.contains("Second parity question"),
        "SHELL-QUESTION: second prompt visible after Tab\n{after_tab}"
    );
    assert!(
        !after_tab.contains("First parity question"),
        "SHELL-QUESTION: first prompt hidden after Tab switches to tab 1\n{after_tab}"
    );
}
