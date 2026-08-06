use super::{GestureDevice, FLUSH_INTERVAL_MS, GESTURE_BOUNDARY_MS};

/// The signed vertical direction of a scroll gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollDirection {
    #[default]
    None,
    Up,
    Down,
}

/// A deterministic result from accepting one scroll delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollEmission {
    pub lines: i32,
    pub flush_due: bool,
    pub gesture_reset: bool,
}

/// Accumulates fractional scroll input without losing signed line movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollGesture {
    pub device: GestureDevice,
    pub delta_lines: f64,
    pub fractional_carry: f64,
    pub direction: ScrollDirection,
    generation: u64,
    last_event_ms: Option<u64>,
    last_flush_ms: Option<u64>,
}

impl ScrollGesture {
    pub const fn new(device: GestureDevice) -> Self {
        Self {
            device,
            delta_lines: 0.0,
            fractional_carry: 0.0,
            direction: ScrollDirection::None,
            generation: 0,
            last_event_ms: None,
            last_flush_ms: None,
        }
    }

    /// Accepts a delta and returns the whole signed lines ready for routing.
    pub fn push(&mut self, delta: f64) -> i32 {
        if !delta.is_finite() || delta == 0.0 {
            return 0;
        }

        let direction = if delta.is_sign_positive() {
            ScrollDirection::Up
        } else {
            ScrollDirection::Down
        };
        if self.direction != ScrollDirection::None && self.direction != direction {
            self.reset_for_direction_change();
        }
        self.direction = direction;
        self.delta_lines += delta;
        let (emitted, pending) = whole_lines(self.fractional_carry + delta);
        self.fractional_carry = pending;
        emitted
    }

    /// Accepts a timestamped delta, applying the gesture boundary and flush interval.
    pub fn push_at(&mut self, delta: f64, timestamp_ms: u64) -> ScrollEmission {
        let gesture_reset = self
            .last_event_ms
            .is_some_and(|last| timestamp_ms.saturating_sub(last) > GESTURE_BOUNDARY_MS);
        if gesture_reset {
            self.reset_for_boundary();
        }
        let lines = self.push(delta);
        let flush_due = self.flush_due(timestamp_ms);
        self.last_event_ms = Some(timestamp_ms);
        if self.last_flush_ms.is_none() || flush_due {
            self.last_flush_ms = Some(timestamp_ms);
        }
        ScrollEmission {
            lines,
            flush_due,
            gesture_reset,
        }
    }

    pub const fn fractional_carry(self) -> f64 {
        self.fractional_carry
    }

    pub const fn direction(self) -> ScrollDirection {
        self.direction
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub fn flush_due(&self, timestamp_ms: u64) -> bool {
        self.last_flush_ms
            .is_some_and(|last| timestamp_ms.saturating_sub(last) >= FLUSH_INTERVAL_MS)
    }

    fn reset_for_direction_change(&mut self) {
        self.delta_lines = 0.0;
        self.fractional_carry = 0.0;
        self.generation = self.generation.saturating_add(1);
    }

    fn reset_for_boundary(&mut self) {
        self.delta_lines = 0.0;
        self.fractional_carry = 0.0;
        self.direction = ScrollDirection::None;
        self.generation = self.generation.saturating_add(1);
        self.last_flush_ms = None;
    }
}

#[expect(
    clippy::while_float,
    reason = "fractional carry must emit one signed line per whole threshold"
)]
fn whole_lines(mut pending: f64) -> (i32, f64) {
    let mut emitted = 0_i32;
    if pending >= f64::from(i32::MAX) {
        emitted = i32::MAX;
        pending -= f64::from(i32::MAX);
    } else if pending <= f64::from(i32::MIN) {
        emitted = i32::MIN;
        pending -= f64::from(i32::MIN);
    } else {
        while pending >= 1.0 {
            emitted += 1;
            pending -= 1.0;
        }
        while pending <= -1.0 {
            emitted -= 1;
            pending += 1.0;
        }
    }
    (emitted, pending)
}
