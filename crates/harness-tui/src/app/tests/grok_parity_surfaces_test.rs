//! Behavioral owners also export the exact rendered cells for xterm.js inspection.
use super::*;
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use ratatui::{backend::CrosstermBackend, TerminalOptions, Viewport};
#[path = "../../../tests/support/deterministic_render_fixtures.rs"]
mod recorded_tools;

const STRUCTURED_MARKDOWN: &str = "**outer *inner* end** and **[reference](https://example.com)**.\n\n~~~rust\nfn main() {\n\tlet value = 42;\n    println!(\"{value}\");\n}\n~~~\n\n| Left | Center | Right |\n| :--- | :---: | ---: |\n| alpha | beta | 123 |\n\n> outer quote\n> > nested quote with wrapped words\n\n$E=mc^2$\n\n";

#[test]
fn composer_selection_and_mentions_follow_visible_wrapped_cells() {
    for (width, height) in [(40, 24), (80, 24), (120, 40)] {
        let area = Rect::new(0, 0, width, height);
        let mut app = AppState::new_live(None, false, None);
        app.set_reduced_motion_for_evidence(true);
        app.set_frame_area(area);
        app.set_file_mention_collaborators_for_test(
            PathBuf::from("/virtual/workspace"),
            vec!["src/main.rs".into()],
            123,
        );
        for ch in "@main".chars() {
            app.handle_key(key(KeyCode::Char(ch)));
        }
        app.handle_key(key(KeyCode::Enter));
        app.handle_paste("cafe\u{301} 👩‍💻 hello");
        app.handle_key(key_with_modifiers(
            KeyCode::Left,
            KeyModifiers::SHIFT | KeyModifiers::CONTROL,
        ));
        let _ = capture(&app, area, "composer-selection");
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
        terminal
            .draw(|frame| render_app(frame, &app))
            .unwrap_or_abort();
        let buffer = terminal.backend().buffer();
        let selected: String = buffer
            .content
            .iter()
            .filter(|cell| cell.modifier.contains(Modifier::REVERSED))
            .map(|cell| cell.symbol())
            .collect();
        assert_eq!(
            selected, "hello",
            "selection must be visible at {width}x{height}"
        );
        let mention = buffer
            .content
            .iter()
            .find(|cell| cell.symbol() == "@")
            .unwrap_or_abort();
        assert_eq!(mention.fg, app.theme().status.warning);
        assert!(buffer
            .content
            .iter()
            .any(|cell| cell.symbol() == "e\u{301}"));
        assert!(buffer.content.iter().any(|cell| cell.symbol() == "👩‍💻"));
        let index = buffer
            .content
            .iter()
            .position(|cell| cell.modifier.contains(Modifier::REVERSED))
            .unwrap_or_abort();
        let index = u16::try_from(index).unwrap_or_abort();
        let hello = (index % width, index / width);
        assert_eq!(
            terminal.get_cursor_position().unwrap_or_abort(),
            hello.into()
        );

        // Drive actual pointer input against the cells the user sees.
        for (kind, column) in [
            (MouseEventKind::Down(MouseButton::Left), hello.0),
            (MouseEventKind::Drag(MouseButton::Left), hello.0 + 5),
            (MouseEventKind::Up(MouseButton::Left), hello.0 + 5),
        ] {
            app.handle_mouse(
                MouseEvent {
                    kind,
                    column,
                    row: hello.1,
                    modifiers: KeyModifiers::NONE,
                },
                area,
                None,
                None,
                None,
            );
        }
        assert_eq!(
            app.composer.selection_anchor,
            Some(app.composer.prompt_cursor - 5)
        );
        app.handle_paste("ready");
        assert_eq!(
            app.composer.prompt_buffer,
            "@src/main.rs cafe\u{301} 👩‍💻 ready"
        );
        let _ = capture(&app, area, "composer-mouse-edit");

        // A grapheme that straddled the old scalar-width boundary stays on one row.
        app.clear_prompt_input();
        let input_width = FrameLayoutPlan::for_app(&app, area)
            .composer
            .unwrap_or_abort()
            .width
            - 6;
        let prefix = "x".repeat(usize::from(input_width - 2));
        app.handle_paste(&format!("{prefix}👩‍💻e\u{301}界"));
        terminal
            .draw(|frame| render_app(frame, &app))
            .unwrap_or_abort();
        let buffer = terminal.backend().buffer();
        let index = buffer
            .content
            .iter()
            .position(|cell| cell.symbol() == "👩‍💻")
            .unwrap_or_abort();
        let index = u16::try_from(index).unwrap_or_abort();
        let emoji = (index % width, index / width);
        assert_eq!(buffer[(emoji.0 - 1, emoji.1)].symbol(), "x");
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_mouse(
                MouseEvent {
                    kind,
                    column: emoji.0 + 1,
                    row: emoji.1,
                    modifiers: KeyModifiers::NONE,
                },
                area,
                None,
                None,
                None,
            );
        }
        assert_eq!(app.composer.prompt_cursor, prefix.len());
        let _ = capture(&app, area, "composer-unicode-wrap");
        app.handle_key(key(KeyCode::Delete));
        assert_eq!(app.composer.prompt_buffer, format!("{prefix}e\u{301}界"));
        app.handle_key(key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT));
        app.handle_paste("A");
        assert_eq!(app.composer.prompt_buffer, format!("{prefix}A界"));

        // A new key owner and a resized surface invalidate an unfinished drag.
        let pointer = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: emoji.0,
            row: emoji.1,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_composer_mouse(pointer, area);
        app.handle_key(key_with_modifiers(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL,
        ));
        let cursor = app.composer.prompt_cursor;
        assert!(!app.handle_composer_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 0,
                ..pointer
            },
            area
        ));
        assert_eq!(app.composer.prompt_cursor, cursor);
        app.handle_key(key(KeyCode::Esc));
        app.handle_composer_mouse(pointer, area);
        assert!(!app.handle_composer_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 0,
                ..pointer
            },
            Rect::new(0, 0, width + 1, height)
        ));
        app.replay_mode = true;
        assert!(!app.handle_composer_mouse(pointer, area));
    }
}

