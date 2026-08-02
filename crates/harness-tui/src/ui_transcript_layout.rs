use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::theme::Theme;

use super::ui_transcript::{TranscriptRenderSurface, TranscriptRenderSurfaceKind};
use super::ui_transcript_interaction::TranscriptInteractionRow;
use super::ui_transcript_selection::TranscriptSelectionRow;
use super::ui_transcript_surface::{
    render_transcript_surface, render_transcript_surface_lines, transcript_surface_content_width,
    transcript_surface_leading_gap, transcript_surface_render_width,
};

const SELECTED_RAIL_GLYPH: &str = "❙";

const TRANSCRIPT_SECTION_GAP_HEIGHT: usize = 2;

#[derive(Debug, Clone)]
pub(super) struct MeasuredTranscriptSection {
    pub(super) top_row: usize,
    pub(super) leading_gap_height: usize,
    pub(super) content_height: usize,
    pub(super) surfaces: Vec<MeasuredTranscriptSurface>,
    pub(super) lines: Vec<Line<'static>>,
}

impl MeasuredTranscriptSection {
    pub(super) fn total_height(&self) -> usize {
        self.leading_gap_height + self.content_height
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct MeasuredTranscriptLayout {
    pub(super) sections: Vec<MeasuredTranscriptSection>,
    pub(super) total_height: usize,
}

impl MeasuredTranscriptLayout {
    fn rendered_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for section in &self.sections {
            if section.leading_gap_height > 0 {
                lines.push(Line::default());
            }
            lines.extend(section.lines.iter().cloned());
        }
        lines
    }
}

#[derive(Debug, Clone)]
pub(super) struct MeasuredTranscriptSurface {
    pub(super) kind: TranscriptRenderSurfaceKind,
    pub(super) top_offset: usize,
    pub(super) height: usize,
    pub(super) width: u16,
    pub(super) show_outer_rail: bool,
    pub(super) rail_glyph: &'static str,
    pub(super) rail_color: Color,
    pub(super) surface: Color,
    pub(super) lines: Vec<Line<'static>>,
    pub(super) interaction_rows: Option<Vec<Option<TranscriptInteractionRow>>>,
    pub(super) selection_rows: Option<Vec<TranscriptSelectionRow>>,
    pub(super) diff_hunk_offsets: Vec<usize>,
    pub(super) selected_rail: bool,
}

pub(super) fn measure_transcript_layout<Section>(
    sections: &[Section],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    mut render_surfaces: impl FnMut(&Section, &Theme, u16, Color) -> Vec<TranscriptRenderSurface>,
) -> MeasuredTranscriptLayout {
    let mut top_row = 0;
    let mut measured_sections = Vec::with_capacity(sections.len());

    for section in sections {
        let surfaces = render_surfaces(section, theme, width, base_surface);
        let lines = render_transcript_surface_lines(&surfaces);
        let mut content_height = 0usize;
        let mut measured_surfaces = Vec::with_capacity(surfaces.len());
        let mut previous_surface_kind = None;
        for surface in surfaces.into_iter() {
            let top_offset = content_height
                + transcript_surface_leading_gap(previous_surface_kind, surface.kind);
            let render_width = transcript_surface_render_width(width, surface.kind);
            let content_width = usize::from(transcript_surface_content_width(
                render_width,
                surface.show_outer_rail,
            ));
            let height = transcript_visual_rows(&surface.lines, content_width);
            content_height = top_offset + height;
            measured_surfaces.push(MeasuredTranscriptSurface {
                kind: surface.kind,
                top_offset,
                height,
                width: render_width,
                show_outer_rail: surface.show_outer_rail,
                rail_glyph: surface.rail_glyph,
                rail_color: surface.rail_color,
                surface: surface.surface,
                lines: surface.lines,
                interaction_rows: surface.interaction_rows,
                selection_rows: surface.selection_rows,
                diff_hunk_offsets: surface.diff_hunk_offsets,
                selected_rail: surface.selected_rail,
            });
            previous_surface_kind = Some(surface.kind);
        }
        let leading_gap_height =
            usize::from(!measured_sections.is_empty()) * TRANSCRIPT_SECTION_GAP_HEIGHT;
        let measured_section = MeasuredTranscriptSection {
            top_row,
            leading_gap_height,
            content_height,
            surfaces: measured_surfaces,
            lines,
        };
        top_row += measured_section.total_height();
        measured_sections.push(measured_section);
    }

    MeasuredTranscriptLayout {
        sections: measured_sections,
        total_height: top_row,
    }
}

pub(super) fn transcript_layout_lines(
    layout: &MeasuredTranscriptLayout,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = layout.rendered_lines();

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Waiting for first turn…",
            Style::default().fg(theme.text.secondary),
        )));
    }

    lines
}

