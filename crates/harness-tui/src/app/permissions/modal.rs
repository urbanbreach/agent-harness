#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionModalSelection {
    #[default]
    AllowAlways,
    /// Session-scoped grant for the current permission request (freeze option 2).
    AllowSession,
    AllowOnce,
    Reject,
}

impl PermissionModalSelection {
    pub(crate) const fn number(self) -> usize {
        match self {
            Self::AllowAlways => 1,
            Self::AllowSession => 2,
            Self::AllowOnce => 3,
            Self::Reject => 4,
        }
    }

    pub(super) const fn from_number(number: char) -> Option<Self> {
        match number {
            '1' => Some(Self::AllowAlways),
            '2' => Some(Self::AllowSession),
            '3' => Some(Self::AllowOnce),
            '4' => Some(Self::Reject),
            _ => None,
        }
    }

    pub(super) fn cycle(self, forward: bool, allow_always: bool) -> Self {
        let options = if allow_always {
            // Permission order: always-approve, session edits, yes, reject.
            [
                Self::AllowAlways,
                Self::AllowSession,
                Self::AllowOnce,
                Self::Reject,
            ]
            .as_slice()
        } else {
            [Self::AllowOnce, Self::Reject].as_slice()
        };
        let current = options
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % options.len()
        } else {
            (current + options.len() - 1) % options.len()
        };
        options[next]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionConfirmSelection {
    #[default]
    Confirm,
    Cancel,
}

impl PermissionConfirmSelection {
    pub(super) fn cycle(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Confirm, true) | (Self::Cancel, false) => Self::Cancel,
            (Self::Cancel, true) | (Self::Confirm, false) => Self::Confirm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionModalStage {
    #[default]
    Decision,
    AlwaysConfirm,
}
