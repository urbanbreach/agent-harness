use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::theme_tokens::{TextModifier, DESIGN_TOKENS};

use super::Suggestion;

pub fn muted_style() -> Style {
    let hierarchy = DESIGN_TOKENS.hierarchy.tertiary;
    let color = DESIGN_TOKENS
        .palette
        .roles
        .iter()
        .find(|token| token.role == hierarchy.color)
        .map_or(Color::Reset, |token| {
            Color::Rgb(token.value.red, token.value.green, token.value.blue)
        });
    let modifier = match hierarchy.modifier {
        TextModifier::Normal => Modifier::empty(),
        TextModifier::Dim => Modifier::DIM,
        TextModifier::Bold => Modifier::BOLD,
    };
    Style::default().fg(color).add_modifier(modifier)
}

pub fn render_ghost(suggestion: &Suggestion) -> Span<'_> {
    Span::styled(suggestion.text(), muted_style())
}
