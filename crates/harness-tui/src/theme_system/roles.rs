#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteRole {
    SurfaceCanvas,
    SurfaceShell,
    SurfacePanel,
    SurfacePanelElevated,
    SurfaceOverlay,
    SurfaceCard,
    SurfaceHover,
    SurfaceSelectedCard,
    BorderSubtle,
    BorderStrong,
    BorderFocus,
    TextPrimary,
    TextSecondary,
    TextTertiary,
    TextAccent,
    TextInverse,
    QuestionSurface,
    QuestionSelected,
    QuestionPrimary,
    QuestionAccent,
    QuestionSecondary,
    StatusSuccess,
    StatusWarning,
    StatusError,
    StatusInfo,
    StatusDisabled,
    MarkdownHeadingH1,
    MarkdownHeadingH2,
    MarkdownHeadingH3,
    MarkdownHeadingH4,
    MarkdownHeadingH5,
    MarkdownHeadingH6,
    MarkdownLink,
    MarkdownLinkText,
    MarkdownCode,
    MarkdownTaskChecked,
    MarkdownTaskUnchecked,
    MarkdownMuted,
    MarkdownCodeBackground,
    MarkdownText,
    MarkdownEmph,
    MarkdownStrong,
    MarkdownBlockQuote,
    MarkdownListItem,
    MarkdownListEnum,
    MarkdownRule,
    AgentBuild,
    AgentPlan,
    AgentDocs,
    AgentAsk,
    ScrollbarTrack,
    ScrollbarThumb,
    ScrollbarThumbActive,
}

