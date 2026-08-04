use std::time::Duration;

pub const DEFAULT_CTRL_C_WINDOW: Duration = Duration::from_millis(1_000);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlCAction {
    Interrupt { input_nonempty: bool },
    Kill,
}

#[derive(Debug, Clone, Copy)]
pub struct CtrlCTracker {
    first_at: Option<Duration>,
    window: Duration,
}

impl Default for CtrlCTracker {
    fn default() -> Self {
        Self::new(DEFAULT_CTRL_C_WINDOW)
    }
}

impl CtrlCTracker {
    pub const fn new(window: Duration) -> Self {
        Self {
            first_at: None,
            window,
        }
    }

    pub fn press(&mut self, at: Duration, input_nonempty: bool) -> CtrlCAction {
        let is_second = self
            .first_at
            .is_some_and(|first_at| at.saturating_sub(first_at) <= self.window);
        if is_second {
            self.first_at = None;
            CtrlCAction::Kill
        } else {
            self.first_at = Some(at);
            CtrlCAction::Interrupt { input_nonempty }
        }
    }

    pub fn reset(&mut self) {
        self.first_at = None;
    }
}
