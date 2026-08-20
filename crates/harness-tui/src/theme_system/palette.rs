use ratatui::style::Color;

use crate::theme::Theme;

use super::roles::PaletteRole;

impl PaletteRole {
    pub const LABELS: [&str; 53] = [
        "surface.canvas",
        "surface.shell",
        "surface.panel",
        "surface.panel_elevated",
        "surface.overlay",
        "surface.card",
        "surface.hover",
        "surface.selected_card",
        "border.subtle",
        "border.strong",
        "border.focus",
        "text.primary",
        "text.secondary",
        "text.tertiary",
        "text.accent",
        "text.inverse",
        "question.surface",
        "question.selected",
        "question.primary",
        "question.accent",
        "question.secondary",
        "status.success",
        "status.warning",
        "status.error",
        "status.info",
        "status.disabled",
        "markdown.heading_h1",
        "markdown.heading_h2",
        "markdown.heading_h3",
        "markdown.heading_h4",
        "markdown.heading_h5",
        "markdown.heading_h6",
        "markdown.link",
        "markdown.link_text",
        "markdown.code",
        "markdown.task_checked",
        "markdown.task_unchecked",
        "markdown.muted",
        "markdown.code_background",
        "markdown.text",
        "markdown.emph",
        "markdown.strong",
        "markdown.block_quote",
        "markdown.list_item",
        "markdown.list_enum",
        "markdown.rule",
        "agent.build",
        "agent.plan",
        "agent.docs",
        "agent.ask",
        "scrollbar.track",
        "scrollbar.thumb",
        "scrollbar.thumb_active",
    ];

    pub const fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub values: [Color; 53],
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let surfaces = theme.surface;
        let borders = theme.border;
        let text = theme.text;
        let question = theme.question_prompt;
        let status = theme.status;
        let markdown = theme.markdown;
        let scrollbar = theme.scrollbar;
        Self {
            values: [
                surfaces.canvas,
                surfaces.shell,
                surfaces.panel,
                surfaces.panel_elevated,
                surfaces.overlay,
                surfaces.card,
                surfaces.hover,
                surfaces.selected_card,
                borders.subtle,
                borders.strong,
                borders.focus,
                text.primary,
                text.secondary,
                text.tertiary,
                text.accent,
                text.inverse,
                question.surface,
                question.selected,
                question.primary,
                question.accent,
                question.secondary,
                status.success,
                status.warning,
                status.error,
                status.info,
                status.disabled,
                markdown.heading_h1,
                markdown.heading_h2,
                markdown.heading_h3,
                markdown.heading_h4,
                markdown.heading_h5,
                markdown.heading_h6,
                markdown.link,
                markdown.link_text,
                markdown.code,
                markdown.task_checked,
                markdown.task_unchecked,
                markdown.muted,
                markdown.code_background,
                markdown.text,
                markdown.emph,
                markdown.strong,
                markdown.block_quote,
                markdown.list_item,
                markdown.list_enum,
                markdown.rule,
                text.accent,
                text.accent,
                text.accent,
                text.accent,
                scrollbar.track,
                scrollbar.thumb,
                scrollbar.thumb_active,
            ],
        }
    }

    pub const fn color(self, role: PaletteRole) -> Color {
        self.values[role.index()]
    }
}
