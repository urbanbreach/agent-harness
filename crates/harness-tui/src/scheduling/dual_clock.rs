use harness_core::clock::{Clock, FakeClock};

use super::scheduler::active_animation_period_ms;
use super::FLUSH_DEADLINE_MS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameNow {
    pub animation_ms: u64,
    pub flush_ms: u64,
}

#[derive(Debug, Default)]
pub struct DualClock {
    pub animation: FakeClock,
    pub flush: FakeClock,
}

impl DualClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> FrameNow {
        FrameNow {
            animation_ms: self.animation_now(),
            flush_ms: self.flush_now(),
        }
    }

    pub fn animation_now(&self) -> u64 {
        self.animation.mono_ms()
    }

    pub fn flush_now(&self) -> u64 {
        self.flush.mono_ms()
    }

    pub fn advance_animation(&self, milliseconds: u64) -> u64 {
        self.animation.advance(milliseconds);
        self.animation_now()
    }

    pub fn advance_flush(&self, milliseconds: u64) -> u64 {
        self.flush.advance(milliseconds);
        self.flush_now()
    }

    pub fn tick_animation(&self) -> u64 {
        self.advance_animation(active_animation_period_ms())
    }

    pub fn tick_flush(&self) -> u64 {
        self.advance_flush(FLUSH_DEADLINE_MS)
    }
}
