use serde::{Deserialize, Serialize};

use super::{LifecycleState, ResponsiveBreakpoints, ViewportId};

#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorRole {
    Canvas, Shell, Panel, PanelElevated, Overlay, Card, SelectedCard, BorderSubtle, BorderStrong, BorderFocus, TextPrimary, TextSecondary, TextTertiary, TextAccent, TextInverse, StatusSuccess, StatusWarning, StatusError, StatusInfo, StatusDisabled, QuestionSurface, QuestionSelected, QuestionPrimary, QuestionAccent, QuestionSecondary, AgentBuild, AgentPlan, AgentDocs, AgentAsk, TerminalPrimary, TerminalSecondary, TerminalMuted, TerminalError, TerminalPaletteSection, TerminalForkAccent, DiffAdded, DiffRemoved, DiffAddedGutter, DiffRemovedGutter, DiffAddedHighlight, DiffRemovedHighlight, DiffHunkHeader,
}

#[rustfmt::skip]
impl ColorRole {
    pub const ALL: [Self; 42] = [Self::Canvas, Self::Shell, Self::Panel, Self::PanelElevated, Self::Overlay, Self::Card, Self::SelectedCard, Self::BorderSubtle, Self::BorderStrong, Self::BorderFocus, Self::TextPrimary, Self::TextSecondary, Self::TextTertiary, Self::TextAccent, Self::TextInverse, Self::StatusSuccess, Self::StatusWarning, Self::StatusError, Self::StatusInfo, Self::StatusDisabled, Self::QuestionSurface, Self::QuestionSelected, Self::QuestionPrimary, Self::QuestionAccent, Self::QuestionSecondary, Self::AgentBuild, Self::AgentPlan, Self::AgentDocs, Self::AgentAsk, Self::TerminalPrimary, Self::TerminalSecondary, Self::TerminalMuted, Self::TerminalError, Self::TerminalPaletteSection, Self::TerminalForkAccent, Self::DiffAdded, Self::DiffRemoved, Self::DiffAddedGutter, Self::DiffRemovedGutter, Self::DiffAddedHighlight, Self::DiffRemovedHighlight, Self::DiffHunkHeader];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorToken {
    pub role: ColorRole,
    pub value: PaletteColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaletteTokens {
    pub roles: &'static [ColorToken],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[rustfmt::skip]
pub enum BorderRole {
    None, Subtle, Strong, Focus,
}

impl BorderRole {
    pub const ALL: [Self; 4] = [Self::None, Self::Subtle, Self::Strong, Self::Focus];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorderToken {
    pub role: BorderRole,
    pub color: Option<ColorRole>,
    pub intensity: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorderTokens {
    pub none: BorderToken,
    pub subtle: BorderToken,
    pub strong: BorderToken,
    pub focus: BorderToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[rustfmt::skip]
pub enum GlyphRole {
    Streaming, Done, Error, PendingPermission, Queued, Running, Succeeded, Failed, UserMarker, ToolMarker, CardTop, CardMiddle, CardBottom,
}

#[rustfmt::skip]
impl GlyphRole {
    pub const ALL: [Self; 13] = [Self::Streaming, Self::Done, Self::Error, Self::PendingPermission, Self::Queued, Self::Running, Self::Succeeded, Self::Failed, Self::UserMarker, Self::ToolMarker, Self::CardTop, Self::CardMiddle, Self::CardBottom];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GlyphToken {
    pub role: GlyphRole,
    pub preferred: &'static str,
    pub ascii: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GlyphRoles {
    pub all: &'static [GlyphToken],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[rustfmt::skip]
pub enum TextModifier {
    Normal, Dim, Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchyLevel {
    pub rank: u8,
    pub color: ColorRole,
    pub modifier: TextModifier,
}

pub type HierarchyToken = HierarchyLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchyTokens {
    pub primary: HierarchyLevel,
    pub secondary: HierarchyLevel,
    pub tertiary: HierarchyLevel,
    pub accent: HierarchyLevel,
    pub inverse: HierarchyLevel,
    pub disabled: HierarchyLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpacingTokens {
    pub unit: u16,
    pub composer_padding_x: u16,
    pub sidebar_padding_x: u16,
    pub sidebar_padding_y: u16,
    pub footer_prefix_gap: u16,
    pub transcript_gutter_x: u16,
    pub transcript_gutter_y: u16,
    pub status_separator: u16,
    pub modal_margin: u16,
    pub surface_margin_x: u16,
    pub surface_margin_y: u16,
    pub surface_gap: u16,
    pub header_rows: u16,
    pub tabs_rows: u16,
    pub status_rows: u16,
    pub footer_rows: u16,
    pub prompt_input_rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[rustfmt::skip]
pub enum FocusRole {
    Panel, SelectedRow, Cursor, Permission, Question,
}

#[rustfmt::skip]
impl FocusRole {
    pub const ALL: [Self; 5] = [Self::Panel, Self::SelectedRow, Self::Cursor, Self::Permission, Self::Question];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusStyle {
    pub role: FocusRole,
    pub border: BorderRole,
    pub foreground: ColorRole,
    pub background: ColorRole,
    pub modifier: TextModifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusStyles {
    pub all: [FocusStyle; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[rustfmt::skip]
pub enum MotionKind {
    ActiveTick, StreamingSpinner, ToolPulse, ToolFinishFlash, StartupShimmer, ToastLifetime,
}

#[rustfmt::skip]
impl MotionKind {
    pub const ALL: [Self; 6] = [Self::ActiveTick, Self::StreamingSpinner, Self::ToolPulse, Self::ToolFinishFlash, Self::StartupShimmer, Self::ToastLifetime];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionToken {
    pub kind: MotionKind,
    pub interval_ms: u16,
    pub frames: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionTokens {
    pub all: [MotionToken; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[rustfmt::skip]
pub enum MotionReplacement {
    StaticFrame, Immediate, NoOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReducedMotionSubstitution {
    pub kind: MotionKind,
    pub replacement: MotionReplacement,
    pub frame: &'static str,
}

#[rustfmt::skip]
impl ReducedMotionSubstitution {
    pub const fn is_static(self) -> bool {
        matches!(self.replacement, MotionReplacement::StaticFrame | MotionReplacement::Immediate | MotionReplacement::NoOp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ReducedMotionSubstitutions {
    pub all: &'static [ReducedMotionSubstitution],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateColorBinding {
    pub state: LifecycleState,
    pub foreground: ColorRole,
    pub background: ColorRole,
    pub glyph: GlyphRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateColors {
    pub bindings: [StateColorBinding; 16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DesignTokens {
    pub palette: PaletteTokens,
    pub spacing: SpacingTokens,
    pub borders: BorderTokens,
    pub glyph_roles: GlyphRoles,
    pub hierarchy: HierarchyTokens,
    pub breakpoints: ResponsiveBreakpoints,
    pub state_colors: StateColors,
    pub focus_styles: FocusStyles,
    pub motion_tokens: MotionTokens,
    pub reduced_motion_substitutions: ReducedMotionSubstitutions,
}
