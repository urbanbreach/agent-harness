use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Block,
    Frame,
};

use crate::theme::Theme;

const TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH: u16 = 0;
const TRANSCRIPT_SCROLLBAR_TRACK_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_THUMB_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT: usize = 3;
const TRANSCRIPT_SCROLLBAR_THUMB_GLYPH: &str = "█";

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptViewportLayout {
    pub(super) content: Rect,
    pub(super) scrollbar_chrome: Option<Rect>,
    pub(super) scrollbar_lane: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptScrollbarHit {
    pub lane: Rect,
    pub track: Rect,
    pub thumb: Rect,
    pub max_scroll: usize,
}

pub(super) fn render_transcript_scrollbar(
    frame: &mut Frame,
    viewport: TranscriptViewportLayout,
    scroll_top: usize,
    max_scroll: usize,
    theme: &Theme,
    base_surface: Color,
    drag_active: bool,
) {
    if let Some(chrome) = viewport.scrollbar_chrome {
        frame.render_widget(
            Block::default().style(Style::default().bg(base_surface)),
            chrome,
        );
    }

    let Some(scrollbar) = transcript_scrollbar_geometry(viewport, scroll_top, max_scroll) else {
        return;
    };

    let buffer = frame.buffer_mut();
    for y in scrollbar.track.y..scrollbar.track.bottom() {
        for x in scrollbar.track.x..scrollbar.track.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(" ");
            cell.set_bg(theme.scrollbar.track);
        }
    }

    let thumb_color = if drag_active {
        theme.scrollbar.thumb_active
    } else {
        theme.scrollbar.thumb
    };
    for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
        for x in scrollbar.thumb.x..scrollbar.thumb.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(TRANSCRIPT_SCROLLBAR_THUMB_GLYPH);
            cell.set_fg(thumb_color);
            cell.set_bg(theme.scrollbar.track);
        }
    }
}

pub(super) fn transcript_scrollbar_geometry(
    viewport: TranscriptViewportLayout,
    scroll_top: usize,
    max_scroll: usize,
) -> Option<TranscriptScrollbarHit> {
    let lane = viewport.scrollbar_lane?;
    if lane.width == 0 || lane.height == 0 || viewport.content.height == 0 {
        return None;
    }

    let track = lane;
    if track.width == 0 || track.height == 0 {
        return None;
    }

    let viewport_height = usize::from(viewport.content.height);
    let track_height = usize::from(track.height);
    let total_height = max_scroll.saturating_add(viewport_height);
    let min_thumb_height = TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT.min(track_height.max(1));
    let thumb_height = if max_scroll == 0 {
        track_height
    } else {
        viewport_height
            .saturating_mul(track_height)
            .div_ceil(total_height.max(1))
            .clamp(min_thumb_height, track_height)
    };
    let thumb_top = if max_scroll == 0 || thumb_height >= track_height {
        0
    } else {
        scroll_top
            .saturating_mul(track_height.saturating_sub(thumb_height))
            .div_ceil(max_scroll)
            .min(track_height.saturating_sub(thumb_height))
    };
    let thumb_y = track
        .y
        .saturating_add(u16::try_from(thumb_top).unwrap_or(u16::MAX));
    let thumb_width = TRANSCRIPT_SCROLLBAR_THUMB_WIDTH.min(track.width).max(1);
    let thumb = Rect::new(
        track.right().saturating_sub(thumb_width),
        thumb_y,
        thumb_width,
        u16::try_from(thumb_height).unwrap_or(track.height),
    );

    Some(TranscriptScrollbarHit {
        lane,
        track,
        thumb,
        max_scroll,
    })
}

pub(super) fn transcript_viewport_layout(
    area: Rect,
    show_scrollbar: bool,
) -> TranscriptViewportLayout {
    let reserved_width = TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH + TRANSCRIPT_SCROLLBAR_TRACK_WIDTH;
    if !show_scrollbar || area.width <= reserved_width {
        return TranscriptViewportLayout {
            content: area,
            scrollbar_chrome: None,
            scrollbar_lane: None,
        };
    }

    let content_width = area.width.saturating_sub(reserved_width);
    let content = Rect::new(area.x, area.y, content_width, area.height);
    let scrollbar_chrome = Rect::new(content.right(), area.y, reserved_width, area.height);
    let scrollbar_lane = Rect::new(
        content
            .right()
            .saturating_add(TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH),
        area.y,
        TRANSCRIPT_SCROLLBAR_TRACK_WIDTH,
        area.height,
    );

    TranscriptViewportLayout {
        content,
        scrollbar_chrome: Some(scrollbar_chrome),
        scrollbar_lane: Some(scrollbar_lane),
    }
}

pub(super) fn transcript_scrollbar_needed(total_height: usize, area: Rect) -> bool {
    area.width > TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH + TRANSCRIPT_SCROLLBAR_TRACK_WIDTH
        && total_height > usize::from(area.height)
}

pub(super) fn current_transcript_scroll_top(
    follow_mode: bool,
    transcript_scroll: usize,
    max_scroll: usize,
) -> usize {
    if max_scroll == 0 {
        return 0;
    }

    if follow_mode {
        return max_scroll;
    }

    max_scroll.saturating_sub(transcript_scroll).min(max_scroll)
}

pub(super) fn transcript_scroll_offset(
    follow_mode: bool,
    transcript_scroll: usize,
    total_height: usize,
    viewport_height: u16,
) -> usize {
    let viewport_height = usize::from(viewport_height);
    if viewport_height == 0 {
        return 0;
    }

    let max_scroll = total_height.saturating_sub(viewport_height);
    if max_scroll == 0 {
        return 0;
    }

    if follow_mode {
        return max_scroll;
    }

    current_transcript_scroll_top(follow_mode, transcript_scroll, max_scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_scrollbar_layout_matches_shell_lane_width() {
        let viewport = transcript_viewport_layout(Rect::new(4, 2, 20, 12), true);

        assert_eq!(viewport.content, Rect::new(4, 2, 19, 12));
        assert_eq!(viewport.scrollbar_chrome, Some(Rect::new(23, 2, 1, 12)));
        assert_eq!(viewport.scrollbar_lane, Some(Rect::new(23, 2, 1, 12)));
    }
}
