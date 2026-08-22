use ratatui::{backend::TestBackend, layout::Rect, style::Color, Terminal};

use super::ui_transcript_scrollbar::render_transcript_more_below_affordance;
use crate::theme::Theme;

#[test]
fn detached_transcript_return_brightens_when_hovered() -> Result<(), std::convert::Infallible> {
    // Given: a detached transcript with more content below its viewport.
    let backend = TestBackend::new(20, 6);
    let mut terminal = Terminal::new(backend)?;
    let theme = Theme::default();
    let content = Rect::new(0, 0, 20, 6);

    // When: the centered return affordance is rendered under the pointer.
    terminal.draw(|frame| {
        render_transcript_more_below_affordance(frame, content, 0, 10, &theme, Color::Black, true);
    })?;

    // Then: Grok's muted-to-bright hover treatment uses Harness primary text.
    let center_x = content.x + content.width / 2;
    let bottom_y = content.bottom().saturating_sub(1);
    assert_eq!(
        terminal.backend().buffer()[(center_x, bottom_y)].fg,
        theme.text.primary
    );
    Ok(())
}
