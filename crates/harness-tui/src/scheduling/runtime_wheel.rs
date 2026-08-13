use std::cmp::Ordering;

pub const MAX_WHEEL_STEPS_PER_FLUSH: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDirection {
    Up,
    Down,
}

impl WheelDirection {
    const fn delta(self) -> i16 {
        match self {
            Self::Up => -1,
            Self::Down => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelSample {
    direction: WheelDirection,
    steps: u8,
    column: u16,
    row: u16,
}

impl WheelSample {
    pub const fn new(direction: WheelDirection, column: u16, row: u16) -> Self {
        Self {
            direction,
            steps: 1,
            column,
            row,
        }
    }

    pub const fn logical(direction: WheelDirection, steps: u8, column: u16, row: u16) -> Self {
        Self {
            direction,
            steps,
            column,
            row,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelBatch {
    direction: WheelDirection,
    steps: u8,
    column: u16,
    row: u16,
}

impl WheelBatch {
    pub const fn direction(self) -> WheelDirection {
        self.direction
    }

    pub const fn steps(self) -> u8 {
        self.steps
    }

    pub const fn column(self) -> u16 {
        self.column
    }

    pub const fn row(self) -> u16 {
        self.row
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WheelAccumulator {
    delta: i16,
    column: u16,
    row: u16,
    events_per_step: u8,
}

impl WheelAccumulator {
    pub(super) const fn new(events_per_step: u8) -> Self {
        Self {
            delta: 0,
            column: 0,
            row: 0,
            events_per_step,
        }
    }

    pub(super) fn push(&mut self, sample: WheelSample) {
        let cap =
            i16::from(MAX_WHEEL_STEPS_PER_FLUSH).saturating_mul(i16::from(self.events_per_step));
        self.delta = self
            .delta
            .saturating_add(
                sample
                    .direction
                    .delta()
                    .saturating_mul(i16::from(sample.steps)),
            )
            .clamp(-cap, cap);
        self.column = sample.column;
        self.row = sample.row;
    }

    pub(super) const fn is_pending(self) -> bool {
        self.delta != 0
    }

    pub(super) fn take(&mut self) -> Option<WheelBatch> {
        let delta = std::mem::take(&mut self.delta);
        let direction = match delta.cmp(&0) {
            Ordering::Less => WheelDirection::Up,
            Ordering::Equal => return None,
            Ordering::Greater => WheelDirection::Down,
        };
        let raw_steps = delta.unsigned_abs();
        let divisor = u16::from(self.events_per_step);
        let logical_steps = raw_steps.div_ceil(divisor);
        let steps = u8::try_from(logical_steps)
            .unwrap_or(MAX_WHEEL_STEPS_PER_FLUSH)
            .min(MAX_WHEEL_STEPS_PER_FLUSH);
        Some(WheelBatch {
            direction,
            steps,
            column: self.column,
            row: self.row,
        })
    }
}
