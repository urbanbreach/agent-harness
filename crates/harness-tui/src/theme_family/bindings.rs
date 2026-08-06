use ratatui::style::Color;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionColors {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Color,
    pub hover_background: Color,
    pub hover_foreground: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffColors {
    pub context_background: Color,
    pub added_background: Color,
    pub removed_background: Color,
    pub added_gutter: Color,
    pub removed_gutter: Color,
    pub added_highlight: Color,
    pub removed_highlight: Color,
    pub hunk_header: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolColors {
    pub pending: Color,
    pub queued: Color,
    pub running: Color,
    pub succeeded: Color,
    pub failed: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionColors {
    pub surface: Color,
    pub rail: Color,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub selected: Color,
    pub error: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaColors {
    pub placeholder: Color,
    pub label: Color,
    pub border: Color,
    pub progress: Color,
    pub error: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleState {
    Startup,
    Ready,
    Drafting,
    Sending,
    Streaming,
    ToolQueued,
    ToolRunning,
    PermissionPending,
    PermissionBlocked,
    QuestionPending,
    Success,
    Failure,
    Cancelled,
    Recovering,
    Degraded,
    Disconnected,
}

impl LifecycleState {
    pub const ALL: [Self; 16] = [
        Self::Startup,
        Self::Ready,
        Self::Drafting,
        Self::Sending,
        Self::Streaming,
        Self::ToolQueued,
        Self::ToolRunning,
        Self::PermissionPending,
        Self::PermissionBlocked,
        Self::QuestionPending,
        Self::Success,
        Self::Failure,
        Self::Cancelled,
        Self::Recovering,
        Self::Degraded,
        Self::Disconnected,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Startup => 0,
            Self::Ready => 1,
            Self::Drafting => 2,
            Self::Sending => 3,
            Self::Streaming => 4,
            Self::ToolQueued => 5,
            Self::ToolRunning => 6,
            Self::PermissionPending => 7,
            Self::PermissionBlocked => 8,
            Self::QuestionPending => 9,
            Self::Success => 10,
            Self::Failure => 11,
            Self::Cancelled => 12,
            Self::Recovering => 13,
            Self::Degraded => 14,
            Self::Disconnected => 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleColors {
    pub foreground: Color,
    pub background: Color,
    pub border: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecyclePalette {
    pub values: [LifecycleColors; 16],
}

impl LifecyclePalette {
    pub const fn from_theme(theme: &Theme) -> Self {
        let quiet = LifecycleColors {
            foreground: theme.text.secondary,
            background: theme.surface.panel,
            border: theme.border.subtle,
        };
        let active = LifecycleColors {
            foreground: theme.text.primary,
            background: theme.surface.panel,
            border: theme.border.focus,
        };
        let warning = LifecycleColors {
            foreground: theme.status.warning,
            background: theme.surface.panel_elevated,
            border: theme.status.warning,
        };
        let error = LifecycleColors {
            foreground: theme.status.error,
            background: theme.surface.panel_elevated,
            border: theme.status.error,
        };
        let success = LifecycleColors {
            foreground: theme.status.success,
            background: theme.surface.panel,
            border: theme.status.success,
        };
        let info = LifecycleColors {
            foreground: theme.status.info,
            background: theme.surface.panel,
            border: theme.status.info,
        };
        Self {
            values: [
                LifecycleColors {
                    foreground: theme.text.accent,
                    background: theme.surface.panel_elevated,
                    border: theme.border.focus,
                },
                quiet,
                active,
                info,
                LifecycleColors {
                    foreground: theme.text.accent,
                    background: theme.surface.panel,
                    border: theme.border.focus,
                },
                quiet,
                active,
                warning,
                error,
                warning,
                success,
                error,
                warning,
                info,
                warning,
                error,
            ],
        }
    }

    pub const fn colors(self, state: LifecycleState) -> LifecycleColors {
        self.values[state.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticThemeColors {
    pub selection: SelectionColors,
    pub diff: DiffColors,
    pub tool: ToolColors,
    pub permission: PermissionColors,
    pub media: MediaColors,
    pub lifecycle: LifecyclePalette,
}

impl SemanticThemeColors {
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            selection: SelectionColors {
                background: theme.text.accent,
                foreground: theme.text.inverse,
                cursor: theme.text.primary,
                hover_background: theme.surface.selected_card,
                hover_foreground: theme.text.primary,
            },
            diff: DiffColors {
                context_background: theme.reference_terminal.canvas,
                added_background: theme.reference_terminal.diff_added,
                removed_background: theme.reference_terminal.diff_removed,
                added_gutter: theme.reference_terminal.diff_added_gutter,
                removed_gutter: theme.reference_terminal.diff_removed_gutter,
                added_highlight: theme.reference_terminal.diff_added_highlight,
                removed_highlight: theme.reference_terminal.diff_removed_highlight,
                hunk_header: theme.reference_terminal.diff_hunk_header,
            },
            tool: ToolColors {
                pending: theme.status.warning,
                queued: theme.text.secondary,
                running: theme.text.primary,
                succeeded: theme.text.secondary,
                failed: theme.status.error,
            },
            permission: PermissionColors {
                surface: theme.surface.panel_elevated,
                rail: theme.text.accent,
                primary: theme.question_prompt.primary,
                secondary: theme.question_prompt.secondary,
                accent: theme.question_prompt.accent,
                selected: theme.question_prompt.selected,
                error: theme.status.error,
            },
            media: MediaColors {
                placeholder: theme.text.secondary,
                label: theme.text.secondary,
                border: theme.border.subtle,
                progress: theme.status.info,
                error: theme.status.error,
            },
            lifecycle: LifecyclePalette::from_theme(theme),
        }
    }
}