pub(super) fn render_transcript_layout_surfaces(
    frame: &mut Frame,
    layout: &MeasuredTranscriptLayout,
    area: Rect,
    scroll_top: usize,
    theme: &Theme,
) {
    let viewport_height = usize::from(area.height);
    if viewport_height == 0 || area.width == 0 {
        return;
    }

    // Reference permission state: pin pending-permission Run Write footer to viewport bottom above dock.
    let footer_pin = pending_permission_footer_pin_delta(layout, viewport_height, scroll_top);
    // Reference scroll state: keep the latest turn's user prompt sticky at the top while body scrolls.
    // Freeze packs one blank separator under the sticky user before inventory (f39 at L9, not L8).
    const STICKY_USER_SEPARATOR_ROWS: usize = 1;
    let sticky_user = sticky_user_surface(layout, scroll_top, viewport_height);
    let sticky_height = sticky_user.map(|(_, _, surface)| surface.height.min(viewport_height));
    let sticky_block_height = sticky_height.map(|h| {
        h.saturating_add(STICKY_USER_SEPARATOR_ROWS)
            .min(viewport_height)
    });

    if let Some((_, _, user_surface)) = sticky_user {
        let sticky_h = sticky_height.unwrap_or(0);
        if sticky_h > 0 {
            let local_scroll = user_surface.height.saturating_sub(sticky_h);
            let surface_rect = Rect::new(
                area.x,
                area.y,
                user_surface.width.min(area.width),
                u16::try_from(sticky_h).unwrap_or(u16::MAX),
            );
            render_transcript_surface(frame, user_surface, surface_rect, local_scroll, theme);
        }
    }

    let body_area = if let Some(block_h) = sticky_block_height.filter(|h| *h > 0) {
        let h = u16::try_from(block_h).unwrap_or(u16::MAX);
        Rect::new(
            area.x,
            area.y.saturating_add(h),
            area.width,
            area.height.saturating_sub(h),
        )
    } else {
        area
    };
    let body_viewport_height = usize::from(body_area.height);
    if body_viewport_height == 0 {
        return;
    }

    let viewport_bottom = scroll_top.saturating_add(body_viewport_height);
    let sticky_section = sticky_user.map(|(s_idx, _, _)| s_idx);
    let sticky_surface_idx = sticky_user.map(|(_, surf_idx, _)| surf_idx);

    for (section_idx, section) in layout.sections.iter().enumerate() {
        let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
        for (surface_idx, surface) in section.surfaces.iter().enumerate() {
            if sticky_section == Some(section_idx) && sticky_surface_idx == Some(surface_idx) {
                continue;
            }
            let mut surface_top = section_content_top.saturating_add(surface.top_offset);
            if let Some((s_idx, f_idx, delta)) = footer_pin {
                if s_idx == section_idx && f_idx == surface_idx {
                    surface_top = surface_top.saturating_add(delta);
                }
            }
            let surface_bottom = surface_top.saturating_add(surface.height);
            if surface_bottom <= scroll_top || surface_top >= viewport_bottom {
                continue;
            }

            let visible_top = surface_top.max(scroll_top);
            let visible_bottom = surface_bottom.min(viewport_bottom);
            let local_scroll = visible_top.saturating_sub(surface_top);
            let y_offset = visible_top.saturating_sub(scroll_top);
            let visible_height = visible_bottom.saturating_sub(visible_top);
            // Freeze PERM Run Write / Waiting footer uses lead=4 while body tools use lead=5.
            let footer_outdent = run_write_footer_outdent(surface);
            let surface_rect = Rect::new(
                body_area.x.saturating_sub(footer_outdent),
                body_area
                    .y
                    .saturating_add(u16::try_from(y_offset).unwrap_or(u16::MAX)),
                surface
                    .width
                    .min(body_area.width)
                    .saturating_add(footer_outdent),
                u16::try_from(visible_height).unwrap_or(u16::MAX),
            );
            render_transcript_surface(frame, surface, surface_rect, local_scroll, theme);
            if surface.selected_rail && local_scroll == 0 && surface_rect.height > 0 {
                paint_selected_rail_glyph(frame, surface_rect, theme);
            }
        }
    }
}

