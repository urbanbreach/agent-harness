//! Semantic role facade unifying the design contract role families.

pub use crate::theme_tokens::{BorderRole, ColorRole, FocusRole, GlyphRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    Palette,
    Glyph,
    Border,
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "role")]
pub enum SemanticRole {
    Palette(ColorRole),
    Glyph(GlyphRole),
    Border(BorderRole),
    Focus(FocusRole),
}

impl SemanticRole {
    pub fn kind(&self) -> SemanticKind {
        match self {
            Self::Palette(_) => SemanticKind::Palette,
            Self::Glyph(_) => SemanticKind::Glyph,
            Self::Border(_) => SemanticKind::Border,
            Self::Focus(_) => SemanticKind::Focus,
        }
    }

    pub fn all() -> Vec<Self> {
        ColorRole::ALL
            .into_iter()
            .map(Self::Palette)
            .chain(GlyphRole::ALL.into_iter().map(Self::Glyph))
            .chain(BorderRole::ALL.into_iter().map(Self::Border))
            .chain(FocusRole::ALL.into_iter().map(Self::Focus))
            .collect()
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Palette(role) => match role {
                ColorRole::Canvas => "palette:canvas",
                ColorRole::Shell => "palette:shell",
                ColorRole::Panel => "palette:panel",
                ColorRole::PanelElevated => "palette:panel_elevated",
                ColorRole::Overlay => "palette:overlay",
                ColorRole::Card => "palette:card",
                ColorRole::ModalHover => "palette:modal_hover",
                ColorRole::SelectedCard => "palette:selected_card",
                ColorRole::PromptActiveSurface => "palette:prompt_active_surface",
                ColorRole::BorderSubtle => "palette:border_subtle",
                ColorRole::BorderStrong => "palette:border_strong",
                ColorRole::BorderFocus => "palette:border_focus",
                ColorRole::TextPrimary => "palette:text_primary",
                ColorRole::TextSecondary => "palette:text_secondary",
                ColorRole::TextTertiary => "palette:text_tertiary",
                ColorRole::TextAccent => "palette:text_accent",
                ColorRole::TextInverse => "palette:text_inverse",
                ColorRole::StatusSuccess => "palette:status_success",
                ColorRole::StatusWarning => "palette:status_warning",
                ColorRole::StatusError => "palette:status_error",
                ColorRole::StatusInfo => "palette:status_info",
                ColorRole::StatusDisabled => "palette:status_disabled",
                ColorRole::QuestionSurface => "palette:question_surface",
                ColorRole::QuestionSelected => "palette:question_selected",
                ColorRole::QuestionPrimary => "palette:question_primary",
                ColorRole::QuestionAccent => "palette:question_accent",
                ColorRole::QuestionSecondary => "palette:question_secondary",
                ColorRole::AgentBuild => "palette:agent_build",
                ColorRole::AgentPlan => "palette:agent_plan",
                ColorRole::AgentDocs => "palette:agent_docs",
                ColorRole::AgentAsk => "palette:agent_ask",
                ColorRole::MarkdownHeadingH1 => "palette:markdown_heading_h1",
                ColorRole::MarkdownHeadingH3 => "palette:markdown_heading_h3",
                ColorRole::MarkdownHeadingH4 => "palette:markdown_heading_h4",
                ColorRole::MarkdownHeadingH6 => "palette:markdown_heading_h6",
                ColorRole::MarkdownLinkText => "palette:markdown_link_text",
                ColorRole::MarkdownCode => "palette:markdown_code",
                ColorRole::ScrollbarTrack => "palette:scrollbar_track",
                ColorRole::TerminalPrimary => "palette:terminal_primary",
                ColorRole::TerminalSecondary => "palette:terminal_secondary",
                ColorRole::TerminalMuted => "palette:terminal_muted",
                ColorRole::TerminalError => "palette:terminal_error",
                ColorRole::TerminalPaletteSection => "palette:terminal_palette_section",
                ColorRole::TerminalForkAccent => "palette:terminal_fork_accent",
                ColorRole::DiffAdded => "palette:diff_added",
                ColorRole::DiffRemoved => "palette:diff_removed",
                ColorRole::DiffAddedGutter => "palette:diff_added_gutter",
                ColorRole::DiffRemovedGutter => "palette:diff_removed_gutter",
                ColorRole::DiffAddedHighlight => "palette:diff_added_highlight",
                ColorRole::DiffRemovedHighlight => "palette:diff_removed_highlight",
                ColorRole::DiffHunkHeader => "palette:diff_hunk_header",
            },
            Self::Glyph(role) => match role {
                GlyphRole::Streaming => "glyph:streaming",
                GlyphRole::Done => "glyph:done",
                GlyphRole::Error => "glyph:error",
                GlyphRole::PendingPermission => "glyph:pending_permission",
                GlyphRole::Queued => "glyph:queued",
                GlyphRole::Running => "glyph:running",
                GlyphRole::Succeeded => "glyph:succeeded",
                GlyphRole::Failed => "glyph:failed",
                GlyphRole::Cancelled => "glyph:cancelled",
                GlyphRole::UserMarker => "glyph:user_marker",
                GlyphRole::ToolMarker => "glyph:tool_marker",
                GlyphRole::CardTop => "glyph:card_top",
                GlyphRole::CardMiddle => "glyph:card_middle",
                GlyphRole::CardBottom => "glyph:card_bottom",
            },
            Self::Border(role) => match role {
                BorderRole::None => "border:none",
                BorderRole::Subtle => "border:subtle",
                BorderRole::Strong => "border:strong",
                BorderRole::Focus => "border:focus",
            },
            Self::Focus(role) => match role {
                FocusRole::Panel => "focus:panel",
                FocusRole::SelectedRow => "focus:selected_row",
                FocusRole::Cursor => "focus:cursor",
                FocusRole::Permission => "focus:permission",
                FocusRole::Question => "focus:question",
            },
        }
    }
}

impl std::fmt::Display for SemanticRole {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}