impl PaletteRole {
    pub const ALL: [Self; 53] = [
        Self::SurfaceCanvas,
        Self::SurfaceShell,
        Self::SurfacePanel,
        Self::SurfacePanelElevated,
        Self::SurfaceOverlay,
        Self::SurfaceCard,
        Self::SurfaceHover,
        Self::SurfaceSelectedCard,
        Self::BorderSubtle,
        Self::BorderStrong,
        Self::BorderFocus,
        Self::TextPrimary,
        Self::TextSecondary,
        Self::TextTertiary,
        Self::TextAccent,
        Self::TextInverse,
        Self::QuestionSurface,
        Self::QuestionSelected,
        Self::QuestionPrimary,
        Self::QuestionAccent,
        Self::QuestionSecondary,
        Self::StatusSuccess,
        Self::StatusWarning,
        Self::StatusError,
        Self::StatusInfo,
        Self::StatusDisabled,
        Self::MarkdownHeadingH1,
        Self::MarkdownHeadingH2,
        Self::MarkdownHeadingH3,
        Self::MarkdownHeadingH4,
        Self::MarkdownHeadingH5,
        Self::MarkdownHeadingH6,
        Self::MarkdownLink,
        Self::MarkdownLinkText,
        Self::MarkdownCode,
        Self::MarkdownTaskChecked,
        Self::MarkdownTaskUnchecked,
        Self::MarkdownMuted,
        Self::MarkdownCodeBackground,
        Self::MarkdownText,
        Self::MarkdownEmph,
        Self::MarkdownStrong,
        Self::MarkdownBlockQuote,
        Self::MarkdownListItem,
        Self::MarkdownListEnum,
        Self::MarkdownRule,
        Self::AgentBuild,
        Self::AgentPlan,
        Self::AgentDocs,
        Self::AgentAsk,
        Self::ScrollbarTrack,
        Self::ScrollbarThumb,
        Self::ScrollbarThumbActive,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::SurfaceCanvas => 0,
            Self::SurfaceShell => 1,
            Self::SurfacePanel => 2,
            Self::SurfacePanelElevated => 3,
            Self::SurfaceOverlay => 4,
            Self::SurfaceCard => 5,
            Self::SurfaceHover => 6,
            Self::SurfaceSelectedCard => 7,
            Self::BorderSubtle => 8,
            Self::BorderStrong => 9,
            Self::BorderFocus => 10,
            Self::TextPrimary => 11,
            Self::TextSecondary => 12,
            Self::TextTertiary => 13,
            Self::TextAccent => 14,
            Self::TextInverse => 15,
            Self::QuestionSurface => 16,
            Self::QuestionSelected => 17,
            Self::QuestionPrimary => 18,
            Self::QuestionAccent => 19,
            Self::QuestionSecondary => 20,
            Self::StatusSuccess => 21,
            Self::StatusWarning => 22,
            Self::StatusError => 23,
            Self::StatusInfo => 24,
            Self::StatusDisabled => 25,
            Self::MarkdownHeadingH1 => 26,
            Self::MarkdownHeadingH2 => 27,
            Self::MarkdownHeadingH3 => 28,
            Self::MarkdownHeadingH4 => 29,
            Self::MarkdownHeadingH5 => 30,
            Self::MarkdownHeadingH6 => 31,
            Self::MarkdownLink => 32,
            Self::MarkdownLinkText => 33,
            Self::MarkdownCode => 34,
            Self::MarkdownTaskChecked => 35,
            Self::MarkdownTaskUnchecked => 36,
            Self::MarkdownMuted => 37,
            Self::MarkdownCodeBackground => 38,
            Self::MarkdownText => 39,
            Self::MarkdownEmph => 40,
            Self::MarkdownStrong => 41,
            Self::MarkdownBlockQuote => 42,
            Self::MarkdownListItem => 43,
            Self::MarkdownListEnum => 44,
            Self::MarkdownRule => 45,
            Self::AgentBuild => 46,
            Self::AgentPlan => 47,
            Self::AgentDocs => 48,
            Self::AgentAsk => 49,
            Self::ScrollbarTrack => 50,
            Self::ScrollbarThumb => 51,
            Self::ScrollbarThumbActive => 52,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphRole {
    Streaming,
    Done,
    Error,
    PendingPermission,
    Queued,
    Running,
    Succeeded,
    Failed,
    UserMarker,
    ToolMarker,
    CardTop,
    CardMid,
    CardBottom,
}

impl GlyphRole {
    pub const ALL: [Self; 13] = [
        Self::Streaming,
        Self::Done,
        Self::Error,
        Self::PendingPermission,
        Self::Queued,
        Self::Running,
        Self::Succeeded,
        Self::Failed,
        Self::UserMarker,
        Self::ToolMarker,
        Self::CardTop,
        Self::CardMid,
        Self::CardBottom,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Streaming => 0,
            Self::Done => 1,
            Self::Error => 2,
            Self::PendingPermission => 3,
            Self::Queued => 4,
            Self::Running => 5,
            Self::Succeeded => 6,
            Self::Failed => 7,
            Self::UserMarker => 8,
            Self::ToolMarker => 9,
            Self::CardTop => 10,
            Self::CardMid => 11,
            Self::CardBottom => 12,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderRole {
    None,
    Subtle,
    Strong,
    Focus,
}

impl BorderRole {
    pub const ALL: [Self; 4] = [Self::None, Self::Subtle, Self::Strong, Self::Focus];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::None => 0,
            Self::Subtle => 1,
            Self::Strong => 2,
            Self::Focus => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusRole {
    Focused,
    Unfocused,
    Selected,
    Hovered,
    Disabled,
}

impl FocusRole {
    pub const ALL: [Self; 5] = [
        Self::Focused,
        Self::Unfocused,
        Self::Selected,
        Self::Hovered,
        Self::Disabled,
    ];
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Focused => 0,
            Self::Unfocused => 1,
            Self::Selected => 2,
            Self::Hovered => 3,
            Self::Disabled => 4,
        }
    }
}

pub use super::bindings::{
    DiffColors, LifecycleColors, LifecyclePalette, LifecycleState, MediaColors, PermissionColors,
    SelectionColors, SemanticThemeColors, ToolColors,
};
pub use super::focus::{BorderPalette, FocusPalette, FocusStyle};
pub use super::glyphs::GlyphPalette;
pub use super::palette::Palette;
