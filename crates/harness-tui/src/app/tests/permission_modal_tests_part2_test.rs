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

fn overflowing_question_event(permission_id: &str) -> EventEnvelopeV1 {
    let options = (1..=16)
        .map(|index| {
            serde_json::json!({
                "label": format!("Choice {index}"),
                "description": format!("wrapped description for choice {index}")
            })
        })
        .collect::<Vec<_>>();
    envelope(
        1,
        permission_id,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_overflow".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Which overflowing choice?",
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
    // arrange
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
                Rect::new(5, 29, 111, 1),
            ),
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::AllowSession),
                Rect::new(5, 30, 111, 1),
            ),
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::AllowOnce),
                Rect::new(5, 31, 111, 1),
            ),
            (
                PermissionPointerTarget::Decision(PermissionModalSelection::Reject),
                Rect::new(5, 32, 111, 1),
            ),
        ]
    );

    // act
    app.handle_key(key(KeyCode::Enter));
    // assert
    assert_eq!(
        app.permission_prompt_hit_regions_for_test(frame_area),
        vec![
            (
                PermissionPointerTarget::Confirm(PermissionConfirmSelection::Confirm),
                Rect::new(5, 31, 11, 1),
            ),
            (
                PermissionPointerTarget::Confirm(PermissionConfirmSelection::Cancel),
                Rect::new(17, 31, 10, 1),
            ),
        ]
    );
}

#[test]
fn question_mouse_hit_regions_match_the_rendered_option_rows() {
    // arrange
    // Given: a three-choice question dock at the primary parity viewport.
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(three_choice_question_event("question_mouse_regions"));

    // When: the pointer map is derived from question content packing.
    let regions = app.permission_prompt_hit_regions_for_test(frame_area);

    // act
    // Then: each painted question row has one full-width deterministic target.
    // assert
    assert_eq!(
        regions,
        vec![
            (
                PermissionPointerTarget::QuestionChoice(0),
                Rect::new(5, 25, 111, 1),
            ),
            (
                PermissionPointerTarget::QuestionChoice(1),
                Rect::new(5, 26, 111, 1),
            ),
            (
                PermissionPointerTarget::QuestionChoice(2),
                Rect::new(5, 27, 111, 1),
            ),
        ]
    );
}

#[test]
fn question_mouse_wheel_scrolls_the_shared_option_viewport() {
    // arrange
    // Given: an overflowing question whose hit map includes the painted scrollbar.
    let frame_area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(overflowing_question_event("question_mouse_scroll"));
    let scrollbar = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionScrollbar).then_some(area)
        })
        .unwrap_or_abort();

    // When: the mouse wheel moves down over the shared scroll track.
    let changed = app.handle_mouse(
        mouse_event(MouseEventKind::ScrollDown, scrollbar),
        frame_area,
        None,
        None,
        None,
    );

    // act
    // Then: the question state records one row of explicit scroll.
    // assert
    assert!(changed);
    assert_eq!(app.question_prompt_scroll("question_mouse_scroll", 0), 1);
}

#[test]
fn question_overflow_keeps_custom_error_and_footer_sticky_at_60x20() {
    // arrange
    // Given: a compact overflowing question with custom input and a wrapped validation error.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(overflowing_question_event("question_sticky_rows"));
    app.handle_key(key(KeyCode::Null));
    app.question_prompt.answer_error = Some(
        "The selected answer is no longer available; choose another answer before submitting."
            .to_string(),
    );

    // When: the real question dock renders at the compact reference viewport.
    let rendered = render_text(&app, 60, 20);

    // act
    // Then: fallback allocation preserves options plus custom, error, and the dock footer.
    // assert
    assert!(
        rendered.contains("Choice 1"),
        "selected row hidden\n{rendered}"
    );
    assert!(
        rendered.contains("Type your answer here"),
        "custom row hidden\n{rendered}"
    );
    assert!(
        rendered.contains("before submitting."),
        "validation error clipped\n{rendered}"
    );
    assert!(
        rendered.contains("Enter:submit"),
        "dock footer hidden\n{rendered}"
    );
    assert!(
        rendered.contains("⇧X:cancel"),
        "compact outer footer must name the question cancellation action\n{rendered}"
    );
}

#[test]
fn question_compact_footer_names_active_row_walk_park_and_cancel_actions() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(three_choice_question_event("question_compact_footer"));

    // act
    let rendered = render_text(&app, 60, 20);

    // assert
    assert!(rendered.contains("Esc:park"), "{rendered}");
    assert!(rendered.contains("Tab:next"), "{rendered}");
    assert!(rendered.contains("⇧Tab:prev"), "{rendered}");
    assert!(rendered.contains("⇧X:cancel"), "{rendered}");
    assert!(!rendered.contains("Esc:back"), "{rendered}");
    assert!(!rendered.contains("⇧X:dismiss"), "{rendered}");
}

#[test]
fn question_mouse_hover_paints_row_without_moving_keyboard_selection() {
    // arrange
    // Given: a focused question whose keyboard cursor is on the first choice.
    let frame_area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.ingest_event(three_choice_question_event("question_mouse_hover"));
    let option_area = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .find_map(|(target, area)| {
            (target == PermissionPointerTarget::QuestionChoice(1)).then_some(area)
        })
        .unwrap_or_abort();

    // When: the pointer moves over the second choice.
    let changed = app.handle_mouse(
        mouse_event(MouseEventKind::Moved, option_area),
        frame_area,
        None,
        None,
        None,
    );
    let mut terminal =
        Terminal::new(TestBackend::new(frame_area.width, frame_area.height)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();

    // act
    // Then: hover owns its own fill while keyboard selection remains unchanged.
    // assert
    assert!(changed);
    assert_eq!(app.question_prompt_selection("question_mouse_hover"), 0);
    assert_eq!(
        terminal.backend().buffer()[(option_area.x, option_area.y)].bg,
        Color::Rgb(44, 44, 44)
    );
}

#[test]
fn permission_mouse_click_selects_before_emitting_only_a_resolution_intent() {
    // arrange
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

    // act
    let released = app.handle_mouse(
        mouse_event(MouseEventKind::Up(MouseButton::Left), option_area),
        frame_area,
        None,
        None,
        None,
    );
    // assert
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
    // arrange
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

    // act
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

    // assert
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
