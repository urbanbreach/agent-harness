use super::decision::{FrameDecision, FrameReason};
use super::scheduler::{duration_millis_ceil, FrameInputs, FrameScheduler};
use super::FrameNow;

impl FrameScheduler {
    pub(super) fn schedule_reduced_motion(
        &mut self,
        now: FrameNow,
        inputs: FrameInputs,
    ) -> Option<FrameDecision> {
        let transition_settled = inputs.motion.cadence().interval().is_some() && self.settled;
        let input_pending = inputs.flush_requested || self.redraw_coalescer.is_pending();
        self.animation_deadline = None;
        self.animation_interval = None;
        self.flush_deadline = None;
        self.until_deadline = inputs.motion.until().map(|remaining| {
            let candidate = now
                .animation_ms
                .saturating_add(duration_millis_ceil(remaining));
            self.until_deadline
                .map_or(candidate, |current| current.min(candidate))
        });
        let until_due = self
            .until_deadline
            .is_some_and(|deadline| now.animation_ms >= deadline);

        if transition_settled || input_pending || until_due {
            if inputs.flush_requested {
                self.redraw_coalescer.request();
            }
            self.redraw_coalescer.take();
            if until_due {
                self.until_deadline = None;
            }
            self.settled = self.until_deadline.is_none();
            return Some(FrameDecision::render(
                self.until_deadline,
                FrameReason::ReducedMotion,
            ));
        }

        self.settled = self.until_deadline.is_none();
        self.until_deadline
            .map(|deadline| FrameDecision::pending(deadline, FrameReason::AnimationPending))
    }

    pub(super) fn next_deadline(&self) -> Option<u64> {
        [
            self.animation_deadline,
            self.until_deadline,
            self.flush_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(super) fn pending_reason(&self) -> FrameReason {
        let animation = match (self.animation_deadline, self.until_deadline) {
            (Some(cadence), Some(until)) => Some(cadence.min(until)),
            (Some(cadence), None) => Some(cadence),
            (None, Some(until)) => Some(until),
            (None, None) => None,
        };
        match (animation, self.flush_deadline) {
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
