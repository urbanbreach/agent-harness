use super::*;
use crate::UnwrapOrAbort;

pub(super) fn permission_modal_snapshot_renders_request() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    assert_buffer_snapshot(
        "permission_modal_snapshot_renders_request",
        terminal.backend().buffer(),
    );
}

pub(super) fn permission_dock_packs_measured_content_rows() {
    // Given: a decision-stage permission dock with measured detail content.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(permission_requested_event(
        1,
        "perm_pack_v2",
        "tool_call_pack_v2",
    ));

    // When: rendering the live shell at the consistency geometry.
    let rendered = render_live_lines(&app, 120, 32);
    let lines: Vec<&str> = rendered.lines().collect();
    let dock_start = lines
        .iter()
        .position(|line| line.contains("Allow Edit"))
        .expect("Allow Edit title must render");
    let option_start = lines
        .iter()
        .position(|line| line.contains("1 (●)") || line.contains("1 (○)"))
        .expect("option 1 must render");
    let option_end = lines
        .iter()
        .rposition(|line| line.contains("4 (○)") || line.contains("4 (●)"))
        .expect("option 4 must render");
    let footer = lines
        .iter()
        .position(|line| line.contains("1/4:select"))
        .expect("1/4:select footer must render");

    // Then: title, detail, gap, options, and footer occupy only their measured rows.
    assert!(
        dock_start > 0 && lines[dock_start - 1].contains('┃'),
        "the dock keeps one leading rail row above the title\n{rendered}"
    );
    assert!(
        lines[dock_start + 1].contains("Apply hashline edit to demo.txt")
            && option_start == dock_start + 3,
        "the measured detail and one gap row precede the options\n{rendered}"
    );
    assert!(
        footer == option_end + 1,
        "the footer immediately follows the options without fixed blank rows\n{rendered}"
    );
    assert!(
        rendered.contains("Ctrl+o:always-approve") && rendered.contains("Ctrl+c:cancel"),
        "4-option product keybind packing must stay closed\n{rendered}"
    );
    assert!(
        rendered.contains("Yes, allow all edits during this session"),
        "session option 2 must remain present\n{rendered}"
    );
}

pub(super) fn question_permission_modal_renders_questions_and_answer_input() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_modal"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_modal".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question".into()),
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

    let debug = render_live_buffer(&app, 100, 28);
    assert!(debug.contains("Pick one"));
    // Waiting state: all options (○) until answered; cursor focus uses styles only.
    assert!(
        debug.contains("(○)"),
        "unanswered options must paint ○\n{debug}"
    );
    assert!(
        !debug.contains("(●)"),
        "unanswered options must not paint ●\n{debug}"
    );
    assert!(debug.contains("A"));
    assert!(debug.contains("Type your answer here"));
    assert!(debug.contains("↑/↓ navigate"));
    assert!(debug.contains("y copy"));
    assert!(debug.contains("Enter:submit"));
    assert!(debug.contains("Esc:scrollback"));
    assert!(debug.contains("Tab:next option"));
    assert!(debug.contains("Shift+Tab:previous option"));
    assert!(debug.contains("Shift+X:dismiss"));
    assert!(!debug.contains("Question required"));
    assert!(!debug.contains("default deny"));
    assert!(!debug.contains("always-approve"));
    assert!(!debug.contains("1. A"));
}

pub(super) fn question_permission_modal_aligns_option_description_column() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_align"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_align".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_align".into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Which color?",
                    "header": "Color",
                    "options": [
                        {"label": "Red", "description": "Choose red"},
                        {"label": "Green", "description": "Choose green"},
                        {"label": "Blue", "description": "Choose blue"}
                    ],
                    "multiple": false,
                    "custom": true,
                }]
            })
            .to_string(),
            request_digest: "digest-question-align".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let debug = render_live_buffer(&app, 100, 28);
    assert!(debug.contains("Which color?"), "{debug}");
    // Descriptions share a column after padded labels (Green is widest).
    assert!(
        debug.contains("Red  ") && debug.contains("Choose red"),
        "Red label must pad to Green width\n{debug}"
    );
    assert!(
        debug.contains("Green  Choose green") || debug.contains("Green\tChoose green"),
        "Green description follows label with two-space gap\n{debug}"
    );
    assert!(
        debug.contains("Blue ") && debug.contains("Choose blue"),
        "Blue label must pad to Green width\n{debug}"
    );
}

pub(super) fn answered_questions_render_in_completed_tool_row() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_result"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_question_result".into(),
            text: "Ask me a follow-up".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_question_result"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool_call_question_result".into(),
            tool_id: "user.question".to_string(),
            args_summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    },
                    {
                        "question": "Pick another",
                        "header": "Mode",
                        "options": [{"label": "B", "description": "Option B"}],
                    }
                ]
            })
            .to_string(),
            args_digest: "digest-question-result-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("tool_call_question_result"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_result".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_result".into()),
            summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    },
                    {
                        "question": "Pick another",
                        "header": "Mode",
                        "options": [{"label": "B", "description": "Option B"}],
                    }
                ]
            })
            .to_string(),
            request_digest: "digest-question-result-permission".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("tool_call_question_result"),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_question_result".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: Some("[[\"A\"],[]]".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_question_result"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool_call_question_result".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("User has answered your questions.".to_string()),
            output_digest: Some("digest-question-result-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    let debug = render_live_buffer(&app, 120, 30);
    assert!(debug.contains("Asked 2 questions"));
    assert!(debug.contains("1. Pick one"));
    assert!(debug.contains("→ A"));
    assert!(debug.contains("2. Pick another"));
    assert!(debug.contains("→ (no answer)"));
}

pub(super) fn permission_modal_ctrl_y_emits_resolve_intent_and_closes_on_resolved() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    ));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0],
        UiIntent::ResolvePermission {
            permission_id: "perm_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }
    );
    drop(intents);

    assert!(app.active_permission().is_some());

    app.ingest_event(permission_resolved_event(
        2,
        "perm_1",
        PermissionDecision::Allow,
    ));
    assert!(app.active_permission().is_none());
}