fn event(run: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("{run}-{seq}"),
        seq,
        run_id: run.into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: Some(run.into()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn conversation(run: &str) -> Vec<EventEnvelopeV1> {
    vec![
        event(
            run,
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: run.into(),
                text: "Review parser indentation and terminal rendering".into(),
            }),
        ),
        event(
            run,
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: run.into(),
                provider_id: "mock".into(),
                model_id: "recorded-model".into(),
                prompt_summary: "Review parser indentation and terminal rendering".into(),
                request_digest: "fixture".into(),
                metadata: None,
            }),
        ),
        event(
            run,
            3,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: run.into(),
                delta: STRUCTURED_MARKDOWN.to_owned()
                    + &(0..45)
                        .map(|index| {
                            format!("**Recorded result {index}** with *nested emphasis*.\n\n")
                        })
                        .collect::<Vec<_>>()
                        .join(""),
            }),
        ),
    ]
}

fn capture(app: &AppState, area: Rect, scene: &str) -> String {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(directory) = std::env::var_os("HARNESS_PARITY_RENDER_ARTIFACT_DIR") {
        let directory = Path::new(&directory);
        fs::create_dir_all(directory).unwrap_or_abort();
        let name = format!("{scene}-{}x{}-reduced-0ms", area.width, area.height);
        let mut bytes = Vec::new();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut bytes),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )
        .unwrap_or_abort();
        terminal
            .draw(|frame| render_app(frame, app))
            .unwrap_or_abort();
        drop(terminal);
        fs::write(directory.join(format!("{name}.ansi")), bytes).unwrap_or_abort();
        fs::write(directory.join(format!("{name}.txt")), &rendered).unwrap_or_abort();
    }
    rendered
}

