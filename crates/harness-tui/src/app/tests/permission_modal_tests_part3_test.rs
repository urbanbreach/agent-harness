fn custom_question_event(permission_id: &str, multiple: bool) -> EventEnvelopeV1 {
    envelope(
        1,
        permission_id,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(format!("tool_{permission_id}").into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                    "multiple": multiple,
                    "custom": true
                }]
            })
            .to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn shortcut_question_event(permission_id: &str) -> EventEnvelopeV1 {
    let options = ('A'..='J')
        .map(|label| serde_json::json!({"label": label.to_string(), "description": ""}))
        .collect::<Vec<_>>();
    envelope(
        1,
        permission_id,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(format!("tool_{permission_id}").into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": options,
                    "multiple": false,
                    "custom": true
                }]
            })
            .to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

#[test]
fn question_mouse_click_preserves_shell_state_and_emits_only_answer_intent() {
    // arrange
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

    // When: the B row is clicked once.
    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: pointer-down selects the answer but emits no permission decision.
    assert_eq!(app.question_prompt_selection("question_mouse_select"), 1);
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(
        app.question_prompt_answers("question_mouse_select"),
        vec![vec!["B".to_string()]]
    );

    // When: the first click is released.
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: release does not toggle the answer a second time.
    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(
        app.question_prompt_answers("question_mouse_select"),
        vec![vec!["B".to_string()]]
    );

    // When: the selected row is clicked a second time.
    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: the second pointer-down submits the selected answer.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "question_mouse_select".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"B\"]]".to_string()),
            grant_scope: None,
        }]
    );
    assert_eq!(app.focus, Focus::Prompt);
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

#[test]
fn question_single_select_space_replaces_the_previous_option() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(three_choice_question_event("question_single_replace"));

    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Char(' ')));

    assert_eq!(
        app.question_prompt_answers("question_single_replace"),
        vec![vec!["B".to_string()]]
    );
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(intents.lock().unwrap_or_abort().len(), 1);
}

#[test]
fn question_ctrl_c_cancels_the_question() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(three_choice_question_event("question_empty_ctrl_c"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "question_empty_ctrl_c".to_string(),
            decision: PermissionDecision::Deny,
            reason: None,
            grant_scope: None,
        }]
    );
}

#[test]
fn question_ctrl_y_hides_the_question_without_resolving_it() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(three_choice_question_event("question_ctrl_y_dismiss"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    ));

    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert!(app.active_permission_view().is_none());
}

#[test]
fn question_cancelled_custom_edit_keeps_custom_selected() {
    // Given: a selected fixed answer and an uncommitted custom edit.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_cancel_custom", false));
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('n')));

    // When: custom editing is cancelled.
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    // Then: entering freeform owns the selection even though the draft was not committed.
    assert_eq!(
        app.question_prompt_answers("question_cancel_custom"),
        vec![Vec::<String>::new()]
    );
    assert!(app.question_prompt_custom_selected("question_cancel_custom", 0));
}

#[test]
fn question_space_on_selected_custom_reopens_input() {
    // Given: a committed custom answer with the custom row still focused.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_reopen_custom", false));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('n')));
    app.handle_key(key(KeyCode::Esc));

    // When: Space activates the selected custom row.
    app.handle_key(key(KeyCode::Char(' ')));

    // Then: the saved text is reopened for editing rather than deselected.
    assert!(app.question_prompt_editing("question_reopen_custom"));
    assert!(app.question_prompt_custom_selected("question_reopen_custom", 0));
}

#[test]
fn question_shift_x_is_text_while_custom_input_is_active() {
    // Given: active custom input.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_custom_x", false));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));

    // When: uppercase X is typed.
    app.handle_key(key_with_modifiers(KeyCode::Char('X'), KeyModifiers::SHIFT));

    // Then: X belongs to the draft instead of dismissing the question.
    assert!(app.question_prompt_editing("question_custom_x"));
    assert_eq!(app.question_prompt.answer_buffer, "X");
}

