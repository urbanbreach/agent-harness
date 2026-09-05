use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Block,
    Frame,
};

use super::ui_transcript_style::blend_color;
use crate::theme::Theme;

const TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH: u16 = 0;
const TRANSCRIPT_SCROLLBAR_TRACK_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_THUMB_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT: usize = 3;
const TRANSCRIPT_SCROLLBAR_FOLLOWING_THUMB_BLEND: f32 = 0.45;

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

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptScrollbarRenderSpec {
    pub(super) viewport: TranscriptViewportLayout,
    pub(super) scroll_top: usize,
    pub(super) max_scroll: usize,
    pub(super) base_surface: Color,
    pub(super) following: bool,
    pub(super) drag_active: bool,
}

pub(super) fn render_transcript_scrollbar(
    frame: &mut Frame,
    theme: &Theme,
    spec: TranscriptScrollbarRenderSpec,
) {
    if let Some(chrome) = spec.viewport.scrollbar_chrome {
        frame.render_widget(
            Block::default().style(Style::default().bg(spec.base_surface)),
            chrome,
        );
    }

    let Some(scrollbar) =
        transcript_scrollbar_geometry(spec.viewport, spec.scroll_top, spec.max_scroll)
    else {
        return;
    };

    let glyphs = theme.live_shell.transcript_glyphs;
    let buffer = frame.buffer_mut();
    for y in scrollbar.track.y..scrollbar.track.bottom() {
        for x in scrollbar.track.x..scrollbar.track.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(glyphs.scrollbar_track);
            cell.set_fg(theme.scrollbar.track);
            cell.set_bg(spec.base_surface);
        }
    }

    let thumb_style = if spec.drag_active {
        Style::default()
            .fg(theme.scrollbar.thumb_active)
            .bg(spec.base_surface)
            .add_modifier(Modifier::BOLD)
    } else if spec.following {
        Style::default()
            .fg(blend_color(
                theme.scrollbar.track,
                theme.scrollbar.thumb,
                TRANSCRIPT_SCROLLBAR_FOLLOWING_THUMB_BLEND,
            ))
            .bg(spec.base_surface)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
            .fg(theme.scrollbar.thumb)
            .bg(spec.base_surface)
    };
    for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
        for x in scrollbar.thumb.x..scrollbar.thumb.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(glyphs.scrollbar_thumb);
            cell.set_style(thumb_style);
        }
    }
}

pub(super) fn render_transcript_more_below_affordance(
    frame: &mut Frame,
    content: Rect,
    scroll_top: usize,
    max_scroll: usize,
    theme: &Theme,
    base_surface: Color,
    hovered: bool,
) {
    let Some(area) = transcript_more_below_rect(content, scroll_top, max_scroll) else {
        return;
    };

    let cell = &mut frame.buffer_mut()[(area.x, area.y)];
    cell.set_symbol(theme.live_shell.transcript_glyphs.more_below);
    cell.set_fg(if hovered {
        theme.text.primary
    } else {
        theme.text.secondary
    });
    cell.set_bg(base_surface);
}

pub(super) fn transcript_more_below_rect(
    content: Rect,
    scroll_top: usize,
    max_scroll: usize,
) -> Option<Rect> {
    if max_scroll == 0 || scroll_top >= max_scroll || content.width == 0 || content.height == 0 {
        return None;
    }

    Some(Rect::new(
        content.x.saturating_add(content.width / 2),
        content.bottom().saturating_sub(1),
        1,
        1,
    ))
}

