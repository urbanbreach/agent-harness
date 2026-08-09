#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteRole {
    SurfaceCanvas,
    SurfaceShell,
    SurfacePanel,
    SurfacePanelElevated,
    SurfaceOverlay,
    SurfaceCard,
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
    pub const ALL: [Self; 52] = [
        Self::SurfaceCanvas,
        Self::SurfaceShell,
        Self::SurfacePanel,
        Self::SurfacePanelElevated,
        Self::SurfaceOverlay,
        Self::SurfaceCard,
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
            Self::SurfaceSelectedCard => 6,
            Self::BorderSubtle => 7,
            Self::BorderStrong => 8,
            Self::BorderFocus => 9,
            Self::TextPrimary => 10,
            Self::TextSecondary => 11,
            Self::TextTertiary => 12,
            Self::TextAccent => 13,
            Self::TextInverse => 14,
            Self::QuestionSurface => 15,
            Self::QuestionSelected => 16,
            Self::QuestionPrimary => 17,
            Self::QuestionAccent => 18,
            Self::QuestionSecondary => 19,
            Self::StatusSuccess => 20,
            Self::StatusWarning => 21,
            Self::StatusError => 22,
            Self::StatusInfo => 23,
            Self::StatusDisabled => 24,
            Self::MarkdownHeadingH1 => 25,
            Self::MarkdownHeadingH2 => 26,
            Self::MarkdownHeadingH3 => 27,
            Self::MarkdownHeadingH4 => 28,
            Self::MarkdownHeadingH5 => 29,
            Self::MarkdownHeadingH6 => 30,
            Self::MarkdownLink => 31,
            Self::MarkdownLinkText => 32,
            Self::MarkdownCode => 33,
            Self::MarkdownTaskChecked => 34,
            Self::MarkdownTaskUnchecked => 35,
            Self::MarkdownMuted => 36,
            Self::MarkdownCodeBackground => 37,
            Self::MarkdownText => 38,
            Self::MarkdownEmph => 39,
            Self::MarkdownStrong => 40,
            Self::MarkdownBlockQuote => 41,
            Self::MarkdownListItem => 42,
            Self::MarkdownListEnum => 43,
            Self::MarkdownRule => 44,
            Self::AgentBuild => 45,
            Self::AgentPlan => 46,
            Self::AgentDocs => 47,
            Self::AgentAsk => 48,
            Self::ScrollbarTrack => 49,
            Self::ScrollbarThumb => 50,
            Self::ScrollbarThumbActive => 51,
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
