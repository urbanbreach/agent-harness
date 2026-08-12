use std::cmp::Ordering;

use crate::terminal::{TerminalMultiplexer, TerminalName};

use super::{FrameInputs, FrameNow, FrameReason, FrameScheduler};

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
    column: u16,
    row: u16,
}

impl WheelSample {
    pub const fn new(direction: WheelDirection, column: u16, row: u16) -> Self {
        Self {
            direction,
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
struct WheelAccumulator {
    delta: i16,
    column: u16,
    row: u16,
    events_per_step: u8,
}

impl WheelAccumulator {
    fn push(&mut self, sample: WheelSample) {
        let cap =
            i16::from(MAX_WHEEL_STEPS_PER_FLUSH).saturating_mul(i16::from(self.events_per_step));
        self.delta = self
            .delta
            .saturating_add(sample.direction.delta())
            .clamp(-cap, cap);
        self.column = sample.column;
        self.row = sample.row;
    }

    const fn is_pending(self) -> bool {
        self.delta != 0
    }

    fn take(&mut self) -> Option<WheelBatch> {
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
}

impl RuntimePacer {
    pub const fn new() -> Self {
        Self::with_reduced_motion(false)
    }

    pub const fn with_reduced_motion(reduced_motion: bool) -> Self {
        Self::with_wheel_events_per_step(reduced_motion, 1)
    }

    pub const fn with_terminal_wheel_profile(
        reduced_motion: bool,
        terminal: TerminalName,
        multiplexer: TerminalMultiplexer,
    ) -> Self {
        Self::with_wheel_events_per_step(
            reduced_motion,
            terminal_wheel_events_per_step(terminal, multiplexer),
        )
    }

    const fn with_wheel_events_per_step(reduced_motion: bool, events_per_step: u8) -> Self {
        Self {
            scheduler: FrameScheduler::with_reduced_motion(reduced_motion),
            flush_requested: false,
            wheel: WheelAccumulator {
                delta: 0,
                column: 0,
                row: 0,
                events_per_step,
            },
        }
    }

    pub fn request_flush(&mut self) {
        self.flush_requested = true;
    }

    pub fn queue_wheel(&mut self, sample: WheelSample) {
        self.wheel.push(sample);
    }

    pub fn poll(&mut self, now: FrameNow, animation_active: bool) -> RuntimePacerAction {
        let flush_pending = self.flush_requested || self.wheel.is_pending();
        let decision = self.scheduler.schedule(
            now,
            FrameInputs {
                animation_active,
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
                    action.advance_animation = animation_active;
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
        | TerminalName::GrokDesktop
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
