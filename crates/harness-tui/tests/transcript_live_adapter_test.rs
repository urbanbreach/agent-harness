use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
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
    app.focus = Focus::Details;

    // When: the public key path opens the selected transcript block.
    app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    // act
    // Then: the integrated screen owner enters the full-screen viewer mode.
    // assert
    assert_eq!(
        app.transcript_screen_mode(),
        Some(TranscriptScreenMode::SelectedBlockViewer)
    );
}
