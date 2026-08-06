use super::intent::{FocusDirection, GestureKind, OverlayTarget, UiIntent};
use super::state::{GestureState, InteractionState, ScreenMode};
use crate::app::Focus;
use crate::keybindings::Action;
use crate::overlay::OverlayKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionOutcome {
    Applied,
    Illegal,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableState {
    LivePrompt,
    LivePalette,
    LiveGesture,
    LivePendingAction,
    ReplayPrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableIntent {
    MoveFocus,
    SetFocus,
    OpenOverlay,
    CloseOverlay,
    BeginGesture,
    UpdateGesture,
    EndGesture,
    DispatchAction,
    CompleteAction,
    SwitchScreen,
    StaleClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionCase {
    name: &'static str,
    state: TableState,
    intent: TableIntent,
    outcome: TransitionOutcome,
}

impl TransitionCase {
    pub const fn name(self) -> &'static str {
        self.name
    }

    pub fn state(self) -> InteractionState {
        match self.state {
            TableState::LivePrompt => InteractionState::new(ScreenMode::Live, Focus::Prompt),
            TableState::LivePalette => {
                let mut state = InteractionState::new(ScreenMode::Live, Focus::Prompt);
                state.overlay_stack.push(OverlayKind::CommandPalette);
                state
            }
            TableState::LiveGesture => {
                let mut state = InteractionState::new(ScreenMode::Live, Focus::Details);
                state.gesture = GestureState::Active(GestureKind::TranscriptSelection);
                state
            }
            TableState::LivePendingAction => {
                let mut state = InteractionState::new(ScreenMode::Live, Focus::Prompt);
                state.pending_actions.push(Action::SubmitPrompt);
                state
            }
            TableState::ReplayPrompt => InteractionState::new(ScreenMode::Replay, Focus::Prompt),
        }
    }

    pub fn intent(self) -> UiIntent {
        match self.intent {
            TableIntent::MoveFocus => UiIntent::MoveFocus(FocusDirection::Next),
            TableIntent::SetFocus => UiIntent::SetFocus(Focus::Details),
            TableIntent::OpenOverlay => UiIntent::OpenOverlay(OverlayKind::CommandPalette),
            TableIntent::CloseOverlay => UiIntent::CloseOverlay(OverlayTarget::Top),
            TableIntent::BeginGesture => UiIntent::BeginGesture(GestureKind::TranscriptSelection),
            TableIntent::UpdateGesture => UiIntent::UpdateGesture(GestureKind::TranscriptSelection),
            TableIntent::EndGesture => UiIntent::EndGesture,
            TableIntent::DispatchAction => UiIntent::DispatchAction(Action::SubmitPrompt),
            TableIntent::CompleteAction => UiIntent::CompleteAction(Action::SubmitPrompt),
            TableIntent::SwitchScreen => UiIntent::SwitchScreen(ScreenMode::Replay),
            TableIntent::StaleClose => {
                UiIntent::CloseOverlay(OverlayTarget::Kind(OverlayKind::StatusDialog))
            }
        }
    }

    pub const fn outcome(self) -> TransitionOutcome {
        self.outcome
    }
}

const TRANSITION_TABLE: &[TransitionCase] = &[
    TransitionCase {
        name: "live_prompt_move_focus",
        state: TableState::LivePrompt,
        intent: TableIntent::MoveFocus,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_prompt_set_details",
        state: TableState::LivePrompt,
        intent: TableIntent::SetFocus,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_prompt_open_palette",
        state: TableState::LivePrompt,
        intent: TableIntent::OpenOverlay,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_palette_close_top",
        state: TableState::LivePalette,
        intent: TableIntent::CloseOverlay,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_prompt_begin_gesture",
        state: TableState::LivePrompt,
        intent: TableIntent::BeginGesture,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_gesture_update",
        state: TableState::LiveGesture,
        intent: TableIntent::UpdateGesture,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_gesture_end",
        state: TableState::LiveGesture,
        intent: TableIntent::EndGesture,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_prompt_dispatch_submit",
        state: TableState::LivePrompt,
        intent: TableIntent::DispatchAction,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_pending_complete_submit",
        state: TableState::LivePendingAction,
        intent: TableIntent::CompleteAction,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "live_prompt_switch_replay",
        state: TableState::LivePrompt,
        intent: TableIntent::SwitchScreen,
        outcome: TransitionOutcome::Applied,
    },
    TransitionCase {
        name: "replay_prompt_dispatch_submit",
        state: TableState::ReplayPrompt,
        intent: TableIntent::DispatchAction,
        outcome: TransitionOutcome::Illegal,
    },
    TransitionCase {
        name: "live_prompt_stale_close",
        state: TableState::LivePrompt,
        intent: TableIntent::StaleClose,
        outcome: TransitionOutcome::Stale,
    },
];

pub fn transition_cases() -> &'static [TransitionCase] {
    TRANSITION_TABLE
}