#[test]
fn dashboard_pending_input_uses_visible_review_and_preserves_parent_counts() {
    for (width, height, question) in [
        (80, 24, false),
        (120, 40, false),
        (160, 50, false),
        (80, 24, true),
        (120, 40, true),
        (160, 50, true),
    ] {
        assert_pending_dashboard_case(width, height, question);
    }
}

fn assert_pending_dashboard_case(width: u16, height: u16, question: bool) {
    let area = Rect::new(0, 0, width, height);
    let root = tempfile::tempdir().unwrap_or_abort();
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            let _ = sender.send(intent);
        })),
    );
    app.set_reduced_motion_for_evidence(true);
    for entry in conversation("awaiting-parent") {
        app.ingest_event(entry);
    }
    let mut history = Vec::new();
    for (run, parent) in [
        ("working-parser", None),
        ("working-terminal", None),
        ("child-parser", Some("working-parser")),
    ] {
        let run_dir = root.path().join(run);
        write_events_jsonl(&run_dir, &conversation(run));
        history.push(SessionHistoryEntry {
            run_dir,
            catalog: SessionCatalogEntry {
                run_id: run.into(),
                run_name: Some(run.into()),
                status: Some(RunStatus::Running),
                last_updated_at: None,
                workspace_root: Some(root.path().display().to_string()),
                profile_preset: Some("default".into()),
                provider_model: Some("mock/recorded-model".into()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: true,
                resume_disabled_reason: None,
                artifact_count: 0,
                child_session_count: 0,
                parent_session_id: parent.map(str::to_owned),
            },
        });
    }
    app.set_session_history_entries(history);
    app.set_frame_area(area);
    app.open_status_dashboard_at(area);
    app.ingest_event(event(
        "awaiting-parent",
        4,
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "permission-parity".into(),
            kind: if question { "question" } else { "bash" }.into(),
            tool_call_id: None,
            summary: if question { serde_json::json!({"questions": [{"header": "Theme", "question": "Choose the terminal theme", "options": [{"label": "Dark", "description": "Dark palette"}, {"label": "Light", "description": "Light palette"}]}]}).to_string() } else { "Run cargo check in the selected workspace".into() },
            request_digest: "permission-fixture".into(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    let rendered = capture(
        &app,
        area,
        if question {
            "dashboard-question"
        } else {
            "dashboard-permission"
        },
    );
    let dashboard = app.status_dashboard().unwrap_or_abort();
    assert!(
        rendered.contains("3 agents · 2 working · 1 needs input"),
        "{rendered}"
    );
    assert!(!rendered.contains("{\"questions\""), "{rendered}");
    assert_eq!(
        dashboard
            .dashboard()
            .rows
            .iter()
            .filter(|row| row.relationship.parent.is_none())
            .count(),
        3
    );
    let has_peek = dashboard.layout().peek.height > 0;
    assert_pending_review_visible(&rendered, has_peek, question);
    app.handle_key(key(KeyCode::Tab));
    if !has_peek {
        app.handle_key(key(KeyCode::Char('3')));
        assert!(
            receiver.try_recv().is_err(),
            "a hidden prompt must never accept a decision"
        );
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.status_dashboard_is_active());
        let _ = capture(
            &app,
            area,
            if question {
                "question-review"
            } else {
                "permission-review"
            },
        );
    }
    app.handle_key(key(if question {
        KeyCode::Enter
    } else {
        KeyCode::Char('3')
    }));
    let decisions = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(decisions.len(), 1, "{decisions:?}");
    assert!(
        matches!(&decisions[0], UiIntent::ResolvePermission { permission_id, decision: PermissionDecision::Allow, .. } if permission_id == "permission-parity")
    );
    app.ingest_event(event(
        "awaiting-parent",
        5,
        EventV1::PermissionResolved(harness_core::event::PermissionResolvedEvent {
            permission_id: "permission-parity".into(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: None,
        }),
    ));
    app.open_status_dashboard_at(area);
    let settled = capture(&app, area, "dashboard-resolved");
    assert!(settled.contains("3 agents · 3 working"), "{settled}");
    assert!(!settled.contains("needs input"), "{settled}");
}

#[test]
fn dashboard_tail_pins_prompt_and_secondary_editors_keep_recorded_content() {
    for (width, height) in [(80, 24), (120, 40), (160, 50)] {
        let area = Rect::new(0, 0, width, height);
        let mut app = AppState::new_live(None, false, None);
        app.set_reduced_motion_for_evidence(true);
        for entry in conversation("working-parser") {
            app.ingest_event(entry);
        }
        app.set_frame_area(area);
        let _ = capture(&app, area, "transcript-markdown");
        app.scroll_transcript_up(u16::MAX);
        let rendered = capture(&app, area, "transcript-structure");
        assert!(rendered.contains("outer inner end"), "{rendered}");
        app.set_transcript_following(true);
        app.open_status_dashboard_at(area);
        let rendered = capture(&app, area, "dashboard-working");
        if app
            .status_dashboard()
            .unwrap_or_abort()
            .layout()
            .peek
            .height
            > 0
        {
            assert!(
                rendered.contains("❯ Review parser indentation"),
                "{rendered}"
            );
            assert!(rendered.contains("Recorded result 44"), "{rendered}");
            assert!(!rendered.contains("**Recorded"), "{rendered}");
        }
        app.close_status_dashboard();
        for (command, title) in [
            ("settings", "Settings"),
            ("usage", "Usage"),
            ("extensions", "Extensions"),
        ] {
            app.execute_slash_command(command, None);
            let rendered = capture(&app, area, command);
            assert!(rendered.contains(title), "{rendered}");
            app.handle_key(key(KeyCode::Esc));
        }
    }
    for (width, height) in [(5, 4), (20, 6), (20, 8), (47, 9), (48, 10)] {
        let area = Rect::new(0, 0, width, height);
        let mut app = AppState::new_live(None, false, None);
        app.set_frame_area(area);
        app.execute_slash_command("settings", None);
        let _ = capture(&app, area, "settings-narrow");
    }
}

#[test]
fn settings_drafts_filter_choose_and_edit_through_visible_controls() {
    for (width, height) in [(40, 12), (80, 24), (120, 40)] {
        verify_settings_controls(Rect::new(0, 0, width, height));
    }
}

fn verify_settings_controls(area: Rect) {
    let root = tempfile::tempdir().unwrap_or_abort();
    let path = root.path().join("settings.json");
    let initial_config = r#"{"providers":{"default":{"type":"openai_compatible","base_url":"http://127.0.0.1:9/v1","api_key":"test-key","models":{"test":{"display_name":"Test"}}}},"model":"default:test"}"#;
    fs::write(&path, initial_config).unwrap_or_abort();
    let mut app = AppState::new_live(None, false, None);
    app.set_reduced_motion_for_evidence(true);
    app.set_frame_area(area);
    app.handle_paste("keep this draft");
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    app.handle_key(key(KeyCode::End));
    assert_eq!(
        app.settings_editor_selected_index(),
        app.settings_editor_rows().len() - 1
    );
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.settings_editor_selected_index(), 0);
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(
        app.settings_editor_selected_index(),
        usize::from(area.height.min(28) - 6)
    );
    app.handle_key(key(KeyCode::Home));

    click_settings_text(&mut app, area, "TUI");
    assert_eq!(app.settings_editor_tab(), SettingsTab::Tui);
    click_settings_text(&mut app, area, "Runtime");
    click_settings_text(&mut app, area, "/ to search");
    assert!(app.settings_interaction.filtering);
    app.handle_paste("no-such-setting👩‍💻");
    app.handle_key(key(KeyCode::Backspace));
    assert_eq!(app.settings_interaction.query, "no-such-setting");
    let rendered = capture(&app, area, "settings-no-matches");
    assert!(rendered.contains("No matches"), "{rendered}");
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.settings_editor_selected_id().is_none());
    assert!(app.settings_interaction.edit.is_none());

    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("permission bash");
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    let rendered = capture(&app, area, "settings-choices");
    for choice in ["ask", "allow", "deny"] {
        assert!(rendered.contains(choice), "{rendered}");
    }
    click_settings_text(&mut app, area, "allow");
    assert_eq!(
        app.settings_interaction
            .edit
            .as_ref()
            .unwrap_or_abort()
            .editor
            .text(),
        "allow"
    );
    assert_eq!(fs::read_to_string(&path).unwrap_or_abort(), initial_config);
    app.handle_paste("deny");
    assert_eq!(
        app.settings_interaction
            .edit
            .as_ref()
            .unwrap_or_abort()
            .editor
            .text(),
        "allow"
    );
    let _ = capture(&app, area, "settings-choice-selected");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.settings_interaction.edit.is_none());
    assert_eq!(
        harness_core::config::read_project_setting_value(&path, "permission.bash")
            .unwrap_or_abort()
            .as_deref(),
        Some("allow")
    );

    verify_settings_value_drafts(&mut app, area, &path);
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    verify_modal_paste_ownership(&mut app);
}