#[test]
fn question_blank_custom_enter_submits_an_empty_response() {
    // Given: the custom row is being edited with an empty buffer.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(custom_question_event("question_blank_custom", false));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));

    // When: the blank custom answer is committed.
    app.handle_key(key(KeyCode::Enter));

    // Then: the question is submitted with an accepted empty answer list.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "question_blank_custom".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[]]".to_string()),
            grant_scope: None,
        }]
    );
}

#[test]
fn question_multi_select_keeps_custom_answer_when_fixed_option_toggles() {
    // Given: a multi-select question with a committed custom answer.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_multi_custom", true));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));
    for character in "note".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Esc));

    // When: a fixed option is toggled on.
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Char(' ')));

    // Then: both answers remain visibly selected and submit together.
    assert!(app.question_prompt_custom_selected("question_multi_custom", 0));
    assert_eq!(
        app.question_prompt_answers("question_multi_custom"),
        vec![vec!["A".to_string(), "note".to_string()]]
    );
}

#[test]
fn question_multi_select_keeps_equal_fixed_and_custom_answers_distinct() {
    // Given: a fixed option A and a custom answer with the same text.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_equal_custom", true));
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('A')));
    app.handle_key(key(KeyCode::Esc));

    // When: the custom answer is reopened and committed empty.
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Esc));

    // Then: the independently selected fixed option remains.
    assert_eq!(
        app.question_prompt_answers("question_equal_custom"),
        vec![vec!["A".to_string()]]
    );
}

#[test]
fn question_tab_and_fullscreen_round_trip_preserves_scroll_offsets() {
    // Given: two questions with explicit per-tab scroll offsets.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "question_scroll_round_trip",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "question_scroll_round_trip".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_question_scroll_round_trip".into()),
            summary: serde_json::json!({
                "questions": [
                    {"question": "First", "header": "First", "options": [{"label": "A", "description": "A"}]},
                    {"question": "Second", "header": "Second", "options": [{"label": "B", "description": "B"}]}
                ]
            }).to_string(),
            request_digest: "digest-question-scroll-round-trip".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    app.handle_key(key(KeyCode::Down));
    app.question_prompt.scroll_offsets[0] = 3;
    app.question_prompt.scroll_offsets[1] = 5;

    // When: tabs and fullscreen are round-tripped.
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key_with_modifiers(KeyCode::Char('f'), KeyModifiers::CONTROL));
    app.handle_key(key_with_modifiers(KeyCode::Char('f'), KeyModifiers::CONTROL));

    // Then: both explicit offsets remain intact.
    assert_eq!(app.question_prompt.scroll_offsets, vec![3, 5]);
}

#[test]
fn question_clearing_custom_selection_preserves_its_text() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "question_custom_preserved",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "question_custom_preserved".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_question_custom_preserved".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Which color?",
                    "header": "Color",
                    "options": [{"label": "A", "description": "Option A"}],
                    "multiple": false,
                    "custom": true
                }]
            })
            .to_string(),
            request_digest: "digest-question-custom-preserved".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Char('n')));
    app.handle_key(key(KeyCode::Char('o')));
    app.handle_key(key(KeyCode::Char('t')));
    app.handle_key(key(KeyCode::Char('e')));
    app.handle_key(key(KeyCode::Esc));

    assert_eq!(
        app.question_prompt_custom("question_custom_preserved", 0),
        Some("note")
    );
    app.handle_key(key(KeyCode::Esc));

    assert!(!app.question_prompt_custom_selected("question_custom_preserved", 0));
    assert_eq!(
        app.question_prompt_custom("question_custom_preserved", 0),
        Some("note")
    );
}

