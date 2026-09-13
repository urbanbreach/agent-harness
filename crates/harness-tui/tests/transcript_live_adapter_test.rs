use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestStartedEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::session::AssistantPart;
use harness_tui::app::{AppState, Focus};
use harness_tui::transcript_blocks::BlockKind;
use harness_tui::transcript_identity::TranscriptScreenMode;

fn user_event() -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-live-adapter-1".to_owned(),
        seq: 1,
        run_id: "run-live-adapter".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: Some("request-live-adapter".to_owned()),
        causation_id: None,
        stream_key: Some("run:run-live-adapter".to_owned()),
        payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "request-live-adapter".into(),
            text: "show the live adapter".to_owned(),
        }),
    }
}

#[test]
fn live_projection_reaches_the_production_transcript_adapter() {
    // arrange
    // Given: a real live AppState receiving a replayable user event.
    let mut app = AppState::new_live(None, false, None);

    // When: the production ingestion path accepts the event.
    app.ingest_event(user_event());

    // act
    // Then: the new identity/block owner contains the live turn and user block.
    let view = app
        .transcript_view_model()
        .expect("live AppState must expose the integrated transcript");
    // assert
    assert_eq!(view.identity.turns().len(), 1);
    assert_eq!(view.blocks.len(), 1);
    assert_eq!(view.blocks[0].kind, BlockKind::User);
}

#[test]
fn live_transcript_viewer_is_reachable_from_production_input() {
    // arrange
    // Given: a live transcript with transcript focus.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(user_event());
    let mut started = user_event();
    started.seq = 2;
    started.event_id = "provider-start".into();
    started.payload = EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
        request_id: "provider-viewer".into(),
        provider_id: "mock".into(),
        model_id: "fixture".into(),
        prompt_summary: "show the live adapter".into(),
        request_digest: "fixture".into(),
        metadata: None,
    });
    app.ingest_event(started);
    let mut finished = user_event();
    finished.seq = 3;
    finished.event_id = "provider-finish".into();
    finished.payload = EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
        request_id: "provider-viewer".into(),
        tool_call_count: 0,
        parts: vec![
            AssistantPart::Reasoning {
                text: "First inspect the source".into(),
            },
            AssistantPart::Text {
                text: (0..60)
                    .map(|index| {
                        if index == 40 {
                            "unique-tail 川山".into()
                        } else {
                            format!("**Recorded answer line {index}**")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            },
        ],
        provenance: None,
        assistant_message: None,
    });
    app.ingest_event(finished);
    app.focus = Focus::Details;
    let render = |app: &AppState| {
        harness_tui::render_test::render_to_string(
            app,
            ratatui::layout::Rect::new(0, 0, 80, 24),
            |app, frame, _| harness_tui::ui::render_app(frame, app),
        )
    };
    let _ = render(&app);

    // Selection advances within a turn, and opens the selected answer rather than its prompt.
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    // act
    // Then: the integrated screen owner enters the full-screen viewer mode.
    // assert
    assert_eq!(
        app.transcript_screen_mode(),
        Some(TranscriptScreenMode::SelectedBlockViewer)
    );
    let first = render(&app);
    assert!(first.contains("Recorded answer line 0"), "{first}");
    assert!(!first.contains("unique-tail"));
    assert!(
        !first.contains("**Recorded"),
        "pretty viewer must parse source Markdown"
    );
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    let paged = render(&app);
    assert!(!paged.contains("Recorded answer line 0"), "{paged}");
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for character in "unique-tail".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let found = render(&app);
    assert!(
        found.contains("unique-tail") && found.contains('川') && found.contains('山'),
        "{found}"
    );
    assert!(found.contains("[search: unique-tail]"), "{found}");
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert!(
        render(&app).contains("**Recorded answer line 0**"),
        "raw viewer must retain Markdown source"
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert!(
        !render(&app).contains("Keyboard Shortcuts"),
        "viewer must own input"
    );
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_ne!(
        app.transcript_screen_mode(),
        Some(TranscriptScreenMode::SelectedBlockViewer)
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert!(
        render(&app).contains("Recorded answer line 0"),
        "closing restores the selected entry"
    );
}