fn verify_modal_paste_ownership(app: &mut AppState) {
    for action in [Action::OpenSettings, Action::OpenThemeDialog] {
        app.execute_action(action);
        let draft = app.composer.prompt_buffer.clone();
        app.handle_paste("must stay in the modal");
        assert_eq!(app.composer.prompt_buffer, draft);
        app.handle_key(key(KeyCode::Esc));
        if app.settings_editor_visible {
            app.handle_key(key(KeyCode::Esc));
        }
    }
}

fn verify_settings_value_drafts(app: &mut AppState, area: Rect, path: &Path) {
    let (width, height) = (area.width, area.height);
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("fallback input tokens");
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("invalid");
    let before = fs::read(path).unwrap_or_abort();
    app.handle_key(key(KeyCode::Enter));
    let rendered = capture(app, area, "settings-invalid-value");
    assert!(rendered.contains("Enter a whole number"), "{rendered}");
    assert_eq!(fs::read(path).unwrap_or_abort(), before);
    app.handle_key(key(KeyCode::Esc));

    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("session dir");
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    let value = format!("{}cafe\u{301}👩‍💻界", "workspace/".repeat(12));
    app.handle_paste(&value);
    app.handle_key(key(KeyCode::Left));
    let rendered = capture(app, area, "settings-unicode-edit");
    assert!(
        rendered.contains("cafe\u{301}👩‍💻") && rendered.contains('界'),
        "{rendered}"
    );
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let caret = buffer
        .content
        .iter()
        .find(|cell| cell.symbol() == "界")
        .unwrap_or_abort();
    assert_eq!(
        caret.bg,
        app.theme().text.primary,
        "caret must follow the Unicode cursor"
    );
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    let _ = capture(app, area, "settings-value-selection");
    app.handle_paste("captured-sessions");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        harness_core::config::read_project_setting_value(path, "runtime.session_dir")
            .unwrap_or_abort()
            .as_deref(),
        Some("captured-sessions")
    );
}

