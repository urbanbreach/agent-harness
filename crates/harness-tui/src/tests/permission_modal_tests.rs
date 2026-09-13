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
        footer == option_end + 3,
        "the shell footer follows the panel's bottom padding and spacer\n{rendered}"
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
    assert!(debug.contains("Tab:next answer"));
    assert!(debug.contains("Shift+x:dismiss"));
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

#[test]
fn question_native_request_does_not_require_a_header() {
    let mut question = question_parity_fixture();
    question["questions"][0]
        .as_object_mut()
        .unwrap()
        .remove("header");
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_parity_request(&question));

    let permission = app.active_permission_view().unwrap();
    let prompts = permission
        .question_prompts
        .as_ref()
        .expect("native question pane");
    assert_eq!(prompts.len(), 1);
    assert!(!prompts[0].multiple);
    assert!(prompts[0].custom);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.question_prompt_selection("question-parity"), 1);
    assert!(!app.permission_submission_pending("question-parity"));
    capture_native_question_states(&question, "native-single");
}

#[test]
fn question_native_multiselect_fields_preserve_fixed_answers() {
    for field in ["multiSelect", "multi_select", "multiple"] {
        let mut question = question_parity_fixture();
        question["questions"][0][field] = serde_json::json!(true);
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(question_parity_request(&question));
        app.handle_key(key(KeyCode::Char(' ')));
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Char(' ')));

        assert_eq!(
            app.question_prompt_answers("question-parity"),
            vec![vec!["SQLite".to_string(), "PostgreSQL".to_string()]],
            "{field} must enable multiple selection through the real input handler"
        );
        assert!(!app.permission_submission_pending("question-parity"));
        app.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(
            app.question_prompt_answers("question-parity"),
            vec![vec!["SQLite".to_string()]]
        );
        if field != "multiple" {
            capture_native_question_states(&question, field);
        }
    }
}

#[test]
fn question_compact_footer_does_not_paint_a_partial_shortcut() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_parity_request(&question_parity_fixture()));
    let area = ratatui::layout::Rect::new(0, 0, 40, 32);
    let status = crate::layout::FrameLayoutPlan::for_app(&app, area)
        .status
        .unwrap();
    let footer_y = status.bottom() - 2;
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();

    // The last shortcut cannot fit at this viewport. Its partial tail must not
    // occupy the remaining cells; this asserts geometry, not hint wording.
    for x in status.right() - 2..status.right() {
        assert_eq!(buffer[(x, footer_y)].symbol(), " ");
    }
    assert_eq!(
        buffer[(status.x, footer_y)].fg,
        app.theme().terminal_colors.prompt_accent
    );
}

#[test]
fn question_body_uses_semantic_palette() {
    use crate::theme_tokens::{ColorRole, DESIGN_TOKENS};
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_parity_request(&question_parity_fixture()));
    app.handle_key(key(KeyCode::Down));
    let area = ratatui::layout::Rect::new(0, 0, 120, 32);
    let status = crate::layout::FrameLayoutPlan::for_app(&app, area)
        .status
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let token = |role| {
        let value = DESIGN_TOKENS
            .palette
            .roles
            .iter()
            .find(|token| token.role == role)
            .unwrap()
            .value;
        ratatui::style::Color::Rgb(value.red, value.green, value.blue)
    };
    assert_eq!(
        buffer[(status.x, status.y)].fg,
        token(ColorRole::QuestionAccent)
    );
    assert_eq!(
        buffer[(status.x + 1, status.y)].bg,
        token(ColorRole::QuestionSurface)
    );
    let choice_y = (status.y..status.bottom())
        .find(|&y| buffer[(status.x + 3, y)].symbol() == "2")
        .unwrap();
    assert_eq!(
        buffer[(status.x + 9, choice_y)].bg,
        token(ColorRole::QuestionSelected)
    );
    assert_eq!(
        buffer[(status.x + 21, choice_y)].fg,
        token(ColorRole::QuestionSecondary)
    );
    assert_eq!(
        buffer[(status.x + 9, choice_y + 1)].fg,
        token(ColorRole::QuestionSecondary)
    );
}

