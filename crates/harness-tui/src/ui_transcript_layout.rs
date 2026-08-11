use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::theme::Theme;

use super::ui_transcript::{ToolRailMotion, TranscriptRenderSurface, TranscriptRenderSurfaceKind};
use super::ui_transcript_interaction::TranscriptInteractionRow;
use super::ui_transcript_selection::{
    compact_selection_row, TranscriptSelectionCell, TranscriptSelectionRow,
};
use super::ui_transcript_surface::{
    render_transcript_surface, render_transcript_surface_lines, transcript_surface_content_width,
    transcript_surface_leading_gap, transcript_surface_render_width,
};

const TRANSCRIPT_SECTION_GAP_HEIGHT: usize = 2;

#[derive(Debug, Clone)]
pub(super) struct MeasuredTranscriptSection {
    pub(super) activity_first_seq: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptContentAnchor {
    activity_first_seq: u64,
    surface_kind: TranscriptRenderSurfaceKind,
    surface_ordinal: usize,
    logical_line: usize,
    display_column: usize,
    row_within_surface: usize,
    column_within_surface: usize,
    selection_backed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptViewportRows {
    body_scroll_top: usize,
    sticky_source_top: Option<usize>,
    sticky_height: usize,
    viewport_height: usize,
}

impl TranscriptViewportRows {
    pub(super) const fn linear(viewport_height: usize, scroll_top: usize) -> Self {
        Self {
            body_scroll_top: scroll_top,
            sticky_source_top: None,
            sticky_height: 0,
            viewport_height,
        }
    }

    pub(super) fn absolute_row(self, local_row: usize) -> Option<usize> {
        if local_row >= self.viewport_height {
            return None;
        }
        if local_row < self.sticky_height {
            return self
                .sticky_source_top
                .map(|top| top.saturating_add(local_row));
        }
        Some(
            self.body_scroll_top
                .saturating_add(local_row.saturating_sub(self.sticky_height)),
        )
    }
}

pub(super) fn transcript_viewport_rows(
    layout: &MeasuredTranscriptLayout,
    viewport_height: usize,
    scroll_top: usize,
) -> TranscriptViewportRows {
    let sticky = sticky_user_surface(layout, scroll_top, viewport_height);
    let sticky_height = sticky
        .map(|(_, _, surface)| surface.height.min(viewport_height))
        .unwrap_or(0);
    let sticky_source_top = sticky.map(|(section_idx, _, surface)| {
        let section = &layout.sections[section_idx];
        section
            .top_row
            .saturating_add(section.leading_gap_height)
            .saturating_add(surface.top_offset)
            .saturating_add(surface.height.saturating_sub(sticky_height))
    });
    TranscriptViewportRows {
        body_scroll_top: scroll_top,
        sticky_source_top,
        sticky_height,
        viewport_height,
    }
}

pub(super) fn transcript_layout_has_visible_running_tool(
    layout: &MeasuredTranscriptLayout,
    viewport_height: usize,
    scroll_top: usize,
) -> bool {
    let viewport_bottom = scroll_top.saturating_add(viewport_height);
    layout.sections.iter().any(|section| {
        let section_top = section.top_row.saturating_add(section.leading_gap_height);
        section.surfaces.iter().any(|surface| {
            let surface_top = section_top.saturating_add(surface.top_offset);
            let surface_bottom = surface_top.saturating_add(surface.height);
            matches!(
                surface.tool_rail_motion,
                Some(ToolRailMotion::Running { .. })
            ) && surface_bottom > scroll_top
                && surface_top < viewport_bottom
        })
    })
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

    pub(crate) fn capture_content_anchor(
        &self,
        scroll_top: usize,
    ) -> Option<TranscriptContentAnchor> {
        self.capture_content_position(scroll_top, 0)
    }

    pub(crate) fn capture_selection_anchor(
        &self,
        cell: TranscriptSelectionCell,
    ) -> Option<TranscriptContentAnchor> {
        self.capture_content_position(cell.row, cell.column)
    }

    fn capture_content_position(
        &self,
        absolute_row: usize,
        column: usize,
    ) -> Option<TranscriptContentAnchor> {
        let point = absolute_row.min(self.total_height.saturating_sub(1));
        let section = self
            .sections
            .iter()
            .find(|section| point < section.top_row.saturating_add(section.total_height()))
            .or_else(|| self.sections.last())?;
        let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
        let surface_index = section
            .surfaces
            .iter()
            .position(|surface| {
                point
                    < section_content_top
                        .saturating_add(surface.top_offset)
                        .saturating_add(surface.height)
            })
            .unwrap_or_else(|| section.surfaces.len().saturating_sub(1));
        let surface = section.surfaces.get(surface_index)?;
        let surface_top = section_content_top.saturating_add(surface.top_offset);
        let row_within_surface = point
            .saturating_sub(surface_top)
            .min(surface.height.saturating_sub(1));
        let surface_ordinal = section.surfaces[..surface_index]
            .iter()
            .filter(|candidate| candidate.kind == surface.kind)
            .count();
        let logical_position = surface
            .selection_rows
            .as_ref()
            .and_then(|rows| logical_position_for_cell(rows, row_within_surface, column));
        let (logical_line, display_column, selection_backed) = logical_position
            .map_or((row_within_surface, 0, false), |(line, column)| {
                (line, column, true)
            });
        Some(TranscriptContentAnchor {
            activity_first_seq: section.activity_first_seq,
            surface_kind: surface.kind,
            surface_ordinal,
            logical_line,
            display_column,
            row_within_surface,
            column_within_surface: column,
            selection_backed,
        })
    }

    pub(crate) fn resolve_content_anchor(&self, anchor: TranscriptContentAnchor) -> Option<usize> {
        self.resolve_content_position(anchor).map(|cell| cell.row)
    }

    pub(crate) fn resolve_selection_anchor(
        &self,
        anchor: TranscriptContentAnchor,
    ) -> Option<TranscriptSelectionCell> {
        self.resolve_content_position(anchor)
    }

    fn resolve_content_position(
        &self,
        anchor: TranscriptContentAnchor,
    ) -> Option<TranscriptSelectionCell> {
        let section = self
            .sections
            .iter()
            .find(|section| section.activity_first_seq == anchor.activity_first_seq)?;
        let surface = section
            .surfaces
            .iter()
            .filter(|surface| surface.kind == anchor.surface_kind)
            .nth(anchor.surface_ordinal)?;
        let (row_within_surface, column) = if anchor.selection_backed {
            surface.selection_rows.as_ref().and_then(|rows| {
                cell_for_logical_position(rows, anchor.logical_line, anchor.display_column)
            })?
        } else {
            (
                anchor
                    .row_within_surface
                    .min(surface.height.saturating_sub(1)),
                anchor.column_within_surface,
            )
        };
        Some(TranscriptSelectionCell {
            row: section
                .top_row
                .saturating_add(section.leading_gap_height)
                .saturating_add(surface.top_offset)
                .saturating_add(row_within_surface),
            column,
        })
    }
}

fn logical_position_for_cell(
    rows: &[TranscriptSelectionRow],
    target_row: usize,
    target_column: usize,
) -> Option<(usize, usize)> {
    let mut logical_line = 0_usize;
    let mut display_column = 0_usize;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 && !row.continues_previous {
            logical_line += 1;
            display_column = 0;
        }
        if index == target_row {
            return Some((
                logical_line,
                display_column.saturating_add(selection_row_column(row, target_column)),
            ));
        }
        display_column = display_column.saturating_add(selection_row_width(row));
    }
    None
}

fn cell_for_logical_position(
    rows: &[TranscriptSelectionRow],
    target_line: usize,
    target_column: usize,
) -> Option<(usize, usize)> {
    let mut logical_line = 0_usize;
    let mut display_column = 0_usize;
    let mut last_target_cell = None;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 && !row.continues_previous {
            logical_line += 1;
            display_column = 0;
        }
        if logical_line != target_line {
            continue;
        }
        let compact = compact_selection_row(row, 0);
        let row_column = compact.start_cell.saturating_add(
            target_column
                .saturating_sub(display_column)
                .min(selection_row_width(row)),
        );
        last_target_cell = Some((index, row_column));
        let width = selection_row_width(row);
        if target_column < display_column.saturating_add(width.max(1)) {
            return Some((index, row_column));
        }
        display_column = display_column.saturating_add(width);
    }
    last_target_cell
}

