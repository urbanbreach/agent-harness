use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender};
use crossterm::event::Event;
use thiserror::Error;

use super::{TerminalEnvelope, TerminalQueue, TerminalSequence};
use crate::event::normalize_event;

const READER_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub trait TerminalEventSource: Send + 'static {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    fn read(&mut self) -> io::Result<Event>;
}

pub struct CrosstermEventSource;

impl TerminalEventSource for CrosstermEventSource {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        crossterm::event::poll(timeout)
    }

    fn read(&mut self) -> io::Result<Event> {
        crossterm::event::read()
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TerminalReaderError {
    #[error("terminal event poll failed: {0}")]
    Poll(String),
    #[error("terminal event read failed: {0}")]
    Read(String),
    #[error("terminal ingress reader panicked")]
    Panicked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalReaderStatus {
    Stopped,
    Failed(TerminalReaderError),
}

pub struct TerminalIngress {
    pub queue: TerminalQueue,
    pub status: Receiver<TerminalReaderStatus>,
}

pub struct TerminalIngressReader {
    stop: Sender<()>,
    join: Option<JoinHandle<()>>,
}

impl TerminalIngressReader {
    pub fn spawn(capacity: usize) -> (Self, TerminalIngress) {
        Self::spawn_with_source(capacity, CrosstermEventSource)
    }

    pub fn spawn_with_source<S: TerminalEventSource>(
        capacity: usize,
        source: S,
    ) -> (Self, TerminalIngress) {
        let (events_tx, events_rx) = bounded(capacity);
        let (status_tx, status_rx) = bounded(1);
        let (stop_tx, stop_rx) = bounded(1);
        let join = thread::spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                run_reader(source, &events_tx, &status_tx, &stop_rx)
            }));
            if result.is_err() {
                let _ =
                    status_tx.try_send(TerminalReaderStatus::Failed(TerminalReaderError::Panicked));
            }
        });
        (
            Self {
                stop: stop_tx,
                join: Some(join),
            },
            TerminalIngress {
                queue: TerminalQueue::new(events_rx),
                status: status_rx,
            },
        )
    }

    pub fn stop_and_join(mut self) -> thread::Result<()> {
        let _ = self.stop.try_send(());
        match self.join.take() {
            Some(join) => join.join(),
            None => Ok(()),
        }
    }
}

fn run_reader<S: TerminalEventSource>(
    mut source: S,
    events: &Sender<TerminalEnvelope>,
    status: &Sender<TerminalReaderStatus>,
    stop: &Receiver<()>,
) {
    let mut sequence = 1_u64;
    loop {
        if stop.try_recv().is_ok() {
            let _ = status.try_send(TerminalReaderStatus::Stopped);
            return;
        }
        let ready = match source.poll(READER_POLL_INTERVAL) {
            Ok(ready) => ready,
            Err(error) => {
                let _ = status.try_send(TerminalReaderStatus::Failed(TerminalReaderError::Poll(
                    error.to_string(),
                )));
                return;
            }
        };
        if !ready {
            continue;
        }
        let raw = match source.read() {
            Ok(event) => event,
            Err(error) => {
                let _ = status.try_send(TerminalReaderStatus::Failed(TerminalReaderError::Read(
                    error.to_string(),
                )));
                return;
            }
        };
        let Some(event) = normalize_event(raw) else {
            continue;
        };
        let envelope =
            TerminalEnvelope::new(TerminalSequence::new(sequence), Instant::now(), event);
        sequence = sequence.saturating_add(1);
        crossbeam_channel::select_biased! {
            recv(stop) -> _ => {
                let _ = status.try_send(TerminalReaderStatus::Stopped);
                return;
            }
            send(events, envelope) -> result => {
                if result.is_err() {
                    return;
                }
            }
        }
    }
}
