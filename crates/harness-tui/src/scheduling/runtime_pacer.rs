use crate::terminal::FrameSubmission;
use crate::terminal::{TerminalMultiplexer, TerminalName};

use super::runtime_wheel::{WheelAccumulator, WheelBatch, WheelSample};
use super::{FrameInputs, FrameNow, FrameReason, FrameScheduler, MotionPlan};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimePacerAction {
    pub paint: bool,
    pub advance_animation: bool,
    pub wheel_batch: Option<WheelBatch>,
    pub next_wait_ms: Option<u64>,
}

impl RuntimePacerAction {
    pub const fn should_paint(self, wheel_changed: bool) -> bool {
        self.paint || wheel_changed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePacer {
    scheduler: FrameScheduler,
    flush_requested: bool,
    wheel: WheelAccumulator,
    suppressed_periodic: Option<(super::MotionCadence, u64, u64)>,
}

impl RuntimePacer {
    pub const fn new() -> Self {
        Self::with_reduced_motion(false)
    }

    pub const fn with_reduced_motion(reduced_motion: bool) -> Self {
        Self::with_wheel_events_per_step(reduced_motion, 1, super::FLUSH_DEADLINE_MS)
    }

    pub const fn with_reduced_motion_and_flush_interval_ms(
        reduced_motion: bool,
        flush_interval_ms: u64,
    ) -> Self {
        Self::with_wheel_events_per_step(reduced_motion, 1, flush_interval_ms)
    }

    pub const fn with_terminal_wheel_profile(
        reduced_motion: bool,
        terminal: TerminalName,
        multiplexer: TerminalMultiplexer,
    ) -> Self {
        Self::with_wheel_events_per_step(
            reduced_motion,
            terminal_wheel_events_per_step(terminal, multiplexer),
            super::FLUSH_DEADLINE_MS,
        )
    }

    const fn with_wheel_events_per_step(
        reduced_motion: bool,
        events_per_step: u8,
        flush_interval_ms: u64,
    ) -> Self {
        Self {
            scheduler: FrameScheduler::with_reduced_motion_and_flush_interval_ms(
                reduced_motion,
                flush_interval_ms,
            ),
            flush_requested: false,
            wheel: WheelAccumulator::new(events_per_step),
            suppressed_periodic: None,
        }
    }

    pub fn request_flush(&mut self) {
        self.flush_requested = true;
    }

    pub fn queue_wheel(&mut self, sample: WheelSample) {
        self.wheel.push(sample);
    }

    pub fn poll(&mut self, now: FrameNow, motion: impl Into<MotionPlan>) -> RuntimePacerAction {
        let motion = motion.into();
        let effective_motion = self.effective_motion(motion);
        let flush_pending = self.flush_requested || self.wheel.is_pending();
        let decision = self.scheduler.schedule(
            now,
            FrameInputs {
                motion: effective_motion,
                flush_requested: flush_pending,
            },
        );

        let mut action = RuntimePacerAction::default();
        if let Some(decision) = decision.filter(|decision| decision.render) {
            match decision.reason {
                FrameReason::Animation => action.advance_animation = true,
                FrameReason::Flush => self.release_flush(&mut action),
                FrameReason::AnimationAndFlush => {
                    action.advance_animation = true;
                    self.release_flush(&mut action);
                }
                FrameReason::ReducedMotion => {
                    action.advance_animation = !effective_motion.is_none();
                    self.release_flush(&mut action);
                }
                FrameReason::AnimationPending
                | FrameReason::FlushPending
                | FrameReason::AnimationAndFlushPending => {}
            }
        }
        action.paint |= action.advance_animation;
        action.next_wait_ms = self.next_wait_ms(now);
        action
    }

    pub fn next_wait_ms(&self, now: FrameNow) -> Option<u64> {
        let animation_wait = self
            .scheduler
            .animation_deadline()
            .map(|deadline| deadline.saturating_sub(now.animation_ms));
        let flush_wait = self
            .scheduler
            .flush_deadline()
            .map(|deadline| deadline.saturating_sub(now.flush_ms));
        match (animation_wait, flush_wait) {
            (Some(animation), Some(flush)) => Some(animation.min(flush)),
            (Some(animation), None) => Some(animation),
            (None, Some(flush)) => Some(flush),
            (None, None) => None,
        }
    }

    pub fn needs_poll(&self, now: FrameNow, motion: impl Into<MotionPlan>) -> bool {
        let motion = self.effective_motion(motion.into());
        let flush_pending = self.flush_requested || self.wheel.is_pending();
        let flush_unarmed = flush_pending && self.scheduler.flush_deadline().is_none();
        let animation_unarmed = !motion.is_none() && self.scheduler.animation_deadline().is_none();
        flush_unarmed
            || animation_unarmed
            || self.next_wait_ms(now).is_some_and(|millis| millis == 0)
    }

    pub fn record_submission(&mut self, submission: FrameSubmission, motion: MotionPlan) {
        match submission {
            FrameSubmission::Unchanged if motion.cadence().interval().is_some() => {
                self.suppressed_periodic =
                    Some((motion.cadence(), motion.revision(), motion.visual_sample()));
            }
            FrameSubmission::Accepted(_) | FrameSubmission::ResyncRequired => {
                self.suppressed_periodic = None;
            }
            FrameSubmission::Unchanged => {}
        }
    }

    fn effective_motion(&self, motion: MotionPlan) -> MotionPlan {
        if self.suppressed_periodic
            == Some((motion.cadence(), motion.revision(), motion.visual_sample()))
        {
            motion.without_cadence()
        } else {
            motion
        }
    }

    fn release_flush(&mut self, action: &mut RuntimePacerAction) {
        action.paint = self.flush_requested;
        action.wheel_batch = self.wheel.take();
        self.flush_requested = false;
    }
}

const fn terminal_wheel_events_per_step(
    terminal: TerminalName,
    multiplexer: TerminalMultiplexer,
) -> u8 {
    if multiplexer.is_detected() {
        return 1;
    }
    match terminal {
        TerminalName::Iterm2
        | TerminalName::VsCode
        | TerminalName::Cursor
        | TerminalName::Windsurf
        | TerminalName::Zed
        | TerminalName::WezTerm => 1,
        TerminalName::AppleTerminal
        | TerminalName::Ghostty
        | TerminalName::WarpTerminal
        | TerminalName::Kitty
        | TerminalName::Alacritty
        | TerminalName::Rio
        | TerminalName::Foot
        | TerminalName::JetBrains
        | TerminalName::Vte
        | TerminalName::Terminator
        | TerminalName::WindowsTerminal
        | TerminalName::Otty
        | TerminalName::Unknown => 3,
    }
}

impl Default for RuntimePacer {
    fn default() -> Self {
        Self::new()
    }
}
