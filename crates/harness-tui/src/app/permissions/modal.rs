#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PermissionModalSelection {
    #[default]
    EnableYolo,
    /// Session-scoped grant for the current permission request.
    AllowSession,
    AllowOnce,
    Reject,
}

impl PermissionModalSelection {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::EnableYolo => "Yes, enable YOLO mode",
            Self::AllowSession => "Yes, remember this approval for this session",
            Self::AllowOnce => "Yes",
            Self::Reject => "No, reject (type to add feedback)",
        }
    }

    pub(super) fn cycle(self, forward: bool, options: &[Self]) -> Self {
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
    YoloConfirm,
}
