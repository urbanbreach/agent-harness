use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Select};

use crate::input::{TerminalEnvelope, TerminalReaderStatus};
use crate::terminal::{FrameAck, FrameAckOutcome, FrameWriteStage};
use crate::LiveUpdate;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameRuntimeEvent {
    Acknowledged(FrameAck),
    Failed {
        ack: FrameAck,
        stage: FrameWriteStage,
    },
    Disconnected,
}

pub enum RuntimeWake {
    Frame(FrameRuntimeEvent),
    Reader(TerminalReaderStatus),
    ReaderDisconnected,
    Terminal(TerminalEnvelope),
    TerminalDisconnected,
    Live(LiveUpdate),
    LiveDisconnected,
    Deadline,
}

pub struct RuntimeWaitSet<'a> {
    pub frame: &'a Receiver<FrameAck>,
    pub reader: &'a Receiver<TerminalReaderStatus>,
    pub terminal: &'a Receiver<TerminalEnvelope>,
    pub live: Option<&'a Receiver<LiveUpdate>>,
}

impl RuntimeWaitSet<'_> {
    pub fn wait(&self, deadline: Option<Instant>) -> RuntimeWake {
        let timeout = deadline.map(|value| value.saturating_duration_since(Instant::now()));
        let timer = timeout.map(crossbeam_channel::after);
        let mut select = Select::new_biased();
        let frame_index = select.recv(self.frame);
        let reader_index = select.recv(self.reader);
        let terminal_index = select.recv(self.terminal);
        let live_index = self.live.map(|receiver| select.recv(receiver));
        let timer_index = timer.as_ref().map(|receiver| select.recv(receiver));
        let selected = select.select();
        let index = selected.index();
        if index == frame_index {
            return match selected.recv(self.frame) {
                Ok(ack) => RuntimeWake::Frame(frame_event(ack)),
                Err(_) => RuntimeWake::Frame(FrameRuntimeEvent::Disconnected),
            };
        }
        if index == reader_index {
            return match selected.recv(self.reader) {
                Ok(status) => RuntimeWake::Reader(status),
                Err(_) => RuntimeWake::ReaderDisconnected,
            };
        }
        if index == terminal_index {
            return match selected.recv(self.terminal) {
                Ok(event) => RuntimeWake::Terminal(event),
                Err(_) => RuntimeWake::TerminalDisconnected,
            };
        }
        if live_index == Some(index) {
            let Some(receiver) = self.live else {
                return RuntimeWake::LiveDisconnected;
            };
            return match selected.recv(receiver) {
                Ok(update) => RuntimeWake::Live(update),
                Err(_) => RuntimeWake::LiveDisconnected,
            };
        }
        if timer_index == Some(index) {
            if let Some(timer) = timer.as_ref() {
                let _ = selected.recv(timer);
            }
            return RuntimeWake::Deadline;
        }
        RuntimeWake::Deadline
    }

    pub fn wait_for(&self, timeout: Duration) -> RuntimeWake {
        self.wait(Some(Instant::now() + timeout))
    }
}

fn frame_event(ack: FrameAck) -> FrameRuntimeEvent {
    match ack.outcome {
        FrameAckOutcome::Success => FrameRuntimeEvent::Acknowledged(ack),
        FrameAckOutcome::Failure { stage } => FrameRuntimeEvent::Failed { ack, stage },
    }
}
