use ratatui::layout::Rect;

const WIDE_VIEWPORT_MIN_WIDTH: u16 = 121;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailsLayoutMode {
    Replacement,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetailsLayout {
    pub mode: DetailsLayoutMode,
    pub roster: Option<Rect>,
    pub details: Rect,
}

pub(crate) fn layout_for(viewport: Rect) -> DetailsLayout {
    if viewport.width < WIDE_VIEWPORT_MIN_WIDTH {
        return DetailsLayout {
            mode: DetailsLayoutMode::Replacement,
            roster: None,
            details: viewport,
        };
    }
    let details_width = viewport.width.saturating_sub(8).min(72);
    let details_height = viewport.height.saturating_sub(4).min(32);
    let details = Rect::new(
        viewport.x + viewport.width.saturating_sub(details_width) / 2,
        viewport.y + viewport.height.saturating_sub(details_height) / 2,
        details_width,
        details_height,
    );
    DetailsLayout {
        mode: DetailsLayoutMode::Overlay,
        roster: Some(viewport),
        details,
    }
}
