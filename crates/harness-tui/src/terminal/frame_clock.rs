//! Deterministic frame-timing clock for the terminal lifecycle shard.
//!
//! Pure value object over [`harness_core::clock::FakeClock`]. Each tick adds a
//! fixed number of milliseconds to the underlying monotonic clock and records
//! one [`FramePhase`], so frame-timing evidence is byte-stable across runs.

use std::sync::Arc;

use harness_core::clock::{Clock, FakeClock};

/// Default per-tick step, matching the animation fixed-tick evidence cadence.
pub const DEFAULT_FRAME_TICK_MS: u64 = 1_000 / 30;

/// A frame count, type-distinct from a millisecond reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FramePhase(pub u64);

impl FramePhase {
    /// The underlying frame count.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A deterministic frame clock backed by a manually advanced [`FakeClock`].
///
/// The backing clock is shared via [`Arc`] so the same reading can be handed to
/// other deterministic consumers (e.g. animation-evidence capture) without
/// copying.
#[derive(Debug, Clone)]
pub struct FrameClock {
    clock: Arc<FakeClock>,
    tick_ms: u64,
    phase: FramePhase,
}

impl FrameClock {
    /// A new frame clock starting at `mono_ms == 0` with the default tick step.
    pub fn new() -> Self {
        Self {
            clock: Arc::new(FakeClock::new()),
            tick_ms: DEFAULT_FRAME_TICK_MS,
            phase: FramePhase(0),
        }
    }

    /// Wrap an existing fake clock (sharing its `mono_ms`) with the default
    /// tick step.
    pub fn from_clock(clock: Arc<FakeClock>) -> Self {
        Self {
            clock,
            tick_ms: DEFAULT_FRAME_TICK_MS,
            phase: FramePhase(0),
        }
    }

    /// Override the per-tick step in milliseconds.
    pub fn with_tick_ms(mut self, tick_ms: u64) -> Self {
        self.tick_ms = tick_ms;
        self
    }

    /// Current `mono_ms` reading of the backing clock.
    pub fn mono_ms(&self) -> u64 {
        self.clock.mono_ms()
    }

    /// Frames advanced so far.
    pub const fn phase(&self) -> FramePhase {
        self.phase
    }

    /// The configured per-tick step.
    pub const fn tick_ms(&self) -> u64 {
        self.tick_ms
    }

    /// A shared handle to the backing clock.
    pub fn clock(&self) -> Arc<FakeClock> {
        Arc::clone(&self.clock)
    }

    /// Advance one frame: add one tick step and record one phase.
    pub fn tick(&mut self) {
        self.advance(self.tick_ms);
    }

    /// Advance `frames` consecutive frames, each adding one tick step.
    pub fn tick_n(&mut self, frames: u64) {
        for _ in 0..frames {
            self.tick();
        }
    }

    /// Advance the backing clock by an explicit number of milliseconds and
    /// record one phase step. Saturates rather than wrapping.
    pub fn advance(&mut self, ms: u64) {
        self.clock.advance(ms);
        self.phase = FramePhase(self.phase.0.saturating_add(1));
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}
