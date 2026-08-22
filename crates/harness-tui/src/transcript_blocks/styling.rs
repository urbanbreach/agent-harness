use super::BlockKind;
use crate::theme_tokens::{
    BorderRole, ColorRole, GlyphRole, HierarchyLevel, TextModifier, DESIGN_TOKENS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockStyle {
    pub separator: &'static str,
    pub indent: u16,
    pub glyph: &'static str,
    pub foreground: ColorRole,
    pub border: BorderRole,
    pub hierarchy: HierarchyLevel,
    pub modifier: TextModifier,
}

pub fn style_for(kind: BlockKind) -> BlockStyle {
    let (separator_role, glyph_role, foreground, border, hierarchy, modifier) = match kind {
        BlockKind::User => (
            GlyphRole::CardTop,
            GlyphRole::UserMarker,
            ColorRole::TextPrimary,
            BorderRole::None,
            DESIGN_TOKENS.hierarchy.primary,
            TextModifier::Normal,
        ),
        BlockKind::Assistant => (
            GlyphRole::CardMiddle,
            GlyphRole::Streaming,
            ColorRole::TextPrimary,
            BorderRole::None,
            DESIGN_TOKENS.hierarchy.primary,
            TextModifier::Normal,
        ),
        BlockKind::Thinking => (
            GlyphRole::CardMiddle,
            GlyphRole::Streaming,
            ColorRole::TextSecondary,
            BorderRole::Subtle,
            DESIGN_TOKENS.hierarchy.secondary,
            TextModifier::Dim,
        ),
        BlockKind::Tool => (
            GlyphRole::CardMiddle,
            GlyphRole::ToolMarker,
            ColorRole::TerminalPrimary,
            BorderRole::Subtle,
            DESIGN_TOKENS.hierarchy.tertiary,
            TextModifier::Normal,
        ),
        BlockKind::Diff => (
            GlyphRole::CardMiddle,
            GlyphRole::ToolMarker,
            ColorRole::DiffHunkHeader,
            BorderRole::Subtle,
            DESIGN_TOKENS.hierarchy.tertiary,
            TextModifier::Normal,
        ),
        BlockKind::System => (
            GlyphRole::CardBottom,
            GlyphRole::Done,
            ColorRole::TextTertiary,
            BorderRole::Strong,
            DESIGN_TOKENS.hierarchy.disabled,
            TextModifier::Dim,
        ),
    };

    BlockStyle {
        separator: glyph_for(separator_role),
        indent: DESIGN_TOKENS.spacing.transcript_gutter_x,
        glyph: glyph_for(glyph_role),
        foreground,
        border,
        hierarchy,
        modifier,
    }
}

fn glyph_for(role: GlyphRole) -> &'static str {
    DESIGN_TOKENS
        .glyph_roles
        .all
        .iter()
        .find(|token| token.role == role)
        .map_or("", |token| token.preferred)
}
