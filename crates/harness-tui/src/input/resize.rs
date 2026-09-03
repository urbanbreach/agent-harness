use std::time::{Duration, Instant};

use crate::event::TuiEvent;
use crate::terminal::ResizeEvent;

use super::{TerminalEnvelope, TerminalQueue};

pub const RESIZE_DEBOUNCE: Duration = Duration::from_millis(16);

#[derive(Default)]
pub(crate) struct RuntimeInputIngress {
    resize: ResizeDebouncer<TerminalEnvelope>,
}

impl RuntimeInputIngress {
    pub(crate) fn ingest_at(
        &mut self,
        at: Duration,
        envelope: TerminalEnvelope,
    ) -> Option<TerminalEnvelope> {
        match &envelope.event {
            TuiEvent::Resize(_, _) => {
                self.resize.push(at, envelope);
                None
            }
            TuiEvent::Key(_)
            | TuiEvent::Mouse(_)
            | TuiEvent::Paste(_)
            | TuiEvent::FocusGained
            | TuiEvent::FocusLost => Some(envelope),
        }
    }

    pub(crate) fn take_ready(
        &mut self,
        queue: &mut TerminalQueue,
        epoch: Instant,
        now: Instant,
    ) -> Option<TerminalEnvelope> {
        // A due resize outranks queued input: without this check first, a
        // sustained input stream would starve the resize indefinitely.
        let elapsed = now.saturating_duration_since(epoch);
        if let Some(due) = self.resize.flush_due(elapsed) {
            return Some(due);
        }
        while let Ok(envelope) = queue.try_recv() {
            let received_at = envelope.received_at.saturating_duration_since(epoch);
            if let Some(ready) = self.ingest_at(received_at, envelope) {
                return Some(ready);
            }
        }
        self.flush_due(now.saturating_duration_since(epoch))
    }

    pub(crate) fn flush_due(&mut self, at: Duration) -> Option<TerminalEnvelope> {
        self.resize.flush_due(at)
    }

    pub(crate) fn deadline(&self) -> Option<Duration> {
        self.resize.deadline()
    }
}

#[derive(Debug)]
pub struct ResizeDebouncer<T = ResizeEvent> {
    pending: Option<(Duration, T)>,
}

impl<T> Default for ResizeDebouncer<T> {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl<T> ResizeDebouncer<T> {
    /// Replaces the pending payload but keeps the window anchored to the
    /// first unseen event, so a storm faster than the debounce still
    /// flushes at the first event's quiet boundary.
    pub fn push(&mut self, at: Duration, event: T) {
        match &mut self.pending {
            Some((_, pending)) => *pending = event,
            None => self.pending = Some((at, event)),
        }
    }

    pub fn flush_due(&mut self, at: Duration) -> Option<T> {
        let (started_at, _) = self.pending.as_ref()?;
        if at.saturating_sub(*started_at) < RESIZE_DEBOUNCE {
            return None;
        }
        self.pending.take().map(|(_, event)| event)
    }

    pub fn deadline(&self) -> Option<Duration> {
        self.pending
            .as_ref()
            .map(|(started_at, _)| started_at.saturating_add(RESIZE_DEBOUNCE))
    }
}
