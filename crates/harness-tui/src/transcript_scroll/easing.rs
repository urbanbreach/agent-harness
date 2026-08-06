use crate::gestures::{GestureDevice, ScrollGesture};

use super::{ScrollError, ScrollResult};

const PAGE_DURATION_MS: u32 = 160;
const JUMP_DURATION_MS: u32 = 220;
const EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPreference {
    Full,
    ReducedMotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingKind {
    Page,
    Jump,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionRequest {
    start: f64,
    target: f64,
    started_at_ms: u64,
    kind: EasingKind,
    motion: MotionPreference,
}

impl TransitionRequest {
    pub const fn new(
        start: f64,
        target: f64,
        started_at_ms: u64,
        kind: EasingKind,
        motion: MotionPreference,
    ) -> Self {
        Self {
            start,
            target,
            started_at_ms,
            kind,
            motion,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollFrame {
    pub value: f64,
    pub settled: bool,
    pub needs_redraw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollTransition {
    start: f64,
    target: f64,
    started_at_ms: u64,
    duration_ms: u32,
    instant: bool,
}

impl ScrollTransition {
    pub fn start(request: TransitionRequest) -> ScrollResult<Self> {
        if !request.start.is_finite() || !request.target.is_finite() {
            return Err(ScrollError::NonFinite("scroll_transition"));
        }
        if request.start < 0.0 || request.target < 0.0 {
            return Err(ScrollError::Negative("scroll_transition"));
        }
        let duration_ms = match request.kind {
            EasingKind::Page => PAGE_DURATION_MS,
            EasingKind::Jump => JUMP_DURATION_MS,
        };
        let instant = matches!(request.motion, MotionPreference::ReducedMotion)
            || (request.target - request.start).abs() <= EPSILON;
        Ok(Self {
            start: request.start,
            target: request.target,
            started_at_ms: request.started_at_ms,
            duration_ms,
            instant,
        })
    }

    pub fn sample(self, now_ms: u64) -> ScrollFrame {
        if self.instant {
            return ScrollFrame {
                value: self.target,
                settled: true,
                needs_redraw: false,
            };
        }
        let elapsed = now_ms.saturating_sub(self.started_at_ms);
        if elapsed >= u64::from(self.duration_ms) {
            return ScrollFrame {
                value: self.target,
                settled: true,
                needs_redraw: false,
            };
        }
        let elapsed_ms = u32::try_from(elapsed).unwrap_or(u32::MAX);
        let progress = f64::from(elapsed_ms) / f64::from(self.duration_ms);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        ScrollFrame {
            value: self.start + (self.target - self.start) * eased,
            settled: false,
            needs_redraw: true,
        }
    }

    pub const fn duration_ms(self) -> u32 {
        self.duration_ms
    }

    pub fn deadline_ms(self) -> Option<u64> {
        if self.instant {
            None
        } else {
            Some(
                self.started_at_ms
                    .saturating_add(u64::from(self.duration_ms)),
            )
        }
    }
}

#[derive(Debug)]
pub struct FractionalScroll {
    gesture: ScrollGesture,
}

impl FractionalScroll {
    pub fn new() -> Self {
        Self::with_device(GestureDevice::Trackpad)
    }

    pub fn with_device(device: GestureDevice) -> Self {
        Self {
            gesture: ScrollGesture::new(device),
        }
    }

    pub fn push(&mut self, delta: f64) -> i32 {
        self.gesture.push(delta)
    }

    pub fn fractional_carry(&self) -> f64 {
        self.gesture.fractional_carry()
    }

    pub fn generation(&self) -> u64 {
        self.gesture.generation()
    }
}

impl Default for FractionalScroll {
    fn default() -> Self {
        Self::new()
    }
}
