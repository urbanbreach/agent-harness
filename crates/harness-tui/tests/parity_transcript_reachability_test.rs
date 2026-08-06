use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus};
use harness_tui::design_contract::LifecycleState;
use harness_tui::transcript_blocks::{BlockKind, BlockLifecycle, FoldState, RawDisclosure};
use harness_tui::transcript_identity::{ReplayTurn, TranscriptScreenMode, TranscriptSelection};
use harness_tui::transcript_integration::{
    BlockSeed, TimelineStatus, TranscriptComposite, TranscriptEvent, TurnSeed,
};
use harness_tui::transcript_pager::{
    PagerCommand, PagerError, PagerStdio, TerminalControl, TerminalState,
};
use harness_tui::transcript_scroll::MotionPreference;
use ratatui::{backend::TestBackend, Terminal};

const APP_SOURCE: &str = include_str!("../src/app.rs");
const TRANSCRIPT_STATE_SOURCE: &str = include_str!("../src/app/transcript_state.rs");
const TRANSCRIPT_INTEGRATION_SOURCE: &str = include_str!("../src/transcript_integration/mod.rs");
const UI_SOURCE: &str = include_str!("../src/ui.rs");
const UI_TRANSCRIPT_SOURCE: &str = include_str!("../src/ui_transcript.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/runtime.rs");
const MOUSE_SOURCE: &str = include_str!("../src/app/mouse_interaction.rs");

fn user_event(seq: u64, request_id: &str, text: &str) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{request_id}"),
        seq,
        run_id: "run-transcript-parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: Some(request_id.to_owned()),
        causation_id: None,
        stream_key: Some("run:run-transcript-parity".to_owned()),
        payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.to_owned().into(),
            text: text.to_owned(),
        }),
    }
}

fn composite_with_raw_tool() -> TranscriptComposite {
    let mut composite = TranscriptComposite::new(ratatui::layout::Rect::new(0, 0, 80, 24))
        .expect("valid transcript viewport");
    let replay = ReplayTurn::event(1, 0, 1);
    composite
        .replace_events(vec![
            TranscriptEvent::TurnStarted(
                TurnSeed::new(replay, TimelineStatus::Completed, LifecycleState::Completed)
                    .with_label("tool turn"),
            ),
            TranscriptEvent::BlockCreated(BlockSeed {
                id: replay.block_id(0),
                turn_id: replay.turn_id(),
                kind: BlockKind::Tool,
                lifecycle: BlockLifecycle::Completed,
                content: "tool output".to_owned(),
                raw: Some(RawDisclosure::from_json(&serde_json::json!({
                    "token": "Bearer secret",
                    "result": "full output",
                }))),
            }),
        ])
        .expect("tool transcript events should project");
    composite
}

#[derive(Debug)]
struct FakeTerminal {
    state: TerminalState,
}

impl FakeTerminal {
    fn new() -> Self {
        Self {
            state: TerminalState::active(100, 30),
        }
    }
}

impl TerminalControl for FakeTerminal {
    fn state(&self) -> TerminalState {
        self.state.clone()
    }

    fn suspend_raw_mode(&mut self) -> Result<(), PagerError> {
        Ok(())
    }

    fn suspend_mouse(&mut self) -> Result<(), PagerError> {
        Ok(())
    }

    fn reset_title(&mut self) -> Result<(), PagerError> {
        Ok(())
    }

    fn pause_media(&mut self) -> Result<(), PagerError> {
        Ok(())
    }

    fn suspend_cursor(&mut self) -> Result<(), PagerError> {
        Ok(())
    }

    fn reinitialize_capabilities(&mut self) -> Result<(), PagerError> {
        Ok(())
    }

    fn restore_cursor(&mut self, _visible: bool) -> Result<(), PagerError> {
        Ok(())
    }

    fn restore_media(&mut self, _active: bool) -> Result<(), PagerError> {
        Ok(())
    }

    fn restore_title(&mut self, _title: Option<&str>) -> Result<(), PagerError> {
        Ok(())
    }

    fn restore_mouse(&mut self, _active: bool) -> Result<(), PagerError> {
        Ok(())
    }

    fn restore_raw_mode(&mut self, _active: bool) -> Result<(), PagerError> {
        Ok(())
    }
}