#[test]
fn question_body_allocation_replaces_composer_and_preserves_chrome() {
    for (width, expected_y, expected_chrome) in [(40, 16, 4), (120, 19, 3)] {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(question_parity_request(&question_parity_fixture()));
        app.handle_key(key(KeyCode::Down));
        let area = ratatui::layout::Rect::new(0, 0, width, 32);
        let plan = crate::layout::FrameLayoutPlan::for_app(&app, area);
        assert_eq!(plan.composer.unwrap().height, 0);
        let status = plan.status.unwrap();
        assert_eq!(
            status,
            ratatui::layout::Rect::new(2, expected_y, width - 4, 32 - expected_y)
        );
        let permission = app.active_permission_view().unwrap();
        let measure = crate::layout::question_dock_measure(&app, status.width, area, &permission);
        assert_eq!(measure.chrome_rows, expected_chrome);
        assert_eq!(measure.status_height, status.height);
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let geometry = crate::layout::question_dock_geometry(
            ratatui::layout::Rect {
                height: status.height - crate::layout::QUESTION_OUTER_FOOTER_ROWS,
                ..status
            },
            &measure,
        );
        for range in &measure.option_ranges {
            let start = range.start.max(measure.scroll_offset);
            let end = range
                .end
                .min(measure.scroll_offset + geometry.options.height);
            for row in start..end {
                let y = geometry.options.y + row - measure.scroll_offset;
                let changed = app.handle_mouse(
                    crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Moved,
                        column: geometry.options.x,
                        row: y,
                        modifiers: KeyModifiers::NONE,
                    },
                    area,
                    None,
                    None,
                    None,
                );
                let _ = changed;
                assert_eq!(
                    app.question_prompt_hovered("question-parity"),
                    Some(range.index)
                );
                if row == range.start {
                    assert_eq!(
                        buffer[(geometry.options.x, y)].symbol(),
                        (range.index + 1).to_string()
                    );
                }
            }
        }
    }
}

#[test]
fn question_freeform_wraps_into_measured_rows_without_a_fake_cursor() {
    for width in [40, 120] {
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(question_parity_request(&question_parity_fixture()));
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Enter));
        let answer = "Use an in-memory store for tests";
        for character in answer.chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        let area = ratatui::layout::Rect::new(0, 0, width, 32);
        let status = crate::layout::FrameLayoutPlan::for_app(&app, area)
            .status
            .unwrap();
        let permission = app.active_permission_view().unwrap();
        let measure = crate::layout::question_dock_measure(&app, status.width, area, &permission);
        let expected_rows = if width == 40 { 2 } else { 1 };
        assert_eq!(measure.sticky_rows, expected_rows);
        let geometry = crate::layout::question_dock_geometry(
            ratatui::layout::Rect {
                height: status.height - crate::layout::QUESTION_OUTER_FOOTER_ROWS,
                ..status
            },
            &measure,
        );
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let text_x = geometry.sticky.x + 8;
        let rendered = (geometry.sticky.y..geometry.sticky.bottom())
            .map(|y| {
                (text_x..status.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(rendered, answer);
        assert!(!buffer[(text_x, geometry.sticky.y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD));
        app.handle_mouse(
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Moved,
                column: text_x,
                row: geometry.sticky.bottom() - 1,
                modifiers: KeyModifiers::NONE,
            },
            area,
            None,
            None,
            None,
        );
        assert_eq!(app.question_prompt_hovered("question-parity"), Some(2));
    }
}

#[test]
fn question_scrollbar_fills_each_native_thumb_cell() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_parity_request(&question_parity_fixture()));
    app.handle_key(key(KeyCode::Down));
    let area = ratatui::layout::Rect::new(0, 0, 120, 32);
    let status = crate::layout::FrameLayoutPlan::for_app(&app, area)
        .status
        .unwrap();
    let permission = app.active_permission_view().unwrap();
    let measure = crate::layout::question_dock_measure(&app, status.width, area, &permission);
    let geometry = crate::layout::question_dock_geometry(
        ratatui::layout::Rect {
            height: measure.dock_height,
            ..status
        },
        &measure,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 32)).unwrap();
    terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
    let (scrollbar, _) = geometry.scrollbar.unwrap();
    assert_eq!(scrollbar.height, 2);
    for row in scrollbar.y..scrollbar.bottom() {
        let cell = &terminal.backend().buffer()[(scrollbar.x, row)];
        assert_eq!(cell.symbol(), "█");
        assert_eq!(cell.bg, app.theme().question_prompt.secondary);
    }
}