fn click_settings_text(app: &mut AppState, area: Rect, text: &str) {
    let (column, row) = transcript_click_position_in_area(app, area, text);
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            area,
            None,
            None,
            None,
        );
    }
}

#[test]
fn recorded_tool_cells_preserve_code_diff_and_terminal_output() {
    for (width, height) in [(40, 40), (80, 24), (120, 40), (160, 50)] {
        let area = Rect::new(0, 0, width, height);
        let root = tempfile::tempdir().unwrap_or_abort();
        fs::create_dir_all(root.path().join("artifacts")).unwrap_or_abort();
        fs::write(root.path().join("artifacts/tool-lifecycle-inline.diff"),
            "--- a/crates/harness-tui/src/ui.rs\n+++ b/crates/harness-tui/src/ui.rs\n@@ -2 +2 @@\n-old line\n+new line\n").unwrap_or_abort();
        let mut app = AppState::new_live(Some(root.path().to_path_buf()), false, None);
        app.set_reduced_motion_for_evidence(true);
        for mut entry in recorded_tools::tool_lifecycle_events() {
            if let EventV1::ToolCallFinished(tool) = &mut entry.payload {
                match tool.tool_call_id.as_str() {
                    "tc_read" => {
                        tool.output_json = Some(
                            serde_json::json!({"metadata": {"display": {"text": "/* recorded multiline\n   comment */\nfn main() {\n\tlet value = 42;\n}\n", "lineStart": 1}}}),
                        )
                    }
                    "tc_edit" => {
                        tool.output_json = Some(
                            serde_json::json!({"before_text": "let text = r#\"\nold line\n\"#;\n", "diff": "--- a/src/ui.rs\n+++ b/src/ui.rs\n@@ -2 +2 @@\n-old line\n+new line\n"}),
                        )
                    }
                    "tc_shell" => {
                        tool.status = ToolCallStatus::Succeeded;
                        tool.output_summary =
                            Some("\u{1b}[31mred\u{1b}[0m\nprogress 1\rprogress 2\u{1b}[K".into());
                        tool.output_json = Some(
                            serde_json::json!({"stdout": tool.output_summary, "exit_code": 0}),
                        );
                    }
                    _ => {}
                }
            }
            app.ingest_event(entry);
        }
        for id in ["tc_read", "tc_edit", "tc_shell"] {
            app.set_tool_output_expanded(id, true);
        }
        app.set_frame_area(area);
        let tail = capture(&app, area, "tools-tail");
        assert!(tail.contains("progress 2"), "{tail}");
        assert!(!tail.contains("progress 1"), "{tail}");
        app.scroll_transcript_up(u16::MAX);
        let head = capture(&app, area, "tools-code-diff");
        assert!(head.contains("recorded multiline"), "{head}");
        assert!(head.contains("4      let value = 42;"), "{head}");
    }
    for (scene, path, mime, content, expected) in [
        ("empty", "src/empty.rs", None, "", "(empty file)"),
        (
            "skill",
            ".harness/skills/deploy/SKILL.md",
            None,
            "# Deploy\nRecorded instructions.",
            "Skill",
        ),
        (
            "image",
            "assets/chart.png",
            Some("image/png"),
            "Recorded image · 640×480",
            "Read chart.png (image)",
        ),
        (
            "pdf",
            "docs/report.pdf",
            Some("application/pdf"),
            "Recorded document · 2 pages",
            "Read report.pdf (PDF)",
        ),
    ] {
        verify_recorded_read_variant(scene, path, mime, content, expected);
    }
}

