use super::{FocusDirection, GestureKind};
use crate::app::Focus;
use crate::keybindings::Action;
use crate::overlay::OverlayKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    Startup,
    #[default]
    Live,
    Replay,
    PostRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GestureState {
    #[default]
    Idle,
    Active(GestureKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverlayStackState {
    overlays: Vec<OverlayKind>,
}

impl OverlayStackState {
    pub fn ordered(&self) -> &[OverlayKind] {
        &self.overlays
    }

    pub fn top(&self) -> Option<OverlayKind> {
        self.overlays.last().copied()
    }

    pub fn contains(&self, kind: OverlayKind) -> bool {
        self.overlays.contains(&kind)
    }

    pub fn push(&mut self, kind: OverlayKind) {
        self.overlays.push(kind);
    }

    pub fn pop(&mut self) -> Option<OverlayKind> {
        self.overlays.pop()
    }

    pub fn blocks_pointer_interaction(&self) -> bool {
        !matches!(self.top(), None | Some(OverlayKind::DetailsDrawer))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionState {
    pub screen_mode: ScreenMode,
    pub focus: Focus,
    pub overlay_stack: OverlayStackState,
    pub gesture: GestureState,
    pub pending_actions: Vec<Action>,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self::new(ScreenMode::Live, Focus::List)
    }
}

impl InteractionState {
    pub fn new(screen_mode: ScreenMode, focus: Focus) -> Self {
        Self {
            screen_mode,
            focus,
            overlay_stack: OverlayStackState::default(),
            gesture: GestureState::Idle,
            pending_actions: Vec::new(),
        }
    }
}

pub(super) fn focus_allowed(screen: ScreenMode, focus: Focus) -> bool {
    match screen {
        ScreenMode::Startup => !matches!(focus, Focus::Terminal),
        ScreenMode::Live => true,
        ScreenMode::Replay => !matches!(focus, Focus::Prompt),
        ScreenMode::PostRun => matches!(focus, Focus::List | Focus::Details),
    }
}

pub(super) fn default_focus(screen: ScreenMode) -> Focus {
    match screen {
        ScreenMode::Startup => Focus::List,
        ScreenMode::Live => Focus::Prompt,
        ScreenMode::Replay => Focus::Details,
        ScreenMode::PostRun => Focus::List,
    }
}

pub(super) fn focus_after(
    screen: ScreenMode,
    focus: Focus,
    direction: FocusDirection,
) -> Option<Focus> {
    match (screen, direction) {
        (ScreenMode::Startup, FocusDirection::Next) => match focus {
            Focus::List => Some(Focus::Prompt),
            Focus::Details | Focus::Terminal | Focus::Prompt => Some(Focus::List),
        },
        (ScreenMode::Startup, FocusDirection::Previous) => match focus {
            Focus::Prompt => Some(Focus::List),
            Focus::List | Focus::Details | Focus::Terminal => Some(Focus::Prompt),
        },
        (ScreenMode::Live, FocusDirection::Next) => match focus {
            Focus::List => Some(Focus::Prompt),
            Focus::Details => Some(Focus::Terminal),
            Focus::Terminal => Some(Focus::List),
            Focus::Prompt => Some(Focus::Details),
        },
        (ScreenMode::Live, FocusDirection::Previous) => match focus {
            Focus::List => Some(Focus::Terminal),
            Focus::Details => Some(Focus::Prompt),
            Focus::Terminal => Some(Focus::Details),
            Focus::Prompt => Some(Focus::List),
        },
        (ScreenMode::Replay, FocusDirection::Next) => match focus {
            Focus::List => Some(Focus::Details),
            Focus::Details => Some(Focus::Terminal),
            Focus::Terminal => Some(Focus::Details),
            Focus::Prompt => None,
        },
        (ScreenMode::Replay, FocusDirection::Previous) => match focus {
            Focus::List => Some(Focus::Details),
            Focus::Details => Some(Focus::List),
            Focus::Terminal => Some(Focus::Details),
            Focus::Prompt => None,
        },
        (ScreenMode::PostRun, FocusDirection::Next | FocusDirection::Previous) => match focus {
            Focus::List => Some(Focus::Details),
            Focus::Details => Some(Focus::List),
            Focus::Terminal | Focus::Prompt => None,
        },
    }
}
