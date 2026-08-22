mod types;

pub use crate::lifecycle_choreography::LifecycleState;
pub use crate::viewport::{BreakpointBand, ResponsiveBreakpoints, ViewportBreakpoint, ViewportId};
pub use types::*;

#[rustfmt::skip]
const fn color(role: ColorRole, red: u8, green: u8, blue: u8) -> ColorToken {
    ColorToken { role, value: PaletteColor { red, green, blue } }
}
#[rustfmt::skip]
const fn glyph(role: GlyphRole, preferred: &'static str, ascii: &'static str) -> GlyphToken {
    GlyphToken { role, preferred, ascii }
}
#[rustfmt::skip]
const fn border(role: BorderRole, color: Option<ColorRole>, intensity: u8) -> BorderToken {
    BorderToken { role, color, intensity }
}
#[rustfmt::skip]
const fn state(state: LifecycleState, foreground: ColorRole, background: ColorRole, glyph: GlyphRole) -> StateColorBinding {
    StateColorBinding { state, foreground, background, glyph }
}
#[rustfmt::skip]
const fn focus(role: FocusRole, border: BorderRole, foreground: ColorRole, background: ColorRole, modifier: TextModifier) -> FocusStyle {
    FocusStyle { role, border, foreground, background, modifier }
}

#[rustfmt::skip]
pub const VIEWPORTS: [ViewportBreakpoint; 7] = [
    ViewportBreakpoint { id: ViewportId::Compact40x10, width: 40, height: 10, band: BreakpointBand::UltraCompact, composer_inset: 1, breadcrumb_top_margin: 0, composer_footer_spacer: 0 },
    ViewportBreakpoint { id: ViewportId::Dense60x15, width: 60, height: 15, band: BreakpointBand::Compact, composer_inset: 1, breadcrumb_top_margin: 0, composer_footer_spacer: 0 },
    ViewportBreakpoint { id: ViewportId::Default80x24, width: 80, height: 24, band: BreakpointBand::Compact, composer_inset: 2, breadcrumb_top_margin: 1, composer_footer_spacer: 1 },
    ViewportBreakpoint { id: ViewportId::Standard100x30, width: 100, height: 30, band: BreakpointBand::Primary, composer_inset: 2, breadcrumb_top_margin: 1, composer_footer_spacer: 1 },
    ViewportBreakpoint { id: ViewportId::Wide132x40, width: 132, height: 40, band: BreakpointBand::Wide, composer_inset: 2, breadcrumb_top_margin: 1, composer_footer_spacer: 1 },
    ViewportBreakpoint { id: ViewportId::Large160x50, width: 160, height: 50, band: BreakpointBand::Large, composer_inset: 2, breadcrumb_top_margin: 1, composer_footer_spacer: 1 },
    ViewportBreakpoint { id: ViewportId::Maximum200x60, width: 200, height: 60, band: BreakpointBand::Maximum, composer_inset: 2, breadcrumb_top_margin: 1, composer_footer_spacer: 1 },
];