#[test]
fn live_and_replay_sessions_reach_the_transcript_parity_owner() {
    // Given: real live and replay AppState values, not a fixture renderer.
    let long_text = "transcript line\n".repeat(48);
    let first = user_event(1, "request-one", &long_text);
    let second = user_event(2, "request-two", "second live turn");
    let mut live = AppState::new_live(None, false, None);
    let mut replay =
        AppState::new_replay(PathBuf::from("/tmp/transcript-parity"), vec![first.clone()]);

    // When: both production ingestion paths receive transcript events and the live shell renders.
    live.ingest_event(first);
    live.ingest_event(second);
    live.focus = Focus::Details;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test terminal");
    terminal
        .draw(|frame| harness_tui::ui::render_app(frame, &live))
        .expect("live transcript should render");

    // Then: both paths expose stable identity/block state through the composite owner.
    assert_eq!(
        live.transcript_view_model()
            .map(|view| view.identity.turns().len()),
        Some(2)
    );
    assert_eq!(
        live.transcript_view_model().map(|view| view.blocks.len()),
        Some(2)
    );
    assert!(live.select_transcript_turn_at(1));
    assert_eq!(
        live.transcript_view_model()
            .map(|view| view.screen.focus_follow().focus),
        Some(harness_tui::transcript_identity::TranscriptFocus::Timeline)
    );
    assert!(!live.transcript_following() || live.transcript_scroll_offset() == 0);
    live.scroll_page_up(1);
    assert!(!live.transcript_following());
    assert!(live.toggle_selected_transcript_fold());
    live.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
    assert_eq!(
        live.transcript_screen_mode(),
        Some(TranscriptScreenMode::SelectedBlockViewer)
    );
    live.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let mut pager_terminal = FakeTerminal::new();
    let pager_exit = live
        .run_transcript_pager(
            &PagerCommand::new("cat"),
            PagerStdio::capture_with_timeout(Duration::from_secs(1)),
            &mut pager_terminal,
        )
        .expect("live pager should restore terminal state");
    assert_eq!(pager_exit.code, Some(0));
    assert_eq!(
        live.transcript_screen_mode(),
        Some(TranscriptScreenMode::InPlace(
            harness_tui::transcript_identity::InPlaceMode::Transcript
        ))
    );
    replay.ingest_event(user_event(2, "request-two", "replayed second turn"));
    assert_eq!(
        replay
            .transcript_view_model()
            .map(|view| view.identity.turns().len()),
        Some(2)
    );
}

#[test]
fn transcript_parity_owner_reaches_navigation_viewer_pager_selection_and_scroll() {
    // Given: an event-derived composite containing a redacted raw tool block.
    let mut composite = composite_with_raw_tool();
    let view = composite.view().clone();
    let turn_id = view.turns[0].turn_id();
    let block_id = view.blocks[0].id;

    // When: timeline selection, fold, viewer raw mode, pager suspension, and scroll are used.
    composite.select_turn(turn_id).expect("timeline selection");
    composite.toggle_fold(block_id).expect("fold toggle");
    assert_eq!(composite.view().blocks[0].fold_state, FoldState::Expanded);
    composite.open_viewer(block_id).expect("viewer opens");
    assert_eq!(
        composite.viewer().map(|viewer| viewer.mode()),
        Some(harness_tui::transcript_block_viewer::ViewerMode::Wrapped)
    );
    composite
        .viewer_mut()
        .expect("viewer remains live")
        .toggle_mode()
        .expect("raw viewer mode");
    assert_eq!(
        composite.viewer().map(|viewer| viewer.mode()),
        Some(harness_tui::transcript_block_viewer::ViewerMode::Raw)
    );
    composite
        .close_viewer()
        .expect("viewer restores transcript");
    let snapshot = composite.suspend_pager().expect("pager snapshot");
    assert!(snapshot.as_str().contains("tool output"));
    composite
        .restore_pager()
        .expect("pager restores transcript");
    composite
        .scroll_to(0.0, 0, MotionPreference::ReducedMotion)
        .expect("scroll transition");
    composite
        .resize(ratatui::layout::Rect::new(0, 0, 100, 30))
        .expect("resize preserves the composite layout");

    // Then: replay identity-backed selection remains valid after the navigation lifecycle.
    let selection = TranscriptSelection::new(turn_id, block_id, 0..4).expect("valid selection");
    assert!(selection.is_present_in(&composite.view().identity));
    assert_eq!(
        composite.view().screen.mode(),
        TranscriptScreenMode::InPlace(harness_tui::transcript_identity::InPlaceMode::Transcript)
    );
}

#[test]
fn live_transcript_reachability_guards_cover_input_mouse_render_and_pager_adapters() {
    // Given: production source seams that must remain connected to the parity owner.
    // When: this regression guard is compiled with the installed crate sources.
    // Then: removing the live adapter or bypassing its render/input hooks fails this test.
    for (source, marker) in [
        (APP_SOURCE, "TranscriptComposite"),
        (TRANSCRIPT_STATE_SOURCE, "transcript_events_for_activities"),
        (TRANSCRIPT_INTEGRATION_SOURCE, "pub fn suspend_pager"),
        (UI_SOURCE, "render_transcript_pane"),
        (UI_TRANSCRIPT_SOURCE, "app.transcript_view_model()"),
        (RUNTIME_SOURCE, "ui::render_app(frame, &app)"),
    ] {
        assert!(source.contains(marker), "missing production seam: {marker}");
    }
    assert!(TRANSCRIPT_STATE_SOURCE.contains("run_pager"));
    assert!(UI_TRANSCRIPT_SOURCE.contains("transcript_timeline_turn_at"));
    assert!(UI_TRANSCRIPT_SOURCE.contains("transcript_selection_cell"));
    assert!(UI_TRANSCRIPT_SOURCE.contains("transcript_scrollbar_needed"));
    assert!(UI_TRANSCRIPT_SOURCE.contains("transcript_viewer"));
    assert!(TRANSCRIPT_INTEGRATION_SOURCE.contains("ExternalPagerSuspended"));
    assert!(MOUSE_SOURCE.contains("set_transcript_scroll_from_top_with_max"));
    assert!(MOUSE_SOURCE.contains("composite.scroll_to"));
}