pub(super) fn transcript_more_below_hit_rect(
    content: Rect,
    scroll_top: usize,
    max_scroll: usize,
) -> Option<Rect> {
    transcript_more_below_rect(content, scroll_top, max_scroll)
        .map(|paint| Rect::new(paint.x.saturating_sub(1), paint.y, 3, 1))
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
    use ratatui::{backend::TestBackend, buffer::Buffer, style::Modifier, Terminal};
    use unicode_width::UnicodeWidthStr;

    fn render_scrollbar_cells(theme: &Theme, following: bool, drag_active: bool) -> Buffer {
        let backend = TestBackend::new(6, 6);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let viewport = transcript_viewport_layout(Rect::new(0, 0, 6, 6), true);

        terminal
            .draw(|frame| {
                render_transcript_scrollbar(
                    frame,
                    theme,
                    TranscriptScrollbarRenderSpec {
                        viewport,
                        scroll_top: 3,
                        max_scroll: 6,
                        base_surface: Color::Black,
                        following,
                        drag_active,
                    },
                );
                render_transcript_more_below_affordance(
                    frame,
                    viewport.content,
                    3,
                    6,
                    theme,
                    Color::Black,
                    false,
                );
            })
            .expect("draw");

        terminal.backend().buffer().clone()
    }

    #[test]
    fn following_thumb_is_dimmed_toward_the_track() {
        // Given: the transcript is following new output.
        let theme = Theme::default();

        // When: the scrollbar cells are rendered.
        let buffer = render_scrollbar_cells(&theme, true, false);
        let track = &buffer[(5, 0)];
        let thumb = &buffer[(5, 2)];

        // Then: the rail remains visible while the normal thumb token is dimmed.
        assert_eq!(track.symbol(), "│");
        assert_eq!(track.fg, theme.scrollbar.track);
        assert_eq!(thumb.symbol(), "█");
        assert_eq!(
            thumb.fg,
            blend_color(
                theme.scrollbar.track,
                theme.scrollbar.thumb,
                TRANSCRIPT_SCROLLBAR_FOLLOWING_THUMB_BLEND,
            )
        );
        assert_ne!(thumb.fg, theme.scrollbar.track);
        assert_ne!(thumb.fg, theme.scrollbar.thumb);
        assert!(thumb.modifier.contains(Modifier::DIM));
        assert!(!thumb.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn detached_thumb_uses_normal_scrollbar_emphasis() {
        // Given: the transcript is detached from new output.
        let theme = Theme::default();

        // When: the scrollbar cells are rendered.
        let buffer = render_scrollbar_cells(&theme, false, false);
        let thumb = &buffer[(5, 2)];

        // Then: the thumb uses the normal token without emphasis modifiers.
        assert_eq!(thumb.symbol(), "█");
        assert_eq!(thumb.fg, theme.scrollbar.thumb);
        assert!(!thumb.modifier.intersects(Modifier::DIM | Modifier::BOLD));
    }

    #[test]
    fn drag_active_thumb_uses_strongest_scrollbar_emphasis() {
        // Given: the transcript scrollbar is being dragged.
        let theme = Theme::default();

        // When: the scrollbar cells are rendered.
        let buffer = render_scrollbar_cells(&theme, true, true);
        let thumb = &buffer[(5, 2)];

        // Then: dragging overrides following with the active token and bold emphasis.
        assert_eq!(thumb.symbol(), "█");
        assert_eq!(thumb.fg, theme.scrollbar.thumb_active);
        assert!(thumb.modifier.contains(Modifier::BOLD));
        assert!(!thumb.modifier.contains(Modifier::DIM));
    }

    #[test]
    fn preferred_scrollbar_glyphs_are_exactly_one_cell_wide() {
        // Given: the preferred Harness glyph mode.
        let theme = Theme::default();

        // When: scrollbar and more-below cells are rendered.
        let buffer = render_scrollbar_cells(&theme, false, false);
        let symbols = [
            buffer[(5, 0)].symbol(),
            buffer[(5, 2)].symbol(),
            buffer[(2, 5)].symbol(),
        ];

        // Then: the semantic catalog supplies the expected narrow glyphs.
        assert_eq!(symbols, ["│", "█", "▼"]);
        assert!(symbols.iter().all(|symbol| symbol.width() == 1));
    }

    #[test]
    fn ascii_scrollbar_glyphs_are_exactly_one_cell_wide() {
        // Given: the ASCII Harness glyph mode.
        let theme = Theme::default().with_glyph_mode(crate::theme::GlyphMode::Ascii);

        // When: scrollbar and more-below cells are rendered.
        let buffer = render_scrollbar_cells(&theme, false, false);
        let symbols = [
            buffer[(5, 0)].symbol(),
            buffer[(5, 2)].symbol(),
            buffer[(2, 5)].symbol(),
        ];

        // Then: each fallback is exact ASCII and occupies one terminal cell.
        assert_eq!(symbols, ["|", "#", "v"]);
        assert!(symbols.iter().all(|symbol| symbol.width() == 1));
    }

    #[test]
    fn transcript_scrollbar_layout_matches_shell_lane_width() {
        let viewport = transcript_viewport_layout(Rect::new(4, 2, 20, 12), true);

        assert_eq!(viewport.content, Rect::new(4, 2, 19, 12));
        assert_eq!(viewport.scrollbar_chrome, Some(Rect::new(23, 2, 1, 12)));
        assert_eq!(viewport.scrollbar_lane, Some(Rect::new(23, 2, 1, 12)));
    }

    #[test]
    fn more_below_affordance_paints_centered_glyph_when_not_at_bottom() {
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let theme = Theme::default();
        let content = Rect::new(0, 0, 20, 6);
        let center_x = content.x + content.width / 2;
        let bottom_y = content.bottom().saturating_sub(1);

        terminal
            .draw(|frame| {
                render_transcript_more_below_affordance(
                    frame,
                    content,
                    0,
                    10,
                    &theme,
                    Color::Black,
                    false,
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let cell = &buffer[(center_x, bottom_y)];
        assert_eq!(
            cell.symbol(),
            theme.live_shell.transcript_glyphs.more_below,
            "mid-scroll viewport must paint centered more-below affordance at ({center_x},{bottom_y})"
        );
    }

    #[test]
    fn more_below_affordance_hidden_when_pinned_to_bottom() {
        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let theme = Theme::default();
        let content = Rect::new(0, 0, 20, 6);
        let center_x = content.x + content.width / 2;
        let bottom_y = content.bottom().saturating_sub(1);

        terminal
            .draw(|frame| {
                render_transcript_more_below_affordance(
                    frame,
                    content,
                    10,
                    10,
                    &theme,
                    Color::Black,
                    false,
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let cell = &buffer[(center_x, bottom_y)];
        assert_ne!(
            cell.symbol(),
            theme.live_shell.transcript_glyphs.more_below,
            "bottom-pinned viewport must not paint more-below affordance"
        );
    }

    #[test]
    fn more_below_hit_area_matches_grok_pointer_geometry() {
        // arrange
        // Given: detached content with more transcript rows below the viewport.
        let content = Rect::new(4, 2, 19, 12);

        // When: resolving the separate paint and pointer geometries.
        let paint = transcript_more_below_rect(content, 3, 8);
        let hit = transcript_more_below_hit_rect(content, 3, 8);

        // act
        // Then: one cell is painted inside Grok's centered three-cell pointer target.
        // assert
        assert_eq!(paint, Some(Rect::new(13, 13, 1, 1)));
        assert_eq!(hit, Some(Rect::new(12, 13, 3, 1)));
        assert_eq!(transcript_more_below_hit_rect(content, 8, 8), None);
    }

    #[test]
    fn more_below_even_width_uses_groks_right_center_cell() {
        // arrange — Given an even-width detached viewport with content below.
        let content = Rect::new(4, 2, 20, 12);

        // act — When resolving the painted affordance cell.
        let paint = transcript_more_below_rect(content, 3, 8);

        // assert — Then Grok's width/2 calculation selects the right center cell.
        assert_eq!(paint, Some(Rect::new(14, 13, 1, 1)));
    }
}