#[rustfmt::skip]
const PALETTE: PaletteTokens = PaletteTokens {
    roles: &[
        color(ColorRole::Canvas, 20, 20, 20),
        color(ColorRole::Shell, 20, 20, 20),
        color(ColorRole::Panel, 20, 20, 20),
        color(ColorRole::PanelElevated, 28, 28, 28),
        color(ColorRole::Overlay, 20, 20, 20),
        color(ColorRole::Card, 36, 36, 36),
        color(ColorRole::ModalHover, 44, 44, 44),
        color(ColorRole::SelectedCard, 85, 87, 83),
        color(ColorRole::PromptActiveSurface, 38, 38, 38),
        color(ColorRole::BorderSubtle, 50, 50, 55),
        color(ColorRole::BorderStrong, 60, 60, 65),
        color(ColorRole::BorderFocus, 80, 80, 88),
        color(ColorRole::TextPrimary, 225, 225, 225),
        color(ColorRole::TextSecondary, 108, 108, 108),
        color(ColorRole::TextTertiary, 88, 88, 88),
        color(ColorRole::TextAccent, 187, 154, 247),
        color(ColorRole::TextInverse, 20, 20, 20),
        color(ColorRole::StatusSuccess, 158, 206, 106),
        color(ColorRole::StatusWarning, 224, 175, 104),
        color(ColorRole::StatusError, 247, 118, 142),
        color(ColorRole::StatusInfo, 125, 207, 255),
        color(ColorRole::StatusDisabled, 88, 88, 88),
        color(ColorRole::QuestionSurface, 36, 36, 36),
        color(ColorRole::QuestionSelected, 54, 54, 54),
        color(ColorRole::QuestionPrimary, 225, 225, 225),
        color(ColorRole::QuestionAccent, 200, 200, 200),
        color(ColorRole::QuestionSecondary, 108, 108, 108),
        color(ColorRole::AgentBuild, 122, 162, 247),
        color(ColorRole::AgentPlan, 187, 154, 247),
        color(ColorRole::AgentDocs, 224, 175, 104),
        color(ColorRole::AgentAsk, 125, 207, 255),
        color(ColorRole::MarkdownHeadingH1, 26, 188, 156),
        color(ColorRole::MarkdownHeadingH3, 157, 124, 216),
        color(ColorRole::MarkdownHeadingH4, 120, 120, 120),
        color(ColorRole::MarkdownHeadingH6, 90, 90, 90),
        color(ColorRole::MarkdownLinkText, 122, 166, 218),
        color(ColorRole::MarkdownCode, 58, 149, 171),
        color(ColorRole::ScrollbarTrack, 17, 17, 17),
        color(ColorRole::TerminalPrimary, 225, 225, 225),
        color(ColorRole::TerminalSecondary, 108, 108, 108),
        color(ColorRole::TerminalMuted, 88, 88, 88),
        color(ColorRole::TerminalError, 247, 118, 142),
        color(ColorRole::TerminalPaletteSection, 187, 154, 247),
        color(ColorRole::TerminalForkAccent, 255, 158, 100),
        color(ColorRole::DiffAdded, 6, 56, 6),
        color(ColorRole::DiffRemoved, 66, 14, 20),
        color(ColorRole::DiffAddedGutter, 6, 56, 6),
        color(ColorRole::DiffRemovedGutter, 66, 14, 20),
        color(ColorRole::DiffAddedHighlight, 158, 206, 106),
        color(ColorRole::DiffRemovedHighlight, 247, 118, 142),
        color(ColorRole::DiffHunkHeader, 122, 162, 247),
    ],
};
const SPACING: SpacingTokens = SpacingTokens {
    unit: 1,
    composer_padding_x: 2,
    sidebar_padding_x: 2,
    sidebar_padding_y: 1,
    footer_prefix_gap: 2,
    transcript_gutter_x: 2,
    transcript_gutter_y: 1,
    status_separator: 2,
    modal_margin: 2,
    surface_margin_x: 2,
    surface_margin_y: 1,
    surface_gap: 1,
    header_rows: 1,
    tabs_rows: 3,
    status_rows: 1,
    footer_rows: 1,
    prompt_input_rows: 3,
};
const BORDERS: BorderTokens = BorderTokens {
    none: border(BorderRole::None, None, 0),
    subtle: border(BorderRole::Subtle, Some(ColorRole::BorderSubtle), 1),
    strong: border(BorderRole::Strong, Some(ColorRole::BorderStrong), 2),
    focus: border(BorderRole::Focus, Some(ColorRole::BorderFocus), 3),
};
#[rustfmt::skip]
const GLYPHS: GlyphRoles = GlyphRoles {
    all: &[glyph(GlyphRole::Streaming, "◐", "o"), glyph(GlyphRole::Done, "●", "*"), glyph(GlyphRole::Error, "✗", "x"), glyph(GlyphRole::PendingPermission, "◷", "?"), glyph(GlyphRole::Queued, "◴", "."), glyph(GlyphRole::Running, "◐", "o"), glyph(GlyphRole::Succeeded, "●", "*"), glyph(GlyphRole::Failed, "✗", "x"), glyph(GlyphRole::Cancelled, "⊘", "-"), glyph(GlyphRole::UserMarker, "❯", ">"), glyph(GlyphRole::ToolMarker, "◆", "*"), glyph(GlyphRole::CardTop, "  ", "  "), glyph(GlyphRole::CardMiddle, " ", " "), glyph(GlyphRole::CardBottom, "  ", "  ")],
};
#[rustfmt::skip]
const HIERARCHY: HierarchyTokens = HierarchyTokens {
    primary: HierarchyLevel { rank: 0, color: ColorRole::TextPrimary, modifier: TextModifier::Normal },
    secondary: HierarchyLevel { rank: 1, color: ColorRole::TextSecondary, modifier: TextModifier::Normal },
    tertiary: HierarchyLevel { rank: 2, color: ColorRole::TextTertiary, modifier: TextModifier::Dim },
    accent: HierarchyLevel { rank: 0, color: ColorRole::TextAccent, modifier: TextModifier::Bold },
    inverse: HierarchyLevel { rank: 0, color: ColorRole::TextInverse, modifier: TextModifier::Normal },
    disabled: HierarchyLevel { rank: 3, color: ColorRole::StatusDisabled, modifier: TextModifier::Dim },
};
#[rustfmt::skip]
const STATES: StateColors = StateColors {
    bindings: [
        state(LifecycleState::Idle, ColorRole::TextSecondary, ColorRole::Shell, GlyphRole::UserMarker),
        state(LifecycleState::Drafting, ColorRole::TextPrimary, ColorRole::PanelElevated, GlyphRole::UserMarker),
        state(LifecycleState::Submitting, ColorRole::StatusInfo, ColorRole::Shell, GlyphRole::Streaming),
        state(LifecycleState::Streaming, ColorRole::StatusInfo, ColorRole::Shell, GlyphRole::Streaming),
        state(LifecycleState::Thinking, ColorRole::TextAccent, ColorRole::Shell, GlyphRole::Streaming),
        state(LifecycleState::Tool, ColorRole::TextAccent, ColorRole::Panel, GlyphRole::ToolMarker),
        state(LifecycleState::Diff, ColorRole::TextPrimary, ColorRole::PanelElevated, GlyphRole::ToolMarker),
        state(LifecycleState::Permission, ColorRole::StatusWarning, ColorRole::PanelElevated, GlyphRole::PendingPermission),
        state(LifecycleState::Question, ColorRole::QuestionPrimary, ColorRole::QuestionSurface, GlyphRole::PendingPermission),
        state(LifecycleState::Queued, ColorRole::TextSecondary, ColorRole::Panel, GlyphRole::Queued),
        state(LifecycleState::Interjected, ColorRole::StatusWarning, ColorRole::PanelElevated, GlyphRole::Error),
        state(LifecycleState::Cancelling, ColorRole::StatusWarning, ColorRole::Shell, GlyphRole::Error),
        state(LifecycleState::Recovering, ColorRole::StatusWarning, ColorRole::Shell, GlyphRole::Streaming),
        state(LifecycleState::Failed, ColorRole::StatusError, ColorRole::Shell, GlyphRole::Failed),
        state(LifecycleState::Completed, ColorRole::StatusSuccess, ColorRole::Shell, GlyphRole::Succeeded),
        state(LifecycleState::Compacting, ColorRole::StatusInfo, ColorRole::Panel, GlyphRole::Streaming),
    ],
};
#[rustfmt::skip]
const FOCUS: FocusStyles = FocusStyles {
    all: [focus(FocusRole::Panel, BorderRole::Focus, ColorRole::TextPrimary, ColorRole::Panel, TextModifier::Normal), focus(FocusRole::SelectedRow, BorderRole::None, ColorRole::TextInverse, ColorRole::TextAccent, TextModifier::Bold), focus(FocusRole::Cursor, BorderRole::None, ColorRole::TextPrimary, ColorRole::Shell, TextModifier::Normal), focus(FocusRole::Permission, BorderRole::Focus, ColorRole::QuestionPrimary, ColorRole::QuestionSurface, TextModifier::Bold), focus(FocusRole::Question, BorderRole::Focus, ColorRole::QuestionPrimary, ColorRole::QuestionSelected, TextModifier::Bold)],
};
#[rustfmt::skip]
const MOTION: MotionTokens = MotionTokens {
    all: [MotionToken { kind: MotionKind::ActiveTick, interval_ms: 33, frames: 0 }, MotionToken { kind: MotionKind::StreamingSpinner, interval_ms: 133, frames: 8 }, MotionToken { kind: MotionKind::ToolPulse, interval_ms: 33, frames: 16 }, MotionToken { kind: MotionKind::ToolFinishFlash, interval_ms: 33, frames: 12 }, MotionToken { kind: MotionKind::StartupShimmer, interval_ms: 100, frames: 8 }, MotionToken { kind: MotionKind::ToastLifetime, interval_ms: 1000, frames: 1 }],
};
#[rustfmt::skip]
const REDUCED: ReducedMotionSubstitutions = ReducedMotionSubstitutions {
    all: &[ReducedMotionSubstitution { kind: MotionKind::ActiveTick, replacement: MotionReplacement::NoOp, frame: "" }, ReducedMotionSubstitution { kind: MotionKind::StreamingSpinner, replacement: MotionReplacement::StaticFrame, frame: "⠋" }, ReducedMotionSubstitution { kind: MotionKind::ToolPulse, replacement: MotionReplacement::StaticFrame, frame: "◆" }, ReducedMotionSubstitution { kind: MotionKind::ToolFinishFlash, replacement: MotionReplacement::Immediate, frame: "◆" }, ReducedMotionSubstitution { kind: MotionKind::StartupShimmer, replacement: MotionReplacement::StaticFrame, frame: "" }, ReducedMotionSubstitution { kind: MotionKind::ToastLifetime, replacement: MotionReplacement::Immediate, frame: "" }],
};

#[rustfmt::skip]
pub const DESIGN_TOKENS: DesignTokens = DesignTokens {
    palette: PALETTE,
    spacing: SPACING,
    borders: BORDERS,
    glyph_roles: GLYPHS,
    hierarchy: HIERARCHY,
    breakpoints: ResponsiveBreakpoints { all: VIEWPORTS },
    state_colors: STATES,
    focus_styles: FOCUS,
    motion_tokens: MOTION,
    reduced_motion_substitutions: REDUCED,
};
