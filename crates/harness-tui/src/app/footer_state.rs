//! Footer vocabulary per shell state — Todo 26.
//!
//! Standalone leaf module mapping shell lifecycle states to their footer
//! hint vocabulary. Included via `#[path]` in the parity test.

use crate::app::shell_status::ShellStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterHint {
    pub label: &'static str,
    pub kind: FooterHintKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FooterHintKind {
    Send,
    Mode,
    Shortcuts,
    Commands,
    Quit,
    Focus,
    Convo,
    Open,
    Replay,
    Cancel,
    Retry,
    Continue,
}

impl FooterHintKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Mode => "mode",
            Self::Shortcuts => "shortcuts",
            Self::Commands => "commands",
            Self::Quit => "quit",
            Self::Focus => "focus",
            Self::Convo => "convo",
            Self::Open => "open",
            Self::Replay => "replay",
            Self::Cancel => "cancel",
            Self::Retry => "retry",
            Self::Continue => "continue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterVocabulary {
    pub status: ShellStatus,
    pub hints: Vec<FooterHint>,
}

impl FooterVocabulary {
    pub fn for_status(status: ShellStatus) -> Self {
        let hints = match status {
            ShellStatus::Idle => vec![
                FooterHint {
                    label: "Enter:send",
                    kind: FooterHintKind::Send,
                },
                FooterHint {
                    label: "Shift+Tab:mode",
                    kind: FooterHintKind::Mode,
                },
                FooterHint {
                    label: "Ctrl+x:shortcuts",
                    kind: FooterHintKind::Shortcuts,
                },
            ],
            ShellStatus::Streaming => vec![
                FooterHint {
                    label: "Ctrl+c:cancel",
                    kind: FooterHintKind::Cancel,
                },
                FooterHint {
                    label: "Shift+Tab:mode",
                    kind: FooterHintKind::Mode,
                },
            ],
            ShellStatus::PermissionBlocked => vec![
                FooterHint {
                    label: "\u{2190}\u{2192}:select",
                    kind: FooterHintKind::Focus,
                },
                FooterHint {
                    label: "Enter:allow",
                    kind: FooterHintKind::Send,
                },
                FooterHint {
                    label: "Esc:deny",
                    kind: FooterHintKind::Quit,
                },
            ],
            ShellStatus::PermissionPending => vec![FooterHint {
                label: "wait",
                kind: FooterHintKind::Shortcuts,
            }],
            ShellStatus::ToolQueued | ShellStatus::ToolRunning => vec![
                FooterHint {
                    label: "Ctrl+c:cancel",
                    kind: FooterHintKind::Cancel,
                },
                FooterHint {
                    label: "Shift+Tab:mode",
                    kind: FooterHintKind::Mode,
                },
            ],
            ShellStatus::ToolSucceeded | ShellStatus::TurnComplete => vec![
                FooterHint {
                    label: "Enter:send",
                    kind: FooterHintKind::Send,
                },
                FooterHint {
                    label: "Ctrl+p:commands",
                    kind: FooterHintKind::Commands,
                },
            ],
            ShellStatus::ToolFailed | ShellStatus::Failure => vec![
                FooterHint {
                    label: "Enter:retry",
                    kind: FooterHintKind::Retry,
                },
                FooterHint {
                    label: "Ctrl+p:commands",
                    kind: FooterHintKind::Commands,
                },
            ],
            ShellStatus::PostRun => vec![
                FooterHint {
                    label: "tab:focus",
                    kind: FooterHintKind::Focus,
                },
                FooterHint {
                    label: "Ctrl+p:commands",
                    kind: FooterHintKind::Commands,
                },
                FooterHint {
                    label: "q:quit",
                    kind: FooterHintKind::Quit,
                },
            ],
            ShellStatus::PostRunFailure => vec![
                FooterHint {
                    label: "Enter:retry",
                    kind: FooterHintKind::Retry,
                },
                FooterHint {
                    label: "Ctrl+p:commands",
                    kind: FooterHintKind::Commands,
                },
                FooterHint {
                    label: "q:quit",
                    kind: FooterHintKind::Quit,
                },
            ],
            ShellStatus::ReplayReadOnly => vec![
                FooterHint {
                    label: "?:shortcuts",
                    kind: FooterHintKind::Shortcuts,
                },
                FooterHint {
                    label: "tab:focus",
                    kind: FooterHintKind::Focus,
                },
                FooterHint {
                    label: "q:quit",
                    kind: FooterHintKind::Quit,
                },
            ],
            ShellStatus::Cancelled => vec![
                FooterHint {
                    label: "Enter:retry",
                    kind: FooterHintKind::Retry,
                },
                FooterHint {
                    label: "Ctrl+p:commands",
                    kind: FooterHintKind::Commands,
                },
            ],
            ShellStatus::Degraded | ShellStatus::Disconnected => vec![
                FooterHint {
                    label: "Ctrl+p:commands",
                    kind: FooterHintKind::Commands,
                },
                FooterHint {
                    label: "q:quit",
                    kind: FooterHintKind::Quit,
                },
            ],
            ShellStatus::Startup => vec![
                FooterHint {
                    label: "Enter:send",
                    kind: FooterHintKind::Send,
                },
                FooterHint {
                    label: "Ctrl+p:open",
                    kind: FooterHintKind::Open,
                },
                FooterHint {
                    label: "q:quit",
                    kind: FooterHintKind::Quit,
                },
            ],
        };
        Self { status, hints }
    }

    pub fn has_send(&self) -> bool {
        self.hints.iter().any(|h| h.kind == FooterHintKind::Send)
    }

    pub fn has_cancel(&self) -> bool {
        self.hints.iter().any(|h| h.kind == FooterHintKind::Cancel)
    }

    pub fn has_retry(&self) -> bool {
        self.hints.iter().any(|h| h.kind == FooterHintKind::Retry)
    }

    pub fn has_quit(&self) -> bool {
        self.hints.iter().any(|h| h.kind == FooterHintKind::Quit)
    }
}