#[test]
fn question_peek_editor_and_small_dock_stay_inside_their_content_bounds() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(question_parity_request(&question_parity_fixture()));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    for character in "甲乙 丙丁 戊己 庚辛 壬癸 ".repeat(20).chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    let permission = app.active_permission_view().unwrap();
    for width in [12, 31, 111] {
        let measure = crate::layout::question_content_measure(&app, width, 32, &permission);
        assert_eq!(measure.content_width, width);
        assert!(measure.editor_lines.len() <= 10);
        for line in &measure.editor_lines {
            assert!(ratatui::text::Line::from(line.as_str()).width() + 8 <= usize::from(width));
        }
    }
    for height in [6, 10, 16, 32] {
        let frame_area = ratatui::layout::Rect::new(0, 0, 40, height);
        let status = crate::layout::FrameLayoutPlan::for_app(&app, frame_area)
            .status
            .unwrap();
        let measure =
            crate::layout::question_dock_measure(&app, status.width, frame_area, &permission);
        let dock = ratatui::layout::Rect {
            height: status
                .height
                .saturating_sub(crate::layout::QUESTION_OUTER_FOOTER_ROWS),
            ..status
        };
        let geometry = crate::layout::question_dock_geometry(dock, &measure);
        for rect in [
            geometry.chrome,
            geometry.options,
            geometry.sticky,
            geometry.footer,
        ] {
            assert!(
                rect.y >= dock.y && rect.bottom() <= dock.bottom(),
                "{dock:?} {rect:?}"
            );
        }
        let mut terminal = Terminal::new(TestBackend::new(40, height)).unwrap();
        terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
    }
    for long_title in [true, false] {
        let mut question = question_parity_fixture();
        let text = serde_json::Value::String("x".repeat(usize::from(u16::MAX) + 1));
        if long_title {
            question["questions"][0]["question"] = text;
        } else {
            question["questions"][0]["options"][0]["description"] = text;
        }
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(question_parity_request(&question));
        let mut terminal = Terminal::new(TestBackend::new(10, 32)).unwrap();
        terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
    }
}

fn question_parity_fixture() -> serde_json::Value {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../scripts/qa/fixtures/tool-interaction-scenarios.json"
    ))
    .unwrap();
    fixture["question"].clone()
}

fn question_parity_request(question: &serde_json::Value) -> EventEnvelopeV1 {
    envelope(
        1,
        Some("question-parity"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "question-parity".into(),
            kind: "question".into(),
            tool_call_id: Some("call".into()),
            summary: question.to_string(),
            request_digest: "synthetic".into(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

// Optional evidence output from production input and rendering, not a reference reducer.
// The unchanged native producer consumes these same fixtures and Down/Enter/text actions.
fn capture_native_question_states(question: &serde_json::Value, name: &str) {
    let Some(output) = std::env::var_os("HARNESS_QUESTION_PARITY_ARTIFACT_DIR") else {
        return;
    };
    let output = std::path::PathBuf::from(output).join(name);
    std::fs::create_dir_all(&output).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../scripts/qa/fixtures/tool-interaction-scenarios.json"
    ))
    .unwrap();
    for width in [40, 120] {
        for state in ["choice", "freeform"] {
            let mut app = AppState::new_live(None, false, None);
            app.restart_motion_epoch_for_evidence();
            app.ingest_event(question_parity_request(question));
            app.handle_key(key(KeyCode::Down));
            if state == "freeform" {
                app.handle_key(key(KeyCode::Down));
                app.handle_key(key(KeyCode::Enter));
                for character in fixture["freeform"].as_str().unwrap().chars() {
                    app.handle_key(key(KeyCode::Char(character)));
                }
            }
            assert_eq!(
                app.question_prompt_selection("question-parity"),
                if state == "freeform" { 2 } else { 1 }
            );
            assert_eq!(
                app.question_prompt_editing("question-parity"),
                state == "freeform"
            );
            assert!(!app.permission_submission_pending("question-parity"));
            let mut bytes = Vec::new();
            {
                let mut terminal = Terminal::with_options(
                    ratatui::backend::CrosstermBackend::new(&mut bytes),
                    ratatui::TerminalOptions {
                        viewport: ratatui::Viewport::Fixed(ratatui::layout::Rect::new(
                            0, 0, width, 32,
                        )),
                    },
                )
                .unwrap();
                terminal.draw(|frame| ui::render_app(frame, &app)).unwrap();
            }
            std::fs::write(
                output.join(format!(
                    "interaction-question-{state}-{width}x32-motion-0ms.ansi"
                )),
                bytes,
            )
            .unwrap();
        }
    }
}
