use crate::transcript_block_viewer::{
    render_to_buffer, ViewerBlockContent, ViewerMode, ViewerReturnSnapshot, ViewerState,
};
use crate::transcript_blocks::FoldState;
use crate::transcript_identity::{FocusFollowState, ReplayTurn, TranscriptFocus};
use crate::transcript_scroll::{LogicalAnchor, TranscriptLayout};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

#[test]
fn child_markdown_modes_keep_semantic_styles_and_full_code_rows(
) -> Result<(), Box<dyn std::error::Error>> {
    let theme = crate::theme::Theme::harness_dark();
    let id = ReplayTurn::event(1, 0, 0).block_id(0);
    let layout = TranscriptLayout::from_heights([(id, 1.0)], 1.0)?;
    let snapshot = ViewerReturnSnapshot::new(
        FoldState::Expanded,
        FocusFollowState::new(TranscriptFocus::Timeline, false),
        LogicalAnchor::capture(&layout, 0.0)?,
    );
    let mut viewer = ViewerState::open(
        id,
        ViewerBlockContent::markdown(
            "First line\nsecond line\n\nA **bold** paragraph.\n\n```rust\nlet x = 1;\n\nlet y = 2;\n```\n",
        ),
        snapshot,
    )?;
    viewer.set_child_running(true);
    viewer.set_theme(theme)?;
    viewer.resize(100, 25)?;
    let area = Rect::new(0, 0, 120, 40);
    for mode in [ViewerMode::Wrapped, ViewerMode::Raw] {
        let surface = viewer.render_surface(area);
        assert_eq!(surface.mode, mode);
        assert_eq!(surface.lines[0].text, "First line second line");
        let bold = surface
            .lines
            .iter()
            .filter_map(|line| line.styled.as_ref())
            .flat_map(|line| &line.spans)
            .find(|span| span.content == "bold")
            .ok_or("bold span missing")?;
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            surface.lines.iter().any(|line| line.text == "```rust"),
            mode == ViewerMode::Raw
        );
        let mut buffer = Buffer::empty(area);
        render_to_buffer(&mut buffer, area, &surface, &theme);
        let code_row = (0..area.height)
            .find(|row| {
                (0..area.width).any(|column| {
                    let cell = &buffer[(column, *row)];
                    cell.symbol() == "l" && cell.bg == theme.markdown.code_background
                })
            })
            .ok_or("code row missing")?;
        for row in code_row..code_row + 3 {
            assert_eq!(
                buffer[(90, row)].bg,
                if row == code_row + 1 {
                    theme.surface.shell
                } else {
                    theme.markdown.code_background
                }
            );
        }
        viewer.toggle_mode()?;
    }
    Ok(())
}
