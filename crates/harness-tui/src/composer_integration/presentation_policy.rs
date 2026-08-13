use crate::keybindings::Action;

use super::ComposerSurface;

impl ComposerSurface {
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
