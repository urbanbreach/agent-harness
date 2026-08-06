use std::time::Duration;

use crate::terminal::ResizeEvent;

pub const RESIZE_DEBOUNCE: Duration = Duration::from_millis(16);

#[derive(Debug, Default)]
pub struct ResizeDebouncer {
    pending: Option<(Duration, ResizeEvent)>,
}

impl ResizeDebouncer {
    pub fn push(&mut self, at: Duration, event: ResizeEvent) {
        self.pending = Some((at, event));
    }

    pub fn flush_due(&mut self, at: Duration) -> Option<ResizeEvent> {
        let (started_at, event) = self.pending?;
        if at.saturating_sub(started_at) < RESIZE_DEBOUNCE {
            return None;
        }
        self.pending = None;
        Some(event)
    }
}
