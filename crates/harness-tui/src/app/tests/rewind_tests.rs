use super::*;
use crate::rewind_view::RewindPhase;
use harness_core::conversation_rewind::RewindPoint;

#[test]
#[allow(
    clippy::cognitive_complexity,
    reason = "One end-to-end interaction sequence shares a live app and intent sink"
)]
fn rewind_keyboard_mouse_draft_confirmation_and_toast_contract() {
    let directory = tempfile::tempdir().unwrap_or_abort();
    let intents = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| intents.lock().unwrap_or_abort().push(intent))
            as Arc<dyn Fn(UiIntent) + Send + Sync>
    };
    let mut app = AppState::new_live(Some(directory.path().to_path_buf()), false, Some(sink));
    app.ingest_event(run_started(1));
    app.ingest_event(envelope_with_actor(
        2,
        "first",
        EventActor::new(ActorKind::User, None),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "first".into(),
            text: "first prompt".into(),
        }),
    ));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.rewind.state.is_none());
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "running Esc must not cancel"
    );
    assert_eq!(
        app.toast().unwrap_or_abort().message,
        "Press Ctrl+c to cancel the turn"
    );
    app.open_rewind();
    assert!(matches!(
        app.rewind.state.as_ref().map(|state| &state.phase),
        Some(RewindPhase::CancelOffer { .. })
    ));
    capture(&app, "cancel");
    app.handle_key(key(KeyCode::Char('n')));
    app.ingest_event(provider_started(3, "first", "mock", "model"));
    app.ingest_event(envelope(
        4,
        "first",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "first".into(),
            finish_reason: "stop".into(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));
    let points = vec![
        RewindPoint {
            seq: 2,
            request_id: "first".into(),
            text: "first prompt".into(),
        },
        RewindPoint {
            seq: 3,
            request_id: "second".into(),
            text: "second prompt".into(),
        },
    ];
    app.rewind.config_path = Some(directory.path().join("tui.json"));
    app.replace_prompt_input("unsent draft".into());
    app.open_rewind();
    let generation = app.rewind.generation;
    assert!(app.composer.prompt_buffer.is_empty());
    let rendered = render_text(&app, 80, 24);
    assert!(
        rendered.contains("Loading rewind points..."),
        "{rendered}\nstate={:?}",
        app.rewind.state.as_ref().map(|state| &state.phase)
    );
    capture(&app, "loading");
    app.apply_rewind_points(generation, Ok(points.clone()));
    let rendered = render_text(&app, 80, 24);
    assert!(rendered.contains("Rewind to which turn?"), "{rendered}");
    assert!(rendered.rfind("second prompt") < rendered.rfind("first prompt"));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.rewind_dim_from_seq(), Some(2));
    capture(&app, "picker");
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol() == "f" && cell.fg == ratatui::style::Color::Rgb(88, 88, 88)),
        "selected prompt uses Grok gray_dim"
    );
    let area = app.rewind_area(Rect::new(0, 0, 80, 24)).unwrap_or_abort();
    app.set_transcript_scroll_from_top_with_max(7, 20);
    app.handle_rewind_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: area.x + 5,
            row: area.y + 3,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 80, 24),
    );
    assert_eq!(
        app.transcript_view.measured_viewport().offset_from_bottom(),
        13,
        "hovering the selected row must preserve manual scroll"
    );
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.composer.prompt_buffer, "unsent draft");
    app.apply_rewind_points(generation, Ok(points.clone()));
    assert!(
        app.rewind.state.is_none(),
        "dismissed fetch cannot reopen the panel"
    );

    app.clear_prompt_input();
    app.rewind.suppress_until = None;
    app.handle_key(key(KeyCode::Esc));
    assert!(app.rewind.state.is_none());
    app.handle_key(KeyEvent::new_with_kind(
        KeyCode::Esc,
        KeyModifiers::NONE,
        crossterm::event::KeyEventKind::Release,
    ));
    assert!(app.rewind.state.is_none());
    app.handle_key(key(KeyCode::Esc));
    let generation = app.rewind.generation;
    app.apply_rewind_points(generation, Ok(points.clone()));
    let area = app.rewind_area(Rect::new(0, 0, 80, 24)).unwrap_or_abort();
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 5,
            row: area.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 80, 24),
        None,
        None,
        None,
    );
    assert!(matches!(
        app.rewind.state.as_ref().map(|state| &state.phase),
        Some(RewindPhase::Confirm {
            target_prompt_index: 1,
            ..
        })
    ));
    capture(&app, "confirm");
    app.handle_key(key(KeyCode::Char('a')));
    capture(&app, "executing");
    assert!(!app.rewind.confirm);
    let saved: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(directory.path().join("tui.json")).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    assert_eq!(saved["confirm_before_rewind"], false);
    assert!(intents.lock().unwrap_or_abort().iter().any(|intent| matches!(intent, UiIntent::RewindConversation {request_id,..} if request_id == "second")));
    app.apply_rewind_result(generation, Ok(points[1].clone()));
    assert_eq!(app.composer.prompt_buffer, "second prompt");
    assert_eq!(
        app.toast().unwrap_or_abort().message,
        "Reverted conversation"
    );
    assert!(render_text(&app, 80, 24).contains("Reverted conversation"));
    capture(&app, "success");
    app.open_rewind();
    let generation = app.rewind.generation;
    app.apply_rewind_points(generation, Ok(points.clone()));
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(
        app.rewind.state.as_ref().map(|state| &state.phase),
        Some(RewindPhase::Executing { .. })
    ));
    app.apply_rewind_result(generation, Err("test failure".into()));
    assert!(render_text(&app, 80, 24).contains("Rewind failed"));
    capture(&app, "error");
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.composer.prompt_buffer, "second prompt");

    // A failed settings write rolls the preference back but never blocks rewind.
    app.rewind.confirm = true;
    app.rewind.config_path = Some(directory.path().to_path_buf());
    app.open_rewind();
    app.apply_rewind_points(app.rewind.generation, Ok(points));
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('a')));
    assert!(app.rewind.confirm);
    assert!(matches!(
        app.rewind.state.as_ref().map(|state| &state.phase),
        Some(RewindPhase::Executing { .. })
    ));
}

fn capture(app: &AppState, name: &str) {
    let Some(directory) = std::env::var_os("HARNESS_REWIND_CAPTURE") else {
        return;
    };
    let directory = PathBuf::from(directory);
    fs::create_dir_all(&directory).unwrap_or_abort();
    for (width, height) in [(40, 16), (80, 24), (120, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
        terminal
            .draw(|frame| render_app(frame, app))
            .unwrap_or_abort();
        let cells: Vec<_> = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| {
                (
                    cell.symbol(),
                    format!("{:?}", cell.fg),
                    format!("{:?}", cell.bg),
                    cell.modifier.bits(),
                )
            })
            .collect();
        let value = serde_json::json!({"width":width,"height":height,"cells":cells});
        fs::write(
            directory.join(format!("{name}-{width}.json")),
            serde_json::to_vec(&value).unwrap_or_abort(),
        )
        .unwrap_or_abort();
    }
}
