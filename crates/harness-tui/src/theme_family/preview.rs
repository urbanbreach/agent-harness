//! Preview, commit, and cancel state machine for live theme switching.

/// The current phase of a live theme preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum PreviewState {
    /// No preview active; `committed` is the live theme.
    Idle {
        committed: super::family::ThemeFamily,
    },
    /// A candidate is being previewed; `prior` is the committed theme to restore on cancel.
    Previewing {
        prior: super::family::ThemeFamily,
        candidate: super::family::ThemeFamily,
    },
}

/// Mutable preview state for live theme switching.
pub struct ThemePreview {
    state: PreviewState,
}

impl ThemePreview {
    pub fn new(committed: super::family::ThemeFamily) -> Self {
        Self {
            state: PreviewState::Idle { committed },
        }
    }

    pub fn state(&self) -> PreviewState {
        self.state
    }

    pub fn active(&self) -> super::family::ThemeFamily {
        match self.state {
            PreviewState::Idle { committed } => committed,
            PreviewState::Previewing { candidate, .. } => candidate,
        }
    }

    pub fn committed(&self) -> super::family::ThemeFamily {
        match self.state {
            PreviewState::Idle { committed } => committed,
            PreviewState::Previewing { prior, .. } => prior,
        }
    }

    pub fn begin_preview(&mut self, candidate: super::family::ThemeFamily) {
        self.state = match self.state {
            PreviewState::Idle { committed } => PreviewState::Previewing {
                prior: committed,
                candidate,
            },
            PreviewState::Previewing { prior, .. } => PreviewState::Previewing { prior, candidate },
        };
    }

    pub fn commit(&mut self) -> super::family::ThemeFamily {
        self.state = match self.state {
            PreviewState::Idle { committed } => PreviewState::Idle { committed },
            PreviewState::Previewing { candidate, .. } => PreviewState::Idle {
                committed: candidate,
            },
        };
        self.committed()
    }

    pub fn cancel(&mut self) -> super::family::ThemeFamily {
        self.state = match self.state {
            PreviewState::Idle { committed } => PreviewState::Idle { committed },
            PreviewState::Previewing { prior, .. } => PreviewState::Idle { committed: prior },
        };
        self.committed()
    }

    pub fn is_previewing(&self) -> bool {
        matches!(self.state, PreviewState::Previewing { .. })
    }
}

/// Errors available to callers that enforce strict preview transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewError {
    AlreadyPreviewing,
    NoActivePreview,
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::AlreadyPreviewing => "a theme preview is already active",
            Self::NoActivePreview => "no active theme preview",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PreviewError {}