fn selection_row_width(row: &TranscriptSelectionRow) -> usize {
    let compact = compact_selection_row(row, 0);
    compact
        .end_cell
        .checked_sub(compact.start_cell)
        .map_or(0, |width| width.saturating_add(1))
}

fn selection_row_column(row: &TranscriptSelectionRow, column: usize) -> usize {
    let compact = compact_selection_row(row, 0);
    column
        .saturating_sub(compact.start_cell)
        .min(selection_row_width(row))
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
    pub(super) tool_rail_motion: Option<ToolRailMotion>,
}

pub(super) fn measure_transcript_layout<Section>(
    sections: &[Section],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    mut activity_first_seq: impl FnMut(&Section) -> u64,
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
                tool_rail_motion: surface.tool_rail_motion,
            });
            previous_surface_kind = Some(surface.kind);
        }
        let leading_gap_height =
            usize::from(!measured_sections.is_empty()) * TRANSCRIPT_SECTION_GAP_HEIGHT;
        let measured_section = MeasuredTranscriptSection {
            activity_first_seq: activity_first_seq(section),
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
    // The measured surface gap already supplies the single blank row under a sticky user.
    let viewport_rows = transcript_viewport_rows(layout, viewport_height, scroll_top);
    let sticky_user = sticky_user_surface(layout, scroll_top, viewport_height);
    let sticky_height = Some(viewport_rows.sticky_height).filter(|height| *height > 0);
    let sticky_block_height = sticky_height;

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
            if surface.selected_rail
                && surface.tool_rail_motion.is_none()
                && local_scroll == 0
                && surface_rect.height > 0
            {
                paint_selected_rail_glyph(
                    frame,
                    surface_rect,
                    surface.rail_color,
                    surface.rail_glyph,
                );
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

fn paint_selected_rail_glyph(
    frame: &mut Frame,
    surface_rect: Rect,
    rail_color: Color,
    rail_glyph: &'static str,
) {
    let rail_x = surface_rect.x;
    let rail_rect = Rect::new(rail_x, surface_rect.y, 1, surface_rect.height);
    let rail_lines = (0..surface_rect.height)
        .map(|_| Line::from(Span::styled(rail_glyph, Style::default().fg(rail_color))))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(rail_lines), rail_rect);
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
    use crate::ui::ui_transcript_interaction::{
        TranscriptInteractionRow, TranscriptMouseTarget, TranscriptViewportHitMap,
    };
    use ratatui::style::Color;
    use ratatui::text::Span;

    fn run_write_layout(total_content_rows: usize) -> MeasuredTranscriptLayout {
        let body_height = total_content_rows.saturating_sub(1).max(1);
        MeasuredTranscriptLayout {
            sections: vec![MeasuredTranscriptSection {
                activity_first_seq: 0,
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
                        tool_rail_motion: None,
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
                        tool_rail_motion: None,
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
                activity_first_seq: 0,
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
                        tool_rail_motion: None,
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
                        tool_rail_motion: None,
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
    fn sticky_viewport_rows_map_body_below_the_pinned_user() {
        // arrange
        let layout = scroll_turn_layout(4, 40);

        // act
        let rows = transcript_viewport_rows(&layout, 20, 10);

        // assert
        assert_eq!(rows.absolute_row(0), Some(0));
        assert_eq!(rows.absolute_row(3), Some(3));
        assert_eq!(rows.absolute_row(4), Some(10));
        assert_eq!(rows.absolute_row(19), Some(25));
    }

    #[test]
    fn sticky_viewport_rows_keep_tool_hit_testing_aligned() {
        // arrange
        let mut layout = scroll_turn_layout(4, 40);
        let target = TranscriptMouseTarget::Tool {
            tool_call_id: "tool-sticky-hit".to_string(),
        };
        let mut interaction_rows = vec![None; 40];
        interaction_rows[0] = Some(TranscriptInteractionRow {
            target: target.clone(),
            hit_start: 0,
            hit_width: 20,
        });
        layout.sections[0].surfaces[1].interaction_rows = Some(interaction_rows);
        let viewport = Rect::new(0, 0, 120, 10);
        let rows = transcript_viewport_rows(&layout, usize::from(viewport.height), 4);

        // act
        let hit = TranscriptViewportHitMap::new(&layout, viewport, rows).hit(1, 4);

        // assert
        assert_eq!(hit, Some(target));
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

    #[test]
    fn rendered_anchor_tracks_same_surface_when_preceding_content_expands() {
        // Given: a detached viewport inside an assistant surface.
        let before = scroll_turn_layout(4, 40);
        let anchor = before
            .capture_content_anchor(10)
            .expect("assistant surface anchor");
        let mut after = before.clone();
        after.sections[0].surfaces[0].height += 5;
        after.sections[0].surfaces[1].top_offset += 5;
        after.sections[0].content_height += 5;
        after.total_height += 5;

        // When: disclosure above the viewport changes rendered height.
        let restored = after
            .resolve_content_anchor(anchor)
            .expect("resolved assistant surface anchor");

        // Then: the same within-surface row stays at the viewport top.
        assert_eq!(restored, 15);
    }

    #[test]
    fn rendered_anchor_tracks_logical_position_through_long_line_rewrap() {
        // Given: a detached viewport on the third wrapped row of one logical line.
        let mut before = scroll_turn_layout(1, 3);
        before.sections[0].surfaces[1].selection_rows = Some(vec![
            selection_row(4, false),
            selection_row(4, true),
            selection_row(4, true),
        ]);
        let anchor = before
            .capture_content_anchor(3)
            .expect("wrapped line anchor");
        let mut after = scroll_turn_layout(1, 2);
        after.sections[0].surfaces[1].selection_rows =
            Some(vec![selection_row(6, false), selection_row(6, true)]);

        // When: width reflow changes the number of visual rows.
        let restored = after
            .resolve_content_anchor(anchor)
            .expect("rewrapped line anchor");

        // Then: the same logical display offset resolves into the new wrapping.
        assert_eq!(restored, 2);
    }

    #[test]
    fn selection_endpoint_tracks_logical_cell_through_long_line_rewrap() {
        // Given: a selection endpoint two cells into the third wrapped row.
        let mut before = scroll_turn_layout(1, 3);
        before.sections[0].surfaces[1].selection_rows = Some(vec![
            selection_row(4, false),
            selection_row(4, true),
            selection_row(4, true),
        ]);
        let anchor = before
            .capture_selection_anchor(TranscriptSelectionCell { row: 3, column: 2 })
            .expect("selection endpoint anchor");
        let mut after = scroll_turn_layout(1, 2);
        after.sections[0].surfaces[1].selection_rows =
            Some(vec![selection_row(6, false), selection_row(6, true)]);

        // When: width reflow packs that logical cell into two rows.
        let restored = after
            .resolve_selection_anchor(anchor)
            .expect("resolved selection endpoint");

        // Then: the endpoint remains on display column ten of the source line.
        assert_eq!(restored, TranscriptSelectionCell { row: 2, column: 4 });
    }

    fn selection_row(width: usize, continues_previous: bool) -> TranscriptSelectionRow {
        TranscriptSelectionRow {
            cells: vec!["x".to_string(); width],
            continues_previous,
            copy_offset: 0,
        }
    }
}
