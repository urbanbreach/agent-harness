use crate::theme_tokens::{MotionKind, DESIGN_TOKENS};

use super::coalesce::RedrawCoalescer;
use super::decision::{FrameDecision, FrameReason};
use super::frame_cadence::clamp_flush_interval_ms;
use super::{FrameNow, MotionPlan, ANIMATION_PERIOD_MS, FLUSH_DEADLINE_MS};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInputs {
    pub motion: MotionPlan,
    pub flush_requested: bool,
}

impl FrameInputs {
    pub const fn idle() -> Self {
        Self {
            motion: MotionPlan::none(),
            flush_requested: false,
        }
    }

    pub const fn active() -> Self {
        Self {
            motion: MotionPlan::from_demand(super::MotionDemand::fast(
                std::time::Duration::from_millis(ANIMATION_PERIOD_MS),
            )),
            flush_requested: false,
        }
    }

    pub const fn flush() -> Self {
        Self {
            motion: MotionPlan::none(),
            flush_requested: true,
        }
    }

    pub const fn active_and_flush() -> Self {
        Self {
            motion: MotionPlan::from_demand(super::MotionDemand::fast(
                std::time::Duration::from_millis(ANIMATION_PERIOD_MS),
            )),
            flush_requested: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameScheduler {
    pub(super) animation_deadline: Option<u64>,
    pub(super) animation_interval: Option<u64>,
    pub(super) until_deadline: Option<u64>,
    pub(super) flush_deadline: Option<u64>,
    pub(super) redraw_coalescer: RedrawCoalescer,
    pub(super) flush_interval_ms: u64,
    pub(super) settled: bool,
    pub(super) reduced_motion: bool,
}

impl FrameScheduler {
    pub const fn new() -> Self {
        Self {
            animation_deadline: None,
            animation_interval: None,
            until_deadline: None,
            flush_deadline: None,
            redraw_coalescer: RedrawCoalescer::new(),
            flush_interval_ms: FLUSH_DEADLINE_MS,
            settled: true,
            reduced_motion: false,
        }
    }

    pub const fn with_reduced_motion(reduced_motion: bool) -> Self {
        Self {
            reduced_motion,
            ..Self::new()
        }
    }

    pub const fn with_flush_interval_ms(flush_interval_ms: u64) -> Self {
        Self {
            flush_interval_ms: clamp_flush_interval_ms(flush_interval_ms),
            ..Self::new()
        }
    }

    pub const fn with_reduced_motion_and_flush_interval_ms(
        reduced_motion: bool,
        flush_interval_ms: u64,
    ) -> Self {
        Self {
            reduced_motion,
            flush_interval_ms: clamp_flush_interval_ms(flush_interval_ms),
            ..Self::new()
        }
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
        if reduced_motion {
            self.animation_deadline = None;
            self.animation_interval = None;
            self.until_deadline = None;
            self.flush_deadline = None;
            self.settled = true;
        }
    }

    pub const fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    pub const fn settled(&self) -> bool {
        self.settled
    }

    pub const fn animation_deadline(&self) -> Option<u64> {
        match (self.animation_deadline, self.until_deadline) {
            (Some(cadence), Some(until)) => Some(if cadence <= until { cadence } else { until }),
            (Some(cadence), None) => Some(cadence),
            (None, Some(until)) => Some(until),
            (None, None) => None,
        }
    }

    pub const fn flush_deadline(&self) -> Option<u64> {
        self.flush_deadline
    }

    pub fn cancel_periodic_motion(&mut self) {
        self.animation_deadline = None;
        self.animation_interval = None;
        self.settled = self.until_deadline.is_none() && self.flush_deadline.is_none();
    }

    pub fn schedule(&mut self, now: FrameNow, inputs: FrameInputs) -> Option<FrameDecision> {
        if self.reduced_motion {
            return self.schedule_reduced_motion(now, inputs);
        }

        let animation_interval = inputs.motion.cadence().interval().map(duration_millis_ceil);
        if let Some(interval) = animation_interval {
            self.settled = false;
            if self.animation_interval != Some(interval) {
                self.animation_deadline = Some(now.animation_ms.saturating_add(interval));
            } else {
                self.animation_deadline
                    .get_or_insert(now.animation_ms.saturating_add(interval));
            }
            self.animation_interval = Some(interval);
        } else {
            self.animation_deadline = None;
            self.animation_interval = None;
        }

        self.until_deadline = match inputs.motion.until() {
            Some(remaining) => {
                let candidate = now
                    .animation_ms
                    .saturating_add(duration_millis_ceil(remaining));
                Some(
                    self.until_deadline
                        .map_or(candidate, |current| current.min(candidate)),
                )
            }
            None => None,
        };

        if inputs.flush_requested {
            self.settled = false;
            self.redraw_coalescer.request();
            self.flush_deadline
                .get_or_insert(now.flush_ms.saturating_add(self.flush_interval_ms));
        }

        let cadence_due = self
            .animation_deadline
            .is_some_and(|deadline| now.animation_ms >= deadline);
        let until_due = self
            .until_deadline
            .is_some_and(|deadline| now.animation_ms >= deadline);
        let animation_due = cadence_due || until_due;
        let flush_due = self.redraw_coalescer.is_pending()
            && self
                .flush_deadline
                .is_some_and(|deadline| now.flush_ms >= deadline);

        if animation_due || flush_due {
            let reason = match (animation_due, flush_due) {
                (true, true) => FrameReason::AnimationAndFlush,
                (true, false) => FrameReason::Animation,
                (false, true) => FrameReason::Flush,
                (false, false) => return None,
            };

            if cadence_due {
                self.animation_deadline = self
                    .animation_interval
                    .map(|interval| now.animation_ms.saturating_add(interval));
            }
            if until_due {
                self.until_deadline = None;
            }
            if flush_due {
                self.flush_deadline = None;
                self.redraw_coalescer.take();
            }

            let deadline_ms = self.next_deadline();
            self.settled = deadline_ms.is_none() && inputs.motion.is_none();
            return Some(FrameDecision::render(deadline_ms, reason));
        }

        let Some(deadline_ms) = self.next_deadline() else {
            self.settled = true;
            return None;
        };

        self.settled = false;
        Some(FrameDecision::pending(deadline_ms, self.pending_reason()))
    }
}

pub(super) fn duration_millis_ceil(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX)
}

impl Default for FrameScheduler {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn active_animation_period_ms() -> u64 {
    DESIGN_TOKENS
        .motion_tokens
        .all
        .iter()
        .find(|token| token.kind == MotionKind::ActiveTick)
        .map_or(ANIMATION_PERIOD_MS, |token| u64::from(token.interval_ms))
}
