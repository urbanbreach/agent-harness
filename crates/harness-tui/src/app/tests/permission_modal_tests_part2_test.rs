pub(super) fn permission_modal_allow_always_requests_coordinator_mode_change() {
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
        !app.always_approve_mode(),
        "the UI must wait for the coordinator acknowledgement"
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::SetAlwaysApproveMode { enabled: true }]
    );

    app.set_always_approve_mode(true);
    assert!(app.always_approve_mode());
}

pub(super) fn always_approve_mode_does_not_late_resolve_projected_permissions() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.set_always_approve_mode(true);

    app.ingest_event(envelope(
        1,
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

    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "projecting an event must never emit a late permission resolution"
    );
}

pub(super) fn pending_always_approve_enable_suppresses_only_ordinary_permission_ui() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.request_always_approve_mode_change(true);

    app.ingest_event(envelope(
        1,
        "req_pending_enable_shell",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_pending_enable_shell".to_string(),
            kind: "shell".to_string(),
            tool_call_id: Some("tc_pending_enable_shell".into()),
            summary: "ordinary shell permission".to_string(),
            request_digest: "digest-pending-enable-shell".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    assert!(app.active_permission_view().is_none());
    assert!(app.transcript_pending_permissions().is_empty());
    assert!(!app.always_approve_mode());

    app.ingest_event(envelope(
        2,
        "req_pending_enable_read",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_pending_enable_read".to_string(),
            kind: "read".to_string(),
            tool_call_id: Some("tc_pending_enable_read".into()),
            summary: "potentially sensitive read permission".to_string(),
            request_digest: "digest-pending-enable-read".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    assert_eq!(
        app.active_permission_view()
            .map(|permission| permission.permission_id),
        Some("perm_pending_enable_read".to_string())
    );
}

pub(super) fn failed_always_approve_enable_restores_suppressed_permission_ui() {
    let mut app = AppState::new_live(None, false, None);
    app.request_always_approve_mode_change(true);
    app.ingest_event(envelope(
        1,
        "req_failed_enable_shell",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_failed_enable_shell".to_string(),
            kind: "shell".to_string(),
            tool_call_id: Some("tc_failed_enable_shell".into()),
            summary: "ordinary shell permission".to_string(),
            request_digest: "digest-failed-enable-shell".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    assert!(app.active_permission_view().is_none());

    app.reject_always_approve_mode_change();

    assert_eq!(
        app.active_permission_view()
            .map(|permission| permission.permission_id),
        Some("perm_failed_enable_shell".to_string())
    );
}

pub(super) fn always_approve_mode_appends_composer_badge_suffix() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(crate::app::LaunchMetadata::new(
        "build",
        "test-provider",
        Some("model-tx".to_string()),
    ));
    app.set_always_approve_mode(true);
    assert!(app.always_approve_mode());
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

fn compact_chrome_question_event(permission_id: &str) -> EventEnvelopeV1 {
    envelope(
        1,
        permission_id,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_compact_chrome".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one\n\nd1\nd2\nd3\nd4",
                    "header": "Choice",
                    "options": [
                        {"label": "Choice 1", "description": ""},
                        {"label": "Choice 2", "description": ""},
                        {"label": "Choice 3", "description": ""}
                    ],
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
    // Given: the four-choice permission dock at the primary consistency viewport.
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
    // Given: a three-choice question dock at the primary consistency viewport.
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
            (
                PermissionPointerTarget::QuestionSubmit,
                Rect::new(104, 29, 12, 1),
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
        "card submit action hidden\n{rendered}"
    );
    assert!(!rendered.contains("Ctrl+F expand"), "{rendered}");
    assert!(
        rendered.contains("X:dismiss"),
        "compact outer footer must name the question cancellation action\n{rendered}"
    );
}

#[test]
fn question_compact_chrome_truncation_keeps_options_in_the_option_viewport() {
    // arrange — Given four description rows exceed the compact question chrome budget.
    let frame_area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(compact_chrome_question_event("question_compact_chrome"));

    // act — When the compact dock and pointer map are rendered from the same measurement.
    let rendered = render_text(&app, frame_area.width, frame_area.height);
    let choice_regions = app
        .permission_prompt_hit_regions_for_test(frame_area)
        .into_iter()
        .filter(|(target, _)| matches!(target, PermissionPointerTarget::QuestionChoice(_)))
        .count();

    // assert — Then hidden chrome is replaced by an affordance and never painted as choices.
    assert!(rendered.contains("... Ctrl-F to expand"), "{rendered}");
    assert!(rendered.contains("Choice 1"), "{rendered}");
    assert!(rendered.contains("Choice 2"), "{rendered}");
    assert!(rendered.contains("Choice 3"), "{rendered}");
    assert_eq!(choice_regions, 4);
}

#[test]
fn question_compact_footer_names_active_row_walk_park_and_cancel_actions() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(three_choice_question_event("question_compact_footer"));

    // act
    let rendered = render_text(&app, 60, 20);

    // assert
    assert!(rendered.contains("Esc:scrollback"), "{rendered}");
    assert!(rendered.contains("Enter:submit"), "{rendered}");
    assert!(!rendered.contains("Ctrl+F expand"), "{rendered}");
    assert!(rendered.contains("Tab:next answer"), "{rendered}");
    assert!(rendered.contains("X:dismiss"), "{rendered}");
    assert!(!rendered.contains("Esc:back"), "{rendered}");
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
    assert!(!app.always_approve_mode());
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::SetAlwaysApproveMode { enabled: true }]
    );
}
