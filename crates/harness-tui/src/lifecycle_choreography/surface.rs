use super::transitions::LifecycleState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionAvailability {
    Enabled,
    Disabled,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirective {
    Composer,
    Transcript,
    PermissionPrompt,
    QuestionPrompt,
    Dashboard,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceState {
    pub state: LifecycleState,
    pub composer_enabled: ActionAvailability,
    pub transcript_focus: FocusDirective,
    pub dashboard_enabled: ActionAvailability,
    pub permission_visible: bool,
    pub question_visible: bool,
    pub cancel_available: ActionAvailability,
}

impl SurfaceState {
    pub fn from_state(state: LifecycleState) -> Self {
        let (
            composer_enabled,
            transcript_focus,
            dashboard_enabled,
            permission_visible,
            question_visible,
            cancel_available,
        ) = match state {
            LifecycleState::Idle | LifecycleState::Drafting => (
                ActionAvailability::Enabled,
                FocusDirective::Composer,
                ActionAvailability::Enabled,
                false,
                false,
                ActionAvailability::Hidden,
            ),
            LifecycleState::Submitting
            | LifecycleState::Streaming
            | LifecycleState::Thinking
            | LifecycleState::Tool
            | LifecycleState::Diff => (
                ActionAvailability::Disabled,
                FocusDirective::Transcript,
                ActionAvailability::Enabled,
                false,
                false,
                ActionAvailability::Enabled,
            ),
            LifecycleState::Permission => (
                ActionAvailability::Disabled,
                FocusDirective::PermissionPrompt,
                ActionAvailability::Enabled,
                true,
                false,
                ActionAvailability::Enabled,
            ),
            LifecycleState::Question => (
                ActionAvailability::Disabled,
                FocusDirective::QuestionPrompt,
                ActionAvailability::Enabled,
                false,
                true,
                ActionAvailability::Enabled,
            ),
            LifecycleState::Queued
            | LifecycleState::Interjected
            | LifecycleState::Recovering
            | LifecycleState::Compacting => (
                ActionAvailability::Disabled,
                FocusDirective::Transcript,
                ActionAvailability::Enabled,
                false,
                false,
                ActionAvailability::Enabled,
            ),
            LifecycleState::Cancelling => (
                ActionAvailability::Disabled,
                FocusDirective::Transcript,
                ActionAvailability::Disabled,
                false,
                false,
                ActionAvailability::Disabled,
            ),
            LifecycleState::Failed | LifecycleState::Completed => (
                ActionAvailability::Enabled,
                FocusDirective::Transcript,
                ActionAvailability::Enabled,
                false,
                false,
                ActionAvailability::Hidden,
            ),
        };
        Self {
            state,
            composer_enabled,
            transcript_focus,
            dashboard_enabled,
            permission_visible,
            question_visible,
            cancel_available,
        }
    }

    pub const fn cursor_visible(&self) -> bool {
        matches!(self.composer_enabled, ActionAvailability::Enabled)
    }

    pub const fn any_prompt_visible(&self) -> bool {
        self.permission_visible || self.question_visible
    }

    pub const fn rest_mid_settled(&self) -> bool {
        matches!(
            self.state,
            LifecycleState::Idle | LifecycleState::Completed | LifecycleState::Failed
        ) && !self.any_prompt_visible()
    }
}
