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

    // act
    app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );
    // assert
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