fn sticky_user_surface<'a>(
    layout: &'a MeasuredTranscriptLayout,
    scroll_top: usize,
    viewport_height: usize,
) -> Option<(usize, usize, &'a MeasuredTranscriptSurface)> {
    if scroll_top == 0 || viewport_height == 0 {
        return None;
    }
    let viewport_bottom = scroll_top.saturating_add(viewport_height);
    if viewport_bottom >= layout.total_height {
        return None;
    }
    let section_idx = layout.sections.len().checked_sub(1)?;
    let section = layout.sections.get(section_idx)?;
    let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
    let section_bottom = section_content_top.saturating_add(section.content_height);
    if section_bottom <= scroll_top {
        return None;
    }
    let (surface_idx, user_surface) = section
        .surfaces
        .iter()
        .enumerate()
        .find(|(_, surface)| surface.kind == TranscriptRenderSurfaceKind::User)?;
    let user_top = section_content_top.saturating_add(user_surface.top_offset);
    let user_bottom = user_top.saturating_add(user_surface.height);
    if user_bottom > scroll_top {
        return None;
    }
    let body_still_visible = section.surfaces.iter().enumerate().any(|(idx, surface)| {
        if idx == surface_idx {
            return false;
        }
        let top = section_content_top.saturating_add(surface.top_offset);
        let bottom = top.saturating_add(surface.height);
        bottom > scroll_top && top < viewport_bottom
    });
    if !body_still_visible {
        return None;
    }
    Some((section_idx, surface_idx, user_surface))
}

/// `(section_idx, surface_idx, pin_delta)` for pending-permission footer bottom pin.
fn pending_permission_footer_pin_delta(
    layout: &MeasuredTranscriptLayout,
    viewport_height: usize,
    scroll_top: usize,
) -> Option<(usize, usize, usize)> {
    if viewport_height == 0 || scroll_top > 0 {
        return None;
    }
    if layout.total_height >= viewport_height {
        return None;
    }
    let section_idx = layout.sections.len().checked_sub(1)?;
    let section = layout.sections.get(section_idx)?;
    let surface_idx = section.surfaces.len().checked_sub(1)?;
    let surface = section.surfaces.get(surface_idx)?;
    let is_pinned_footer = surface.lines.iter().any(|line| {
        line.spans.iter().any(|span| {
            span.content.contains("Run Write")
                || span.content.contains("Waiting on")
                || span.content.contains("Waiting for response")
                || span.content.contains("Retrying (attempt")
                || span.content.contains("Responding")
        })
    });
    if !is_pinned_footer {
        return None;
    }
    let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
    let footer_top = section_content_top.saturating_add(surface.top_offset);
    let footer_bottom = footer_top.saturating_add(surface.height);
    // Pin flush to the inner viewport bottom. The transcript bottom vertical gutter
    // (transcript_gutter_y=1) is the single blank under Run Write before the dock.
    let target_bottom = viewport_height.max(surface.height);
    let target_top = target_bottom.saturating_sub(surface.height);
    let delta = target_top.saturating_sub(footer_top);
    if delta == 0 || footer_bottom.saturating_add(delta) > viewport_height {
        return None;
    }
    Some((section_idx, surface_idx, delta))
}

