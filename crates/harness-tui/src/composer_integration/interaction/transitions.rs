use super::intent::{GestureKind, OverlayTarget, UiIntent};
use super::state::{
    default_focus, focus_after, focus_allowed, GestureState, InteractionState, ScreenMode,
};
use crate::keybindings::Action;
use crate::overlay::OverlayKind;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionReason {
    FocusUnavailable,
    ModalOwnsInput,
    OverlayAlreadyOpen,
    OverlayNotTop,
    NoOverlay,
    GestureAlreadyActive,
    GestureNotActive,
    GestureMismatch,
    ReadOnlyScreen,
    ActionNotPending,
    ScreenAlreadyActive,
}

impl fmt::Display for TransitionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::FocusUnavailable => "focus unavailable",
            Self::ModalOwnsInput => "modal owns input",
            Self::OverlayAlreadyOpen => "overlay already open",
            Self::OverlayNotTop => "overlay is not topmost",
            Self::NoOverlay => "no overlay is open",
            Self::GestureAlreadyActive => "gesture already active",
            Self::GestureNotActive => "gesture is not active",
            Self::GestureMismatch => "gesture kind does not match",
            Self::ReadOnlyScreen => "screen is read-only",
            Self::ActionNotPending => "action is not pending",
            Self::ScreenAlreadyActive => "screen is already active",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    Illegal {
        intent: UiIntent,
        reason: TransitionReason,
    },
    Stale {
        intent: UiIntent,
        reason: TransitionReason,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Illegal { reason, .. } => write!(formatter, "illegal interaction: {reason}"),
            Self::Stale { reason, .. } => write!(formatter, "stale interaction: {reason}"),
        }
    }
}

impl std::error::Error for TransitionError {}

#[derive(Debug, Clone, Copy, Default)]
pub struct TransitionTable;

impl TransitionTable {
    pub fn apply(
        &self,
        mut state: InteractionState,
        intent: UiIntent,
    ) -> Result<InteractionState, TransitionError> {
        match intent {
            UiIntent::MoveFocus(direction) => {
                if state.overlay_stack.blocks_pointer_interaction() {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::MoveFocus(direction),
                        reason: TransitionReason::ModalOwnsInput,
                    });
                }
                let Some(focus) = focus_after(state.screen_mode, state.focus, direction) else {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::MoveFocus(direction),
                        reason: TransitionReason::FocusUnavailable,
                    });
                };
                state.focus = focus;
                Ok(state)
            }
            UiIntent::SetFocus(focus) => {
                if state.overlay_stack.blocks_pointer_interaction() {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::SetFocus(focus),
                        reason: TransitionReason::ModalOwnsInput,
                    });
                }
                if !focus_allowed(state.screen_mode, focus) {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::SetFocus(focus),
                        reason: TransitionReason::FocusUnavailable,
                    });
                }
                if state.focus == focus {
                    return Err(TransitionError::Stale {
                        intent: UiIntent::SetFocus(focus),
                        reason: TransitionReason::FocusUnavailable,
                    });
                }
                state.focus = focus;
                Ok(state)
            }
            UiIntent::OpenOverlay(kind) => {
                if state.overlay_stack.contains(kind) {
                    return Err(TransitionError::Stale {
                        intent: UiIntent::OpenOverlay(kind),
                        reason: TransitionReason::OverlayAlreadyOpen,
                    });
                }
                if state.overlay_stack.top() == Some(OverlayKind::PermissionModal)
                    && kind != OverlayKind::PermissionModal
                {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::OpenOverlay(kind),
                        reason: TransitionReason::ModalOwnsInput,
                    });
                }
                state.overlay_stack.push(kind);
                Ok(state)
            }
            UiIntent::CloseOverlay(target) => close_overlay(state, target, intent),
            UiIntent::BeginGesture(kind) => {
                if !matches!(state.gesture, GestureState::Idle) {
                    return Err(TransitionError::Stale {
                        intent: UiIntent::BeginGesture(kind),
                        reason: TransitionReason::GestureAlreadyActive,
                    });
                }
                if state.overlay_stack.blocks_pointer_interaction()
                    && kind != GestureKind::OverlayActivation
                {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::BeginGesture(kind),
                        reason: TransitionReason::ModalOwnsInput,
                    });
                }
                state.gesture = GestureState::Active(kind);
                Ok(state)
            }
            UiIntent::UpdateGesture(kind) => match state.gesture {
                GestureState::Idle => Err(TransitionError::Stale {
                    intent: UiIntent::UpdateGesture(kind),
                    reason: TransitionReason::GestureNotActive,
                }),
                GestureState::Active(active) if active != kind => Err(TransitionError::Illegal {
                    intent: UiIntent::UpdateGesture(kind),
                    reason: TransitionReason::GestureMismatch,
                }),
                GestureState::Active(_) => Ok(state),
            },
            UiIntent::EndGesture => {
                if matches!(state.gesture, GestureState::Idle) {
                    return Err(TransitionError::Stale {
                        intent: UiIntent::EndGesture,
                        reason: TransitionReason::GestureNotActive,
                    });
                }
                state.gesture = GestureState::Idle;
                Ok(state)
            }
            UiIntent::DispatchAction(action) => {
                if state.screen_mode == ScreenMode::Replay
                    && matches!(action, Action::SubmitPrompt | Action::InterjectPrompt)
                {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::DispatchAction(action),
                        reason: TransitionReason::ReadOnlyScreen,
                    });
                }
                if state.overlay_stack.top() == Some(OverlayKind::PermissionModal)
                    && !matches!(
                        action,
                        Action::AllowPermission
                            | Action::AlwaysApprovePermission
                            | Action::DenyPermission
                            | Action::DismissModal
                    )
                {
                    return Err(TransitionError::Illegal {
                        intent: UiIntent::DispatchAction(action),
                        reason: TransitionReason::ModalOwnsInput,
                    });
                }
                state.pending_actions.push(action);
                Ok(state)
            }
            UiIntent::CompleteAction(action) => {
                let Some(index) = state
                    .pending_actions
                    .iter()
                    .rposition(|pending| *pending == action)
                else {
                    return Err(TransitionError::Stale {
                        intent: UiIntent::CompleteAction(action),
                        reason: TransitionReason::ActionNotPending,
                    });
                };
                state.pending_actions.remove(index);
                Ok(state)
            }
            UiIntent::SwitchScreen(screen_mode) => {
                if state.screen_mode == screen_mode {
                    return Err(TransitionError::Stale {
                        intent: UiIntent::SwitchScreen(screen_mode),
                        reason: TransitionReason::ScreenAlreadyActive,
                    });
                }
                state.screen_mode = screen_mode;
                state.overlay_stack = Default::default();
                state.gesture = GestureState::Idle;
                state.pending_actions.clear();
                state.focus = default_focus(screen_mode);
                Ok(state)
            }
        }
    }
}

fn close_overlay(
    mut state: InteractionState,
    target: OverlayTarget,
    intent: UiIntent,
) -> Result<InteractionState, TransitionError> {
    let Some(top) = state.overlay_stack.top() else {
        return Err(TransitionError::Stale {
            intent,
            reason: TransitionReason::NoOverlay,
        });
    };
    let matches_target = match target {
        OverlayTarget::Top => true,
        OverlayTarget::Kind(kind) => kind == top,
    };
    if !matches_target {
        return Err(TransitionError::Stale {
            intent,
            reason: TransitionReason::OverlayNotTop,
        });
    }
    let _ = state.overlay_stack.pop();
    Ok(state)
}
