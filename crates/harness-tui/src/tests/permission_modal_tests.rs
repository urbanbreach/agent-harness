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
    assert!(debug.contains("1. A"));
    assert!(debug.contains("Type your own answer"));
    assert!(debug.contains("↑↓ select"));
    assert!(debug.contains("enter submit"));
    assert!(debug.contains("esc dismiss"));
    assert!(!debug.contains("Question required"));
    assert!(!debug.contains("default deny"));
    assert!(!debug.contains("Allow once"));
}

pub(super) fn question_permission_modal_matches_reference_palette_contract() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_palette"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_palette".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_palette".into()),
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
            request_digest: "digest-question-palette".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let buffer = render_live_cells(&app, 100, 28);
    let (tab_row, tab_fgs, tab_bgs) =
        row_text_and_palette(&buffer, 100, "Choice").unwrap_or_abort();
    let tab_start = tab_row[..tab_row.find("Choice").unwrap_or_abort()]
        .chars()
        .count();
    let tab_end = tab_start + "Choice".chars().count();
    assert!(tab_bgs[tab_start..tab_end]
        .iter()
        .all(|color| *color == Color::Rgb(0x9D, 0x7C, 0xD8)));
    assert!(tab_fgs[tab_start..tab_end]
        .iter()
        .all(|color| *color == Color::Rgb(0x0A, 0x0A, 0x0A)));

    let (option_row, option_fgs, option_bgs) =
        row_text_and_palette(&buffer, 100, "1. A").unwrap_or_abort();
    let number_start = option_row[..option_row.find("1.").unwrap_or_abort()]
        .chars()
        .count();
    let label_start = option_row[..option_row.find("A").unwrap_or_abort()]
        .chars()
        .count();
    assert_eq!(option_bgs[number_start], Color::Rgb(0x1E, 0x1E, 0x1E));
    assert_eq!(option_bgs[label_start], Color::Rgb(0x1E, 0x1E, 0x1E));
    assert_eq!(option_fgs[label_start], Color::Rgb(0x5C, 0x9C, 0xF5));
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
    assert!(!debug.contains("Pick one"));
    assert!(!debug.contains("Pick another"));
    assert!(!debug.contains("(no answer)"));
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