fn run_write_footer_outdent(surface: &MeasuredTranscriptSurface) -> u16 {
    if surface.kind != TranscriptRenderSurfaceKind::AssistantFooter {
        return 0;
    }
    let is_run_write_footer = surface.lines.iter().any(|line| {
        line.spans
            .iter()
            .any(|span| span.content.contains("Run Write") || span.content.contains("Waiting on"))
    });
    u16::from(is_run_write_footer)
}

fn paint_selected_rail_glyph(frame: &mut Frame, surface_rect: Rect, theme: &Theme) {
    let rail_x = surface_rect.x;
    let rail_rect = Rect::new(rail_x, surface_rect.y, 1, 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            SELECTED_RAIL_GLYPH,
            Style::default().fg(theme.text.secondary),
        ))),
        rail_rect,
    );
}

pub(super) fn transcript_diff_hunk_rows_for_layout(
    layout: &MeasuredTranscriptLayout,
) -> Vec<usize> {
    let mut rows = Vec::new();
    for section in &layout.sections {
        let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
        for surface in &section.surfaces {
            let surface_top = section_content_top.saturating_add(surface.top_offset);
            rows.extend(
                surface
                    .diff_hunk_offsets
                    .iter()
                    .map(|offset| surface_top.saturating_add(*offset)),
            );
        }
    }
    rows
}

