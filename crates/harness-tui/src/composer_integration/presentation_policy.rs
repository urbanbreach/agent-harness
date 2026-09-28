use crate::keybindings::Action;

use super::{ComposerSurface, ComposerTone};

impl ComposerSurface {
    pub(crate) const fn tone(self) -> ComposerTone {
        match self {
            Self::Shell => ComposerTone::Shell,
            Self::Plan => ComposerTone::Plan,
            _ => ComposerTone::Standard,
        }
    }

    pub const fn marker(self) -> Option<&'static str> {
        match self {
            Self::Shell => Some("!"),
            _ => None,
        }
    }

    pub const fn right_label(self) -> Option<&'static str> {
        match self {
            Self::Shell => Some("Run shell command"),
            _ => None,
        }
    }
}

pub(crate) const fn compact_draft_hint_priority(active_turn: bool) -> &'static [Action] {
    use Action::{DismissModal, Help, InsertNewline, SubmitPrompt, VariantCycle};

    if active_turn {
        &[
            SubmitPrompt,
            InsertNewline,
            VariantCycle,
            DismissModal,
            Help,
        ]
    } else {
        &[SubmitPrompt, InsertNewline, VariantCycle, Help]
    }
}
