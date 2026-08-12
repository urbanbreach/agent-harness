use std::collections::VecDeque;
use std::time::Instant;

use crossbeam_channel::{Receiver, TryRecvError};
use crossterm::event::MouseEventKind;

use crate::event::TuiEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerminalSequence(u64);

impl TerminalSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

pub struct TerminalEnvelope {
    pub sequence: TerminalSequence,
    pub source_sequences: Vec<TerminalSequence>,
    pub received_at: Instant,
    pub event: TuiEvent,
}

impl TerminalEnvelope {
    pub fn new(sequence: TerminalSequence, received_at: Instant, event: TuiEvent) -> Self {
        Self {
            sequence,
            source_sequences: vec![sequence],
            received_at,
            event,
        }
    }
}

pub struct TerminalQueue {
    receiver: Receiver<TerminalEnvelope>,
    pending: VecDeque<TerminalEnvelope>,
}

impl TerminalQueue {
    pub fn new(receiver: Receiver<TerminalEnvelope>) -> Self {
        Self {
            receiver,
            pending: VecDeque::new(),
        }
    }

    pub fn receiver(&self) -> &Receiver<TerminalEnvelope> {
        &self.receiver
    }

    pub fn try_recv(&mut self) -> Result<TerminalEnvelope, TryRecvError> {
        let first = match self.pending.pop_front() {
            Some(event) => event,
            None => self.receiver.try_recv()?,
        };
        Ok(self.coalesce(first))
    }

    fn coalesce(&mut self, mut current: TerminalEnvelope) -> TerminalEnvelope {
        while let Ok(next) = self.receiver.try_recv() {
            if events_coalesce(&current.event, &next.event) {
                current.sequence = next.sequence;
                current.received_at = next.received_at;
                current.source_sequences.extend(next.source_sequences);
                current.event = next.event;
            } else {
                self.pending.push_back(next);
                break;
            }
        }
        current
    }
}

fn events_coalesce(current: &TuiEvent, next: &TuiEvent) -> bool {
    match (current, next) {
        (TuiEvent::Resize(_, _), TuiEvent::Resize(_, _)) => true,
        (TuiEvent::Mouse(left), TuiEvent::Mouse(right)) => match (left.kind, right.kind) {
            (MouseEventKind::Moved, MouseEventKind::Moved) => true,
            (MouseEventKind::Drag(left), MouseEventKind::Drag(right)) => left == right,
            _ => false,
        },
        _ => false,
    }
}