fn transcript_visual_rows(lines: &[Line<'static>], viewport_width: usize) -> usize {
    lines
        .iter()
        .map(|line| {
            let width = line.width();
            if width == 0 {
                1
            } else {
                width.div_ceil(viewport_width)
            }
        })
        .sum()
}

#[cfg(test)]
mod pin_tests {
    use super::*;
    use ratatui::style::Color;
    use ratatui::text::Span;

    fn run_write_layout(total_content_rows: usize) -> MeasuredTranscriptLayout {
        let body_height = total_content_rows.saturating_sub(1).max(1);
        MeasuredTranscriptLayout {
            sections: vec![MeasuredTranscriptSection {
                top_row: 0,
                leading_gap_height: 0,
                content_height: total_content_rows,
                surfaces: vec![
                    MeasuredTranscriptSurface {
                        kind: TranscriptRenderSurfaceKind::AssistantTool,
                        top_offset: 0,
                        height: body_height,
                        width: 120,
                        show_outer_rail: false,
                        rail_glyph: " ",
                        rail_color: Color::Reset,
                        surface: Color::Reset,
                        lines: vec![Line::from("Creating demo.txt")],
                        interaction_rows: None,
                        selection_rows: None,
                        diff_hunk_offsets: Vec::new(),
                        selected_rail: false,
                    },
                    MeasuredTranscriptSurface {
                        kind: TranscriptRenderSurfaceKind::AssistantFooter,
                        top_offset: body_height,
                        height: 1,
                        width: 120,
                        show_outer_rail: false,
                        rail_glyph: " ",
                        rail_color: Color::Reset,
                        surface: Color::Reset,
                        lines: vec![Line::from(Span::raw("     ◆ Run Write `demo.txt` 19s"))],
                        interaction_rows: None,
                        selection_rows: None,
                        diff_hunk_offsets: Vec::new(),
                        selected_rail: false,
                    },
                ],
                lines: vec![
                    Line::from("Creating demo.txt"),
                    Line::from("     ◆ Run Write `demo.txt` 19s"),
                ],
            }],
            total_height: total_content_rows,
        }
    }

    #[test]
    fn pending_permission_footer_pins_when_content_shorter_than_viewport() {
        // arrange
        // act
        // assert
        // Given: short PERM turn content in a taller transcript viewport
        let layout = run_write_layout(6);
        // When: computing pin delta for viewport height 20 at follow-top
        let pin = pending_permission_footer_pin_delta(&layout, 20, 0);
        // Then: footer surface is shifted down toward the bottom edge
        let (section_idx, surface_idx, delta) = pin.expect("short content with Run Write must pin");
        assert_eq!(section_idx, 0);
        assert_eq!(surface_idx, 1);
        assert!(delta > 0, "expected positive pin delta, got {delta}");
        // footer top_offset 5 + delta lands flush at viewport bottom (height 1 → top 19)
        assert_eq!(5 + delta, 19);
    }

    #[test]
    fn pending_permission_footer_does_not_pin_when_content_fills_viewport() {
        // arrange
        // act
        // assert
        // Given: content as tall as the viewport
        let layout = run_write_layout(20);
        // When/Then: no pin
        assert_eq!(pending_permission_footer_pin_delta(&layout, 20, 0), None);
    }

    fn scroll_turn_layout(user_height: usize, body_height: usize) -> MeasuredTranscriptLayout {
        let content_height = user_height + body_height;
        MeasuredTranscriptLayout {
            sections: vec![MeasuredTranscriptSection {
                top_row: 0,
                leading_gap_height: 0,
                content_height,
                surfaces: vec![
                    MeasuredTranscriptSurface {
                        kind: TranscriptRenderSurfaceKind::User,
                        top_offset: 0,
                        height: user_height,
                        width: 120,
                        show_outer_rail: false,
                        rail_glyph: " ",
                        rail_color: Color::Reset,
                        surface: Color::Reset,
                        lines: (0..user_height)
                            .map(|i| Line::from(format!("user line {i}")))
                            .collect(),
                        interaction_rows: None,
                        selection_rows: None,
                        diff_hunk_offsets: Vec::new(),
                        selected_rail: false,
                    },
                    MeasuredTranscriptSurface {
                        kind: TranscriptRenderSurfaceKind::AssistantBody,
                        top_offset: user_height,
                        height: body_height,
                        width: 120,
                        show_outer_rail: false,
                        rail_glyph: " ",
                        rail_color: Color::Reset,
                        surface: Color::Reset,
                        lines: (0..body_height)
                            .map(|i| Line::from(format!("body line {i}")))
                            .collect(),
                        interaction_rows: None,
                        selection_rows: None,
                        diff_hunk_offsets: Vec::new(),
                        selected_rail: false,
                    },
                ],
                lines: (0..user_height)
                    .map(|i| Line::from(format!("user line {i}")))
                    .chain((0..body_height).map(|i| Line::from(format!("body line {i}"))))
                    .collect(),
            }],
            total_height: content_height,
        }
    }

    #[test]
    fn sticky_user_activates_when_user_scrolled_off_and_body_visible() {
        // arrange
        // act
        // assert
        let layout = scroll_turn_layout(4, 40);
        let sticky = sticky_user_surface(&layout, 10, 20);
        let (section_idx, surface_idx, surface) =
            sticky.expect("user fully above viewport with body visible must sticky");
        assert_eq!(section_idx, 0);
        assert_eq!(surface_idx, 0);
        assert_eq!(surface.kind, TranscriptRenderSurfaceKind::User);
        assert_eq!(surface.height, 4);
    }

    #[test]
    fn sticky_user_inactive_at_follow_top() {
        // arrange
        // act
        // assert
        let layout = scroll_turn_layout(4, 40);
        assert!(sticky_user_surface(&layout, 0, 20).is_none());
    }

    #[test]
    fn sticky_user_inactive_while_user_still_partially_visible() {
        // arrange
        // act
        // assert
        let layout = scroll_turn_layout(4, 40);
        assert!(sticky_user_surface(&layout, 2, 20).is_none());
    }

    #[test]
    fn sticky_user_inactive_after_turn_fully_scrolled_off() {
        // arrange
        // act
        // assert
        let layout = scroll_turn_layout(4, 40);
        assert!(sticky_user_surface(&layout, 50, 20).is_none());
    }

    #[test]
    fn sticky_user_inactive_at_follow_bottom() {
        // arrange
        // act
        // assert
        let layout = scroll_turn_layout(4, 40);
        assert!(sticky_user_surface(&layout, 24, 20).is_none());
    }
}