fn verify_recorded_read_variant(
    scene: &str,
    path: &str,
    mime: Option<&str>,
    content: &str,
    expected: &str,
) {
    let area = Rect::new(0, 0, 120, 40);
    let mut app = AppState::new_live(None, false, None);
    app.set_reduced_motion_for_evidence(true);
    for mut entry in recorded_tools::tool_lifecycle_events().into_iter().take(5) {
        match &mut entry.payload {
            EventV1::ToolCallRequested(tool) => {
                tool.args_summary = serde_json::json!({"path": path}).to_string()
            }
            EventV1::ToolCallFinished(tool) => {
                tool.output_json = Some(
                    serde_json::json!({"metadata": {"display": {"text": content, "lineStart": 1}}, "attachments": mime.map(|mime| vec![serde_json::json!({"mime": mime})]).unwrap_or_default()}),
                )
            }
            _ => {}
        }
        app.ingest_event(entry);
    }
    app.set_tool_output_expanded("tc_read", true);
    app.set_frame_area(area);
    if mime.is_some() {
        app.focus = Focus::Details;
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        app.focus = Focus::Prompt;
    }
    let rendered = capture(&app, area, &format!("read-{scene}"));
    assert!(rendered.contains(expected), "{rendered}");
}

fn assert_pending_review_visible(rendered: &str, has_peek: bool, question: bool) {
    assert!(
        rendered.contains(if has_peek {
            if question {
                "Choose the terminal theme"
            } else {
                "Allow once"
            }
        } else {
            "Enter to review"
        }),
        "{rendered}"
    );
}
