use crate::design_contract::{MotionKind, DESIGN_TOKENS};

use super::coalesce::RedrawCoalescer;
use super::decision::{FrameDecision, FrameReason};
use super::{FrameNow, ANIMATION_PERIOD_MS, FLUSH_DEADLINE_MS};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInputs {
    pub animation_active: bool,
    pub flush_requested: bool,
}

impl FrameInputs {
    pub const fn idle() -> Self {
        Self {
            animation_active: false,
            flush_requested: false,
        }
    }

    pub const fn active() -> Self {
        Self {
            animation_active: true,
            flush_requested: false,
        }
    }

    pub const fn flush() -> Self {
        Self {
            animation_active: false,
            flush_requested: true,
        }
    }

    pub const fn active_and_flush() -> Self {
        Self {
            animation_active: true,
            flush_requested: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameScheduler {
    animation_deadline: Option<u64>,
    flush_deadline: Option<u64>,
    redraw_coalescer: RedrawCoalescer,
    settled: bool,
    reduced_motion: bool,
}

impl FrameScheduler {
    pub const fn new() -> Self {
        Self {
            animation_deadline: None,
            flush_deadline: None,
            redraw_coalescer: RedrawCoalescer::new(),
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

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
        if reduced_motion {
            self.animation_deadline = None;
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
        self.animation_deadline
    }

    pub const fn flush_deadline(&self) -> Option<u64> {
        self.flush_deadline
    }

    pub fn schedule(&mut self, now: FrameNow, inputs: FrameInputs) -> Option<FrameDecision> {
        if self.reduced_motion {
            return self.schedule_reduced_motion(inputs);
        }

        if inputs.animation_active {
            self.settled = false;
            self.animation_deadline.get_or_insert_with(|| {
                now.animation_ms
                    .saturating_add(active_animation_period_ms())
            });
        } else {
            self.animation_deadline = None;
        }

        if inputs.flush_requested {
            self.settled = false;
            self.redraw_coalescer.request();
            self.flush_deadline
                .get_or_insert(now.flush_ms.saturating_add(FLUSH_DEADLINE_MS));
        }

        let animation_due = inputs.animation_active
            && self
                .animation_deadline
                .is_some_and(|deadline| now.animation_ms >= deadline);
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

            if animation_due {
                self.animation_deadline = Some(
                    now.animation_ms
                        .saturating_add(active_animation_period_ms()),
                );
            }
            if flush_due {
                self.flush_deadline = None;
                self.redraw_coalescer.take();
            }

            let deadline_ms = self.next_deadline();
            self.settled = deadline_ms.is_none() && !inputs.animation_active;
            return Some(FrameDecision::render(deadline_ms, reason));
        }

        let Some(deadline_ms) = self.next_deadline() else {
            self.settled = true;
            return None;
        };

        self.settled = false;
        Some(FrameDecision::pending(deadline_ms, self.pending_reason()))
    }

    fn schedule_reduced_motion(&mut self, inputs: FrameInputs) -> Option<FrameDecision> {
        let transition_settled = inputs.animation_active && self.settled;
        let input_pending = inputs.flush_requested || self.redraw_coalescer.is_pending();
        self.animation_deadline = None;
        self.flush_deadline = None;

        if transition_settled || input_pending {
            if inputs.flush_requested {
                self.redraw_coalescer.request();
            }
            self.redraw_coalescer.take();
            self.settled = true;
            return Some(FrameDecision::render(None, FrameReason::ReducedMotion));
        }

        self.settled = true;
        None
    }

    fn next_deadline(&self) -> Option<u64> {
        match (self.animation_deadline, self.flush_deadline) {
            (Some(animation), Some(flush)) => Some(animation.min(flush)),
            (Some(animation), None) => Some(animation),
            (None, Some(flush)) => Some(flush),
            (None, None) => None,
        }
    }

    fn pending_reason(&self) -> FrameReason {
        match (self.animation_deadline, self.flush_deadline) {
            (Some(animation), Some(flush)) if animation == flush => {
                FrameReason::AnimationAndFlushPending
            }
            (Some(animation), Some(flush)) if animation < flush => FrameReason::AnimationPending,
            (Some(_), Some(_)) => FrameReason::FlushPending,
            (Some(_), None) => FrameReason::AnimationPending,
            (None, Some(_)) => FrameReason::FlushPending,
            (None, None) => FrameReason::ReducedMotion,
        }
    }
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
