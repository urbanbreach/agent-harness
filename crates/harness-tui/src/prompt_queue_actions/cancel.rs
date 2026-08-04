use std::fmt;

use super::state::{QueueLifecycle, QueueState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelStage {
    Interrupt,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelError {
    NoWork,
    KillBeforeInterrupt,
    AlreadyRequested,
}

impl fmt::Display for CancelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NoWork => "cannot cancel without active work",
            Self::KillBeforeInterrupt => "kill requires a prior interrupt",
            Self::AlreadyRequested => "cancel stage was already requested",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CancelError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueVisuals {
    pub busy: bool,
    pub composer_enabled: bool,
    pub editing_enabled: bool,
    pub interrupt_enabled: bool,
    pub kill_enabled: bool,
}

pub fn visuals(lifecycle: QueueLifecycle, cancel_stage: Option<CancelStage>) -> QueueVisuals {
    let busy = lifecycle.is_busy();
    let editing_enabled = !matches!(lifecycle, QueueLifecycle::Tool | QueueLifecycle::Cancelling);
    QueueVisuals {
        busy,
        composer_enabled: !matches!(lifecycle, QueueLifecycle::Tool | QueueLifecycle::Cancelling),
        editing_enabled,
        interrupt_enabled: lifecycle.has_work() && cancel_stage.is_none(),
        kill_enabled: lifecycle == QueueLifecycle::Cancelling
            && cancel_stage == Some(CancelStage::Interrupt),
    }
}

pub(crate) fn apply_cancel(
    mut state: QueueState,
    stage: CancelStage,
) -> Result<QueueState, CancelError> {
    match stage {
        CancelStage::Interrupt => {
            if !state.lifecycle.has_work() {
                return Err(CancelError::NoWork);
            }
            if state.cancel_stage.is_some() {
                return Err(CancelError::AlreadyRequested);
            }
            state.cancel_stage = Some(CancelStage::Interrupt);
            state.lifecycle = QueueLifecycle::Cancelling;
            Ok(state)
        }
        CancelStage::Kill => {
            if state.cancel_stage != Some(CancelStage::Interrupt) {
                return Err(CancelError::KillBeforeInterrupt);
            }
            state.cancel_stage = Some(CancelStage::Kill);
            state.lifecycle = QueueLifecycle::Cancelling;
            Ok(state)
        }
    }
}
