use std::time::{Duration, Instant};

use crossbeam_channel::bounded;
use crossterm::event::Event;
use harness_tui::event::TuiEvent;
use harness_tui::input::{TerminalEnvelope, TerminalReaderStatus, TerminalSequence};
use harness_tui::runtime_wait_set::{RuntimeWaitSet, RuntimeWake};
use harness_tui::{live_update_channel, LiveUpdate};

#[test]
fn terminal_and_live_sources_wake_parked_wait() {
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (terminal_tx, terminal_rx) = bounded(1);
    let (live_tx, live_rx) = live_update_channel();
    terminal_tx
        .send(TerminalEnvelope::new(
            TerminalSequence::new(1),
            Instant::now(),
            TuiEvent::FocusGained,
        ))
        .expect("terminal send");
    let wait = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: Some(live_rx.receiver()),
    };
    assert!(matches!(wait.wait(None), RuntimeWake::Terminal(_)));
    live_tx
        .send(LiveUpdate::Status("ready".to_string()))
        .expect("live send");
    assert!(matches!(wait.wait(None), RuntimeWake::Live(_)));
}

#[test]
fn deadline_wakes_without_periodic_polling() {
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (_terminal_tx, terminal_rx) = bounded(1);
    let (_live_tx, live_rx) = live_update_channel();
    let wait = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: Some(live_rx.receiver()),
    };
    assert!(matches!(
        wait.wait(Some(Instant::now() + Duration::from_millis(1))),
        RuntimeWake::Deadline
    ));
}

#[test]
fn live_disconnect_is_reported_once() {
    let (_frame_tx, frame_rx) = bounded(1);
    let (_status_tx, status_rx) = bounded::<TerminalReaderStatus>(1);
    let (terminal_tx, terminal_rx) = bounded(1);
    let (live_tx, live_rx) = live_update_channel();
    drop(live_tx);
    let with_live = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: Some(live_rx.receiver()),
    };
    assert!(matches!(
        with_live.wait(None),
        RuntimeWake::LiveDisconnected
    ));

    terminal_tx
        .send(TerminalEnvelope::new(
            TerminalSequence::new(1),
            Instant::now(),
            TuiEvent::FocusLost,
        ))
        .expect("terminal send");
    let without_live = RuntimeWaitSet {
        frame: &frame_rx,
        reader: &status_rx,
        terminal: &terminal_rx,
        live: None,
    };
    assert!(matches!(without_live.wait(None), RuntimeWake::Terminal(_)));
}
