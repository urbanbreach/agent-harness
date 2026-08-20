use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use harness_tui::event::TuiEvent;
use harness_tui::input::{
    TerminalEnvelope, TerminalEventSource, TerminalIngressReader, TerminalQueue,
    TerminalReaderError, TerminalReaderStatus, TerminalSequence,
};

struct FakeSource {
    events: VecDeque<io::Result<Event>>,
}

impl TerminalEventSource for FakeSource {
    fn poll(&mut self, _timeout: Duration) -> io::Result<bool> {
        Ok(!self.events.is_empty())
    }

    fn read(&mut self) -> io::Result<Event> {
        self.events
            .pop_front()
            .unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty fake")))
    }
}

fn key(character: char, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(character),
        modifiers: KeyModifiers::NONE,
        kind,
        state: KeyEventState::NONE,
    })
}

#[test]
fn ordered_input_filters_non_press_keys_and_preserves_focus_and_paste() {
    // arrange
    // act
    let source = FakeSource {
        events: VecDeque::from([
            Ok(key('x', KeyEventKind::Release)),
            Ok(key('a', KeyEventKind::Press)),
            Ok(Event::Paste("body".to_string())),
            Ok(Event::FocusLost),
        ]),
    };
    let (reader, mut ingress) = TerminalIngressReader::spawn_with_source(128, source);
    let deadline = Instant::now() + Duration::from_millis(50);
    while ingress.queue.receiver().len() < 3 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let first = ingress.queue.try_recv().expect("key");
    let second = ingress.queue.try_recv().expect("paste");
    let third = ingress.queue.try_recv().expect("focus");
    // assert
    assert_eq!(first.sequence.get(), 1);
    assert!(matches!(first.event, TuiEvent::Key(_)));
    assert!(matches!(second.event, TuiEvent::Paste(ref text) if text == "body"));
    assert!(matches!(third.event, TuiEvent::FocusLost));
    reader.stop_and_join().expect("reader joins");
}

#[test]
fn terminal_queue_backpressures_without_drop_and_shutdown_interrupts_full_send() {
    // arrange
    // act
    let events = (0..129)
        .map(|_| Ok(key('x', KeyEventKind::Press)))
        .collect();
    let source = FakeSource { events };
    let (reader, mut ingress) = TerminalIngressReader::spawn_with_source(128, source);
    let deadline = Instant::now() + Duration::from_millis(50);
    while ingress.queue.receiver().len() < 128 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    // assert
    assert_eq!(ingress.queue.receiver().len(), 128);
    let started = Instant::now();
    reader.stop_and_join().expect("reader joins");
    assert!(started.elapsed() < Duration::from_millis(100));
    let sequences: Vec<_> = std::iter::from_fn(|| ingress.queue.try_recv().ok())
        .flat_map(|event| event.source_sequences)
        .map(|sequence| sequence.get())
        .collect();
    assert_eq!(sequences, (1..=128).collect::<Vec<_>>());
}

#[test]
fn reader_failure_is_typed_and_lossless() {
    // arrange
    // act
    let source = FakeSource {
        events: VecDeque::from([Err(io::Error::other("read defect"))]),
    };
    let (reader, ingress) = TerminalIngressReader::spawn_with_source(128, source);
    let status = ingress
        .status
        .recv_timeout(Duration::from_millis(50))
        .expect("typed status");
    // assert
    assert_eq!(
        status,
        TerminalReaderStatus::Failed(TerminalReaderError::Read("read defect".to_string()))
    );
    reader.stop_and_join().expect("reader joins");
}

#[test]
fn adjacent_resize_coalescing_retains_every_sequence_and_stops_at_focus() {
    // arrange
    // act
    let (sender, receiver) = crossbeam_channel::bounded(4);
    for (sequence, event) in [
        (1, TuiEvent::Resize(80, 24)),
        (2, TuiEvent::Resize(120, 40)),
        (3, TuiEvent::FocusGained),
    ] {
        sender
            .send(TerminalEnvelope::new(
                TerminalSequence::new(sequence),
                Instant::now(),
                event,
            ))
            .expect("queue send");
    }
    let mut queue = TerminalQueue::new(receiver);
    let resize = queue.try_recv().expect("resize");
    // assert
    assert_eq!(
        resize
            .source_sequences
            .iter()
            .map(|sequence| sequence.get())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(matches!(resize.event, TuiEvent::Resize(120, 40)));
    assert!(matches!(
        queue.try_recv().expect("focus").event,
        TuiEvent::FocusGained
    ));
}
