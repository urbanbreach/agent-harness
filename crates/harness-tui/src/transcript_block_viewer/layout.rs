use ratatui::layout::Rect;

#[derive(Clone, Copy)]
pub(crate) struct ViewerLayout {
    pub(crate) overlay: Rect,
    pub(crate) popup: Rect,
    pub(crate) body: Rect,
    pub(crate) close: Rect,
    pub(crate) shortcuts: Rect,
}

pub(crate) fn viewer_layout(area: Rect) -> ViewerLayout {
    let overlay = Rect {
        height: area.height.saturating_sub(2),
        ..area
    };
    let width = u16::try_from(u32::from(overlay.width) * 95 / 100)
        .unwrap_or(u16::MAX)
        .max(60)
        .min(overlay.width);
    let height = u16::try_from(u32::from(overlay.height) * 92 / 100)
        .unwrap_or(u16::MAX)
        .max(12)
        .min(overlay.height.saturating_sub(2));
    let popup = Rect::new(
        overlay.x + overlay.width.saturating_sub(width) / 2,
        overlay.y + overlay.height.saturating_sub(height) / 2,
        width,
        height,
    );
    ViewerLayout {
        overlay,
        popup,
        body: Rect::new(
            popup.x.saturating_add(3),
            popup.y.saturating_add(2),
            popup.width.saturating_sub(6),
            popup.height.saturating_sub(3),
        ),
        close: Rect::new(
            popup.right().saturating_sub(5),
            popup.y.saturating_add(1),
            3,
            1,
        ),
        shortcuts: Rect::new(
            area.x.saturating_add(2),
            overlay.bottom(),
            area.width.saturating_sub(4),
            1,
        ),
    }
}
