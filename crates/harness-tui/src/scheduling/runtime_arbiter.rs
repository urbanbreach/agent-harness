use std::time::{Duration, Instant};

pub const INPUT_BATCH_LIMIT: usize = 16;
pub const INPUT_BATCH_TIME: Duration = Duration::from_millis(2);
pub const LIVE_BATCH_LIMIT: usize = 16;
pub const LIVE_BATCH_TIME: Duration = Duration::from_millis(8);

pub trait ArbiterClock {
    fn now(&self) -> Instant;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemArbiterClock;

impl ArbiterClock for SystemArbiterClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePriority {
    FatalWriterFailure,
    FrameAcknowledged,
    Quit,
    Cancel,
    TerminalInput,
    PacerDeadline,
    AnimationDeadline,
    LiveUpdate,
    Park,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeReady {
    pub fatal_writer_failure: bool,
    pub frame_acknowledged: bool,
    pub quit: bool,
    pub cancel: bool,
    pub terminal_input: bool,
    pub pacer_deadline: bool,
    pub animation_deadline: bool,
    pub live_update: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDecision {
    FatalWriterFailure,
    FrameAcknowledged,
    Quit,
    Cancel,
    TerminalInput,
    PacerDeadline,
    AnimationDeadline,
    LiveUpdate,
    Park,
}

impl RuntimeDecision {
    pub const fn priority(self) -> RuntimePriority {
        match self {
            Self::FatalWriterFailure => RuntimePriority::FatalWriterFailure,
            Self::FrameAcknowledged => RuntimePriority::FrameAcknowledged,
            Self::Quit => RuntimePriority::Quit,
            Self::Cancel => RuntimePriority::Cancel,
            Self::TerminalInput => RuntimePriority::TerminalInput,
            Self::PacerDeadline => RuntimePriority::PacerDeadline,
            Self::AnimationDeadline => RuntimePriority::AnimationDeadline,
            Self::LiveUpdate => RuntimePriority::LiveUpdate,
            Self::Park => RuntimePriority::Park,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FairnessTurn {
    InputFirst,
    OneLiveAfterInputQuantum,
}

#[derive(Clone, Copy, Debug)]
pub struct BatchBudget {
    limit: usize,
    duration: Duration,
    started_at: Instant,
    consumed: usize,
}

impl BatchBudget {
    pub const fn new(limit: usize, duration: Duration, started_at: Instant) -> Self {
        Self {
            limit,
            duration,
            started_at,
            consumed: 0,
        }
    }

    pub const fn input(started_at: Instant) -> Self {
        Self::new(INPUT_BATCH_LIMIT, INPUT_BATCH_TIME, started_at)
    }

    pub fn consume(&mut self) {
        self.consumed = self.consumed.saturating_add(1);
    }

    pub fn exhausted(&self, now: Instant) -> bool {
        self.consumed >= self.limit
            || now.saturating_duration_since(self.started_at) >= self.duration
    }
}

#[derive(Debug)]
pub struct DeferredLiveUpdate<T>(Option<T>);

impl<T> Default for DeferredLiveUpdate<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> DeferredLiveUpdate<T> {
    pub const fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub fn defer(&mut self, update: T) -> Result<(), T> {
        if self.0.is_some() {
            Err(update)
        } else {
            self.0 = Some(update);
            Ok(())
        }
    }

    pub fn take(&mut self) -> Option<T> {
        self.0.take()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RuntimeArbiter {
    fairness: FairnessTurn,
    live_after_deadline: bool,
}

impl Default for RuntimeArbiter {
    fn default() -> Self {
        Self {
            fairness: FairnessTurn::InputFirst,
            live_after_deadline: false,
        }
    }
}

impl RuntimeArbiter {
    pub const fn fairness(&self) -> FairnessTurn {
        self.fairness
    }

    pub fn input_quantum_exhausted(&mut self) {
        self.fairness = FairnessTurn::OneLiveAfterInputQuantum;
    }

    pub fn live_applied(&mut self) {
        self.fairness = FairnessTurn::InputFirst;
        self.live_after_deadline = false;
    }

    pub fn deadline_served(&mut self) {
        self.live_after_deadline = true;
    }

    pub fn decide(&self, ready: RuntimeReady) -> RuntimeDecision {
        if ready.fatal_writer_failure {
            RuntimeDecision::FatalWriterFailure
        } else if ready.frame_acknowledged {
            RuntimeDecision::FrameAcknowledged
        } else if ready.quit {
            RuntimeDecision::Quit
        } else if ready.cancel {
            RuntimeDecision::Cancel
        } else if ready.terminal_input && matches!(self.fairness, FairnessTurn::InputFirst) {
            RuntimeDecision::TerminalInput
        } else if ready.live_update
            && (matches!(self.fairness, FairnessTurn::OneLiveAfterInputQuantum)
                || self.live_after_deadline)
        {
            RuntimeDecision::LiveUpdate
        } else if ready.pacer_deadline {
            RuntimeDecision::PacerDeadline
        } else if ready.animation_deadline {
            RuntimeDecision::AnimationDeadline
        } else if ready.live_update {
            RuntimeDecision::LiveUpdate
        } else if ready.terminal_input {
            RuntimeDecision::TerminalInput
        } else {
            RuntimeDecision::Park
        }
    }
}
