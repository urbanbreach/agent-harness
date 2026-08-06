use ratatui::style::Color;

use crate::theme::Theme;

use super::roles::PaletteRole;

impl PaletteRole {
    pub const LABELS: [&str; 42] = [
        "surface.canvas",
        "surface.shell",
        "surface.panel",
        "surface.panel_elevated",
        "surface.overlay",
        "surface.card",
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
        "markdown.heading",
        "markdown.link",
        "markdown.link_text",
        "markdown.code",
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
    pub values: [Color; 42],
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let surfaces = theme.surface;
        let borders = theme.border;
        let text = theme.text;
        let question = theme.question_prompt;
        let status = theme.status;
        let markdown = theme.markdown;
        let agents = theme.agents;
        let scrollbar = theme.scrollbar;
        Self {
            values: [
                surfaces.canvas,
                surfaces.shell,
                surfaces.panel,
                surfaces.panel_elevated,
                surfaces.overlay,
                surfaces.card,
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
                markdown.heading,
                markdown.link,
                markdown.link_text,
                markdown.code,
                markdown.emph,
                markdown.strong,
                markdown.block_quote,
                markdown.list_item,
                markdown.list_enum,
                markdown.rule,
                agents.build,
                agents.plan,
                agents.docs,
                agents.ask,
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