#[test]
fn question_double_click_history_does_not_cross_question_tabs() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(envelope(
        1,
        "question_cross_tab_click",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "question_cross_tab_click".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_question_cross_tab_click".into()),
            summary: serde_json::json!({
                "questions": [
                    {"question": "First", "header": "First", "options": [{"label": "A", "description": "A"}]},
                    {"question": "Second", "header": "Second", "options": [{"label": "B", "description": "B"}]}
                ]
            }).to_string(),
            request_digest: "digest-question-cross-tab-click".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    let first_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionChoice(0)).then_some(area)
        })
        .unwrap_or_abort();
    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), first_area),
        frame_area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), first_area),
        frame_area,
        None,
        None,
        None,
    );
    app.handle_key(key(KeyCode::Right));
    let second_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionChoice(0)).then_some(area)
        })
        .unwrap_or_abort();

    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), second_area),
        frame_area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), second_area),
        frame_area,
        None,
        None,
        None,
    );

    assert!(intents.lock().unwrap_or_abort().is_empty());
    assert_eq!(
        app.question_prompt_answers("question_cross_tab_click"),
        vec![vec!["A".to_string()], vec!["B".to_string()]]
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
    // arrange
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

    // act
    // Then: the tool remains paused until coordinator-owned events advance it.
    // assert
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

#[test]
fn question_custom_answer_preserves_surrounding_whitespace() {
    // Given: custom input containing intentional surrounding spaces.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_whitespace", false));
    app.handle_key(key(KeyCode::BackTab));
    app.handle_key(key(KeyCode::Enter));
    for character in "  note  ".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }

    // When: editing is committed without submitting the permission.
    app.handle_key(key(KeyCode::Esc));

    // Then: trimming is used only for emptiness, not stored answer content.
    assert_eq!(
        app.question_prompt_answers("question_whitespace"),
        vec![vec!["  note  ".to_string()]]
    );
}

#[test]
fn question_fixed_shortcut_wins_over_custom_type_to_edit() {
    // Given: the custom row is selected below ten advertised fixed options.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(shortcut_question_event("question_shortcut_precedence"));
    app.handle_key(key(KeyCode::BackTab));

    // When: the advertised `a` shortcut is pressed.
    app.handle_key(key(KeyCode::Char('a')));

    // Then: option ten is selected instead of opening custom input with `a`.
    assert!(!app.question_prompt_editing("question_shortcut_precedence"));
    assert_eq!(
        app.question_prompt_answers("question_shortcut_precedence"),
        vec![vec!["J".to_string()]]
    );
}

#[test]
fn question_submit_footer_click_submits_the_selected_option() {
    // Given: a question with a painted Enter action and a permission intent sink.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(three_choice_question_event("question_submit_click"));
    let submit_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionSubmit).then_some(area)
        })
        .unwrap_or_abort();

    // When: the Enter action receives a complete click.
    app.handle_mouse(
        mouse_event(MouseEventKind::Down(MouseButton::Left), submit_area),
        frame_area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), submit_area),
        frame_area,
        None,
        None,
        None,
    );

    // Then: the currently focused option is submitted exactly once.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "question_submit_click".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("[[\"A\"]]".to_string()),
            grant_scope: None,
        }]
    );
}

#[test]
fn replace_events_resets_question_identity_before_reusing_permission_id() {
    // Given: initialized prompt state for a pending question.
    let event = custom_question_event("question_reused_id", false);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event.clone());
    app.handle_key(key(KeyCode::Down));

    // When: replay replacement restores a question with the same permission id.
    app.replace_events(vec![event]);
    app.handle_key(key(KeyCode::Enter));

    // Then: state is reinitialized rather than indexing stale empty vectors.
    assert_eq!(
        app.question_prompt_answers("question_reused_id"),
        vec![vec!["A".to_string()]]
    );
}

#[test]
fn new_session_resets_question_identity_before_reusing_permission_id() {
    // Given: a locally hidden question whose prompt state has been initialized.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(custom_question_event("question_new_reused_id", false));
    app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));

    // When: a new session receives a question with the same permission id.
    app.execute_slash_command("new", None);
    app.ingest_event(custom_question_event("question_new_reused_id", false));
    app.handle_key(key(KeyCode::Enter));

    // Then: prompt vectors are reinitialized and the default option submits normally.
    assert_eq!(
        app.question_prompt_answers("question_new_reused_id"),
        vec![vec!["A".to_string()]]
    );
}
