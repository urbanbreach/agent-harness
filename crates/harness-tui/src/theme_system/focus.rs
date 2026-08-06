use ratatui::style::Color;

use crate::theme::Theme;

use super::roles::{BorderRole, FocusRole};

impl BorderRole {
    pub const LABELS: [&str; 4] = ["none", "subtle", "strong", "focus"];

    pub const fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }
}

impl FocusRole {
    pub const LABELS: [&str; 5] = ["focused", "unfocused", "selected", "hovered", "disabled"];

    pub const fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderPalette {
    pub values: [Color; 4],
}

impl BorderPalette {
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            values: [
                Color::Reset,
                theme.border.subtle,
                theme.border.strong,
                theme.border.focus,
            ],
        }
    }

    pub const fn color(self, role: BorderRole) -> Color {
        self.values[role.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusStyle {
    pub foreground: Color,
    pub background: Color,
    pub border: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusPalette {
    pub values: [FocusStyle; 5],
}

impl FocusPalette {
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            values: [
                FocusStyle {
                    foreground: theme.text.primary,
                    background: theme.surface.panel,
                    border: theme.border.focus,
                },
                FocusStyle {
                    foreground: theme.text.secondary,
                    background: theme.surface.panel,
                    border: theme.border.subtle,
                },
                FocusStyle {
                    foreground: theme.text.inverse,
                    background: theme.text.accent,
                    border: theme.border.focus,
                },
                FocusStyle {
                    foreground: theme.text.primary,
                    background: theme.surface.selected_card,
                    border: theme.border.strong,
                },
                FocusStyle {
                    foreground: theme.status.disabled,
                    background: theme.surface.panel,
                    border: theme.border.subtle,
                },
            ],
        }
    }

    pub const fn style(self, role: FocusRole) -> FocusStyle {
        self.values[role.index()]
    }
}
