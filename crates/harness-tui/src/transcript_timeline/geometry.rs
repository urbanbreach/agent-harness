use ratatui::layout::Rect;

use crate::theme_tokens::ViewportId;
use crate::transcript_identity::TurnId;

use super::clipping::{clip_marker_label, marker_column_width, marker_display_width};
use super::markers::TimelineTurn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineMarkerRect {
    pub turn_id: TurnId,
    pub rect: Rect,
    pub label: String,
    pub clipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineGeometry {
    pub viewport: Rect,
    pub rail_rect: Rect,
    pub marker_rects: Vec<TimelineMarkerRect>,
    pub scroll_top: usize,
}

pub fn geometry_for_viewport(
    viewport: ViewportId,
    turns: &[TimelineTurn],
    scroll_top: usize,
) -> TimelineGeometry {
    let (width, height) = viewport.dimensions();
    geometry_for_rect(Rect::new(0, 0, width, height), turns, scroll_top)
}

pub fn geometry_for_rect(
    viewport: Rect,
    turns: &[TimelineTurn],
    scroll_top: usize,
) -> TimelineGeometry {
    let rail_width = if viewport.width == 0 { 0 } else { 1 };
    let rail_rect = Rect::new(viewport.x, viewport.y, rail_width, viewport.height);
    let label_budget = marker_column_width(viewport.width)
        .min(usize::from(viewport.width.saturating_sub(rail_width)));
    let marker_x = viewport.x.saturating_add(rail_width);
    let marker_rects = if label_budget == 0 || viewport.height == 0 {
        Vec::new()
    } else {
        turns
            .iter()
            .filter_map(|turn| marker_rect(viewport, marker_x, label_budget, turn, scroll_top))
            .collect()
    };

    TimelineGeometry {
        viewport,
        rail_rect,
        marker_rects,
        scroll_top,
    }
}

fn marker_rect(
    viewport: Rect,
    marker_x: u16,
    label_budget: usize,
    turn: &TimelineTurn,
    scroll_top: usize,
) -> Option<TimelineMarkerRect> {
    if turn.row < scroll_top {
        return None;
    }
    let local_row = turn.row.saturating_sub(scroll_top);
    if local_row >= usize::from(viewport.height) {
        return None;
    }

    let source_label = turn.marker_label();
    let label = clip_marker_label(source_label, label_budget);
    let clipped = label != source_label;
    let width = marker_display_width(&label);
    if width == 0 {
        return None;
    }
    let y = viewport
        .y
        .saturating_add(u16::try_from(local_row).unwrap_or(u16::MAX));
    Some(TimelineMarkerRect {
        turn_id: turn.turn_id(),
        rect: Rect::new(marker_x, y, u16::try_from(width).unwrap_or(u16::MAX), 1),
        label,
        clipped,
    })
}
