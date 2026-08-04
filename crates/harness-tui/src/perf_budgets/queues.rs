#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureDecision {
    Accept,
    Drop,
    Throttle(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueBounds {
    pub max_pending_frames: u16,
    pub max_input_events: u16,
    pub max_render_commands: u32,
    pub max_concurrent_workers: u8,
}

impl QueueBounds {
    pub fn defaults() -> Self {
        Self {
            max_pending_frames: 2,
            max_input_events: 256,
            max_render_commands: 8192,
            max_concurrent_workers: 2,
        }
    }
    pub fn strict() -> Self {
        Self {
            max_pending_frames: 1,
            max_input_events: 128,
            max_render_commands: 4096,
            max_concurrent_workers: 1,
        }
    }
    pub fn relaxed() -> Self {
        Self {
            max_pending_frames: 4,
            max_input_events: 512,
            max_render_commands: 16384,
            max_concurrent_workers: 4,
        }
    }

    pub fn decide(self, current_pending: u16, current_events: u16) -> BackpressureDecision {
        if current_pending >= self.max_pending_frames {
            BackpressureDecision::Drop
        } else if u32::from(current_events) >= u32::from(self.max_input_events) * 8 / 10 {
            BackpressureDecision::Throttle(16)
        } else {
            BackpressureDecision::Accept
        }
    }

    pub fn is_within_bounds(self, pending: u16, events: u16, workers: u8) -> bool {
        pending <= self.max_pending_frames
            && events <= self.max_input_events
            && workers <= self.max_concurrent_workers
    }
}
