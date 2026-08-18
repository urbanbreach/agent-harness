use std::rc::Rc;

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::theme::Theme;

use super::ui_transcript::{
    IntoResolvedTranscriptVisualEntryDraft, ResolvedTranscriptVisualEntryDraft, ToolRailMotion,
    TranscriptBlockPlacement, TranscriptRenderSurfaceKind, TranscriptVisualEntryAccent,
    TranscriptVisualEntryDisplayMode, TranscriptVisualEntryDraft, TranscriptVisualEntryGroup,
    TranscriptVisualEntryHitRegion, TranscriptVisualEntryId, TranscriptVisualEntryLifecycle,
    TranscriptVisualEntryMetadata,
};
use super::ui_transcript_interaction::TranscriptInteractionRow;
use super::ui_transcript_selection::{
    compact_selection_row, TranscriptSelectionCell, TranscriptSelectionRow,
};
use super::ui_transcript_surface::{
    render_transcript_surface, render_transcript_surface_lines, transcript_surface_content_width,
    transcript_surface_render_width,
};

const TRANSCRIPT_SECTION_GAP_HEIGHT: usize = 2;

#[derive(Debug, Clone)]
pub(super) struct MeasuredTranscriptSection {
    pub(super) activity_first_seq: u64,
    pub(super) top_row: usize,
    pub(super) leading_gap_height: usize,
    pub(super) content_height: usize,
    pub(super) surfaces: Vec<TranscriptVisualEntry>,
    pub(super) lines: Vec<Line<'static>>,
}

impl MeasuredTranscriptSection {
    pub(super) fn total_height(&self) -> usize {
        self.leading_gap_height + self.content_height
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct MeasuredTranscriptLayout {
    pub(super) sections: Vec<Rc<MeasuredTranscriptSection>>,
    pub(super) total_height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptContentAnchor {
    activity_first_seq: u64,
    entry_id: TranscriptVisualEntryId,
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

    pub(super) const fn body_scroll_top(self) -> usize {
        self.body_scroll_top
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptVisualEntryViewportPlacement {
    pub(super) rect: Rect,
    pub(super) local_scroll: usize,
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
            entry_id: surface.metadata.id,
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
            .find(|surface| surface.metadata.id == anchor.entry_id)?;
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
pub(super) struct TranscriptVisualEntry {
    pub(super) metadata: TranscriptVisualEntryMetadata,
    pub(super) kind: TranscriptRenderSurfaceKind,
    pub(super) leading_gap_rows: usize,
    pub(super) placement: TranscriptBlockPlacement,
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
    pub(super) hit_region: TranscriptVisualEntryHitRegion,
}

pub(super) fn measure_transcript_layout<Section, Entry>(
    sections: &[Section],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    mut activity_first_seq: impl FnMut(&Section) -> u64,
    mut cached_section: impl FnMut(usize, &Section) -> Option<Rc<MeasuredTranscriptSection>>,
    mut render_surfaces: impl FnMut(&Section, &Theme, u16, Color) -> Vec<Entry>,
) -> MeasuredTranscriptLayout
where
    Entry: IntoResolvedTranscriptVisualEntryDraft,
{
    let mut top_row = 0;
    let mut measured_sections = Vec::with_capacity(sections.len());

    for (index, section) in sections.iter().enumerate() {
        let leading_gap_height =
            usize::from(!measured_sections.is_empty()) * TRANSCRIPT_SECTION_GAP_HEIGHT;
        if let Some(measured) = cached_section(index, section).filter(|measured| {
            measured.top_row == top_row && measured.leading_gap_height == leading_gap_height
        }) {
            top_row += measured.total_height();
            measured_sections.push(measured);
            continue;
        }
        let activity_first_seq = activity_first_seq(section);
        let surfaces = render_surfaces(section, theme, width, base_surface)
            .into_iter()
            .enumerate()
            .map(|(ordinal, entry)| entry.into_resolved(activity_first_seq, ordinal))
            .collect::<Vec<_>>();
        let lines = render_transcript_surface_lines(&surfaces);
        let mut content_height = 0usize;
        let mut measured_surfaces = Vec::with_capacity(surfaces.len());
        for surface in surfaces {
            let ResolvedTranscriptVisualEntryDraft {
                metadata,
                draft: surface,
            } = surface;
            let leading_gap_rows = surface.leading_gap_rows;
            let top_offset = content_height + leading_gap_rows;
            let render_width = transcript_surface_render_width(width, surface.kind);
            let content_width = usize::from(transcript_surface_content_width(render_width, false));
            let height = transcript_visual_rows(&surface.lines, content_width);
            content_height = top_offset + height;
            measured_surfaces.push(TranscriptVisualEntry {
                metadata,
                kind: surface.kind,
                leading_gap_rows,
                placement: surface.placement,
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
                hit_region: TranscriptVisualEntryHitRegion {
                    top_row: top_row + leading_gap_height + top_offset,
                    left_column: 0,
                    width: render_width,
                    height,
                },
            });
        }
        let measured_section = MeasuredTranscriptSection {
            activity_first_seq,
            top_row,
            leading_gap_height,
            content_height,
            surfaces: measured_surfaces,
            lines,
        };
        top_row += measured_section.total_height();
        measured_sections.push(Rc::new(measured_section));
    }

    MeasuredTranscriptLayout {
        sections: measured_sections,
        total_height: top_row,
    }
}

pub(super) fn transcript_layout_lines(
    layout: &MeasuredTranscriptLayout,
    _animation_phase: usize,
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
    animation_phase: usize,
    theme: &Theme,
) {
    let viewport_height = usize::from(area.height);
    if viewport_height == 0 || area.width == 0 {
        return;
    }

    for (section_idx, section) in layout.sections.iter().enumerate() {
        for (surface_idx, surface) in section.surfaces.iter().enumerate() {
            let Some(placement) = transcript_visual_entry_viewport_placement(
                layout,
                area,
                scroll_top,
                section_idx,
                surface_idx,
            ) else {
                continue;
            };
            render_transcript_surface(
                frame,
                surface,
                placement.rect,
                placement.local_scroll,
                animation_phase,
                theme,
            );
            if surface.selected_rail
                && surface.tool_rail_motion.is_none()
                && placement.local_scroll == 0
                && placement.rect.height > 0
            {
                paint_selected_rail_glyph(
                    frame,
                    placement.rect,
                    surface.rail_color,
                    surface.rail_glyph,
                );
            }
        }
    }
}

pub(super) fn transcript_visual_entry_viewport_placement(
    layout: &MeasuredTranscriptLayout,
    area: Rect,
    scroll_top: usize,
    section_idx: usize,
    surface_idx: usize,
) -> Option<TranscriptVisualEntryViewportPlacement> {
    let viewport_height = usize::from(area.height);
    if viewport_height == 0 || area.width == 0 {
        return None;
    }
    let section = layout.sections.get(section_idx)?;
    let surface = section.surfaces.get(surface_idx)?;
    let sticky_user = sticky_user_surface(layout, scroll_top, viewport_height);
    if sticky_user.is_some_and(|(sticky_section, sticky_surface, _)| {
        sticky_section == section_idx && sticky_surface == surface_idx
    }) {
        let height = surface.height.min(viewport_height);
        return (height > 0).then(|| TranscriptVisualEntryViewportPlacement {
            rect: Rect::new(
                area.x,
                area.y,
                surface.width.min(area.width),
                u16::try_from(height).unwrap_or(u16::MAX),
            ),
            local_scroll: surface.height.saturating_sub(height),
        });
    }

    let sticky_height = sticky_user
        .map(|(_, _, sticky_surface)| sticky_surface.height.min(viewport_height))
        .unwrap_or(0);
    let sticky_height_u16 = u16::try_from(sticky_height).unwrap_or(u16::MAX);
    let body_area = Rect::new(
        area.x,
        area.y.saturating_add(sticky_height_u16),
        area.width,
        area.height.saturating_sub(sticky_height_u16),
    );
    let body_viewport_height = usize::from(body_area.height);
    if body_viewport_height == 0 {
        return None;
    }
    let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
    let mut surface_top = section_content_top.saturating_add(surface.top_offset);
    if let Some((pin_section, pin_surface, delta)) =
        typed_footer_pin_delta(layout, viewport_height, scroll_top)
    {
        if pin_section == section_idx && pin_surface == surface_idx {
            surface_top = surface_top.saturating_add(delta);
        }
    }
    let surface_bottom = surface_top.saturating_add(surface.height);
    let viewport_bottom = scroll_top.saturating_add(body_viewport_height);
    if surface_bottom <= scroll_top || surface_top >= viewport_bottom {
        return None;
    }
    let visible_top = surface_top.max(scroll_top);
    let visible_bottom = surface_bottom.min(viewport_bottom);
    let local_scroll = visible_top.saturating_sub(surface_top);
    let y_offset = visible_top.saturating_sub(scroll_top);
    let visible_height = visible_bottom.saturating_sub(visible_top);
    let footer_outdent = typed_footer_outdent(surface);
    Some(TranscriptVisualEntryViewportPlacement {
        rect: Rect::new(
            body_area.x.saturating_sub(footer_outdent),
            body_area
                .y
                .saturating_add(u16::try_from(y_offset).unwrap_or(u16::MAX)),
            surface
                .width
                .min(body_area.width)
                .saturating_add(footer_outdent),
            u16::try_from(visible_height).unwrap_or(u16::MAX),
        ),
        local_scroll,
    })
}

fn sticky_user_surface<'a>(
    layout: &'a MeasuredTranscriptLayout,
    scroll_top: usize,
    viewport_height: usize,
) -> Option<(usize, usize, &'a TranscriptVisualEntry)> {
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
    let (surface_idx, user_surface) =
        section.surfaces.iter().enumerate().find(|(_, surface)| {
            surface.placement == TranscriptBlockPlacement::StickyPromptCandidate
        })?;
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
fn typed_footer_pin_delta(
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
    if !matches!(
        surface.placement,
        TranscriptBlockPlacement::PinnedFooter { .. }
    ) {
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

fn typed_footer_outdent(surface: &TranscriptVisualEntry) -> u16 {
    if let TranscriptBlockPlacement::PinnedFooter { outdent_cells } = surface.placement {
        return outdent_cells;
    }
    0
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

    fn test_metadata(
        kind: TranscriptRenderSurfaceKind,
        ordinal: usize,
    ) -> TranscriptVisualEntryMetadata {
        TranscriptVisualEntryMetadata {
            id: match kind {
                TranscriptRenderSurfaceKind::User => TranscriptVisualEntryId::User {
                    activity_first_seq: 0,
                },
                TranscriptRenderSurfaceKind::AssistantFooter => TranscriptVisualEntryId::Footer {
                    activity_first_seq: 0,
                },
                TranscriptRenderSurfaceKind::AssistantReasoning
                | TranscriptRenderSurfaceKind::AssistantBody
                | TranscriptRenderSurfaceKind::AssistantTool
                | TranscriptRenderSurfaceKind::AssistantCommandTool
                | TranscriptRenderSurfaceKind::AssistantError
                | TranscriptRenderSurfaceKind::Compaction => TranscriptVisualEntryId::Part {
                    activity_first_seq: 0,
                    semantic_key: u64::try_from(ordinal).unwrap_or(u64::MAX),
                },
            },
            kind,
            group: TranscriptVisualEntryGroup::Standalone,
            display_mode: TranscriptVisualEntryDisplayMode::Flow,
            lifecycle: TranscriptVisualEntryLifecycle::Settled,
            accent: TranscriptVisualEntryAccent::Hidden,
        }
    }

    fn test_hit_region(
        top_row: usize,
        width: u16,
        height: usize,
    ) -> TranscriptVisualEntryHitRegion {
        TranscriptVisualEntryHitRegion {
            top_row,
            left_column: 0,
            width,
            height,
        }
    }

    fn run_write_layout(total_content_rows: usize) -> MeasuredTranscriptLayout {
        let body_height = total_content_rows.saturating_sub(1).max(1);
        MeasuredTranscriptLayout {
            sections: vec![Rc::new(MeasuredTranscriptSection {
                activity_first_seq: 0,
                top_row: 0,
                leading_gap_height: 0,
                content_height: total_content_rows,
                surfaces: vec![
                    TranscriptVisualEntry {
                        metadata: test_metadata(TranscriptRenderSurfaceKind::AssistantTool, 0),
                        kind: TranscriptRenderSurfaceKind::AssistantTool,
                        leading_gap_rows: 0,
                        placement: TranscriptBlockPlacement::Flow,
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
                        hit_region: test_hit_region(0, 120, body_height),
                    },
                    TranscriptVisualEntry {
                        metadata: test_metadata(TranscriptRenderSurfaceKind::AssistantFooter, 1),
                        kind: TranscriptRenderSurfaceKind::AssistantFooter,
                        leading_gap_rows: 0,
                        placement: TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 },
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
                        hit_region: test_hit_region(body_height, 120, 1),
                    },
                ],
                lines: vec![
                    Line::from("Creating demo.txt"),
                    Line::from("     ◆ Run Write `demo.txt` 19s"),
                ],
            })],
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
        let pin = typed_footer_pin_delta(&layout, 20, 0);
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
        assert_eq!(typed_footer_pin_delta(&layout, 20, 0), None);
    }

    #[test]
    fn footer_anchor_keeps_footer_identity_when_entry_is_inserted_before_it() {
        let before = run_write_layout(2);
        let anchor = before
            .capture_content_anchor(1)
            .expect("footer content anchor");
        assert!(matches!(
            anchor.entry_id,
            TranscriptVisualEntryId::Footer { .. }
        ));

        let original = before.sections[0].as_ref();
        let body = original.surfaces[0].clone();
        let mut inserted = body.clone();
        inserted.metadata.id = TranscriptVisualEntryId::Part {
            activity_first_seq: 0,
            semantic_key: 99,
        };
        inserted.top_offset = 1;
        inserted.lines = vec![Line::from("new entry")];
        inserted.hit_region = test_hit_region(1, 120, 1);
        let mut footer = original.surfaces[1].clone();
        footer.top_offset = 2;
        footer.hit_region = test_hit_region(2, 120, 1);
        let after = MeasuredTranscriptLayout {
            sections: vec![Rc::new(MeasuredTranscriptSection {
                activity_first_seq: 0,
                top_row: 0,
                leading_gap_height: 0,
                content_height: 3,
                surfaces: vec![body, inserted, footer],
                lines: vec![
                    Line::from("Creating demo.txt"),
                    Line::from("new entry"),
                    Line::from("     ◆ Run Write `demo.txt` 19s"),
                ],
            })],
            total_height: 3,
        };

        assert_eq!(after.resolve_content_anchor(anchor), Some(2));
    }

    #[test]
    fn typed_pinned_placement_is_copy_independent() {
        let mut layout = run_write_layout(4);
        let footer = Rc::make_mut(&mut layout.sections[0])
            .surfaces
            .last_mut()
            .expect("footer");
        footer.lines = vec![Line::from("alternate localized footer")];
        let outdent = typed_footer_outdent(footer);

        assert!(typed_footer_pin_delta(&layout, 20, 0).is_some());
        assert_eq!(outdent, 1);
    }

    #[test]
    fn typed_gap_rows_determine_measured_top_offset_once() {
        let theme = Theme::default();
        let layout = measure_transcript_layout(
            &[()],
            &theme,
            80,
            Color::Reset,
            |_| 0,
            |_, _| None,
            |_, _, _, _| vec![test_render_surface(3, None)],
        );

        assert_eq!(layout.sections[0].surfaces[0].top_offset, 3);
        assert_eq!(layout.sections[0].surfaces[0].leading_gap_rows, 3);
    }

    #[test]
    fn typed_sticky_placement_survives_measurement() {
        let theme = Theme::default();
        let mut surface = test_render_surface(0, None);
        surface.kind = TranscriptRenderSurfaceKind::User;
        surface.placement = TranscriptBlockPlacement::StickyPromptCandidate;
        let layout = measure_transcript_layout(
            &[()],
            &theme,
            80,
            Color::Reset,
            |_| 0,
            |_, _| None,
            |_, _, _, _| vec![surface.clone()],
        );

        assert_eq!(
            layout.sections[0].surfaces[0].placement,
            TranscriptBlockPlacement::StickyPromptCandidate
        );
    }

    #[test]
    fn typed_layout_rejects_row_mismatch() {
        use crate::ui::ui_transcript::ui_transcript_block_grammar::{
            resolve_block_surface, test_spec, TranscriptBlockContent, TranscriptBlockRole,
            TranscriptGrammarError,
        };
        let spec = test_spec(
            TranscriptBlockRole::AssistantBody,
            TranscriptBlockContent::AssistantBody {
                text: String::new(),
                streaming: false,
                wall_clock: None,
                has_tools: false,
            },
        );
        let surface = test_render_surface(0, Some(vec![None, None]));

        assert!(matches!(
            resolve_block_surface(&spec, surface),
            Err(TranscriptGrammarError::RowMismatch)
        ));
    }

    #[test]
    fn typed_layout_rejects_invalid_pin() {
        use crate::ui::ui_transcript::ui_transcript_block_grammar::{
            test_spec, validate_block_spec, TranscriptBlockContent, TranscriptBlockRole,
            TranscriptGrammarError,
        };
        let mut spec = test_spec(
            TranscriptBlockRole::AssistantBody,
            TranscriptBlockContent::AssistantBody {
                text: String::new(),
                streaming: false,
                wall_clock: None,
                has_tools: false,
            },
        );
        spec.placement = TranscriptBlockPlacement::PinnedFooter { outdent_cells: 1 };

        assert_eq!(
            validate_block_spec(&spec),
            Err(TranscriptGrammarError::InvalidPlacement)
        );
    }

    fn test_render_surface(
        leading_gap_rows: usize,
        interaction_rows: Option<Vec<Option<TranscriptInteractionRow>>>,
    ) -> TranscriptVisualEntryDraft {
        TranscriptVisualEntryDraft {
            kind: TranscriptRenderSurfaceKind::AssistantBody,
            leading_gap_rows,
            placement: TranscriptBlockPlacement::Flow,
            show_outer_rail: false,
            rail_glyph: " ",
            rail_color: Color::Reset,
            surface: Color::Reset,
            lines: vec![Line::from("body")],
            interaction_rows,
            selection_rows: None,
            diff_hunk_offsets: Vec::new(),
            selected_rail: false,
            tool_rail_motion: None,
        }
    }

    fn scroll_turn_layout(user_height: usize, body_height: usize) -> MeasuredTranscriptLayout {
        let content_height = user_height + body_height;
        MeasuredTranscriptLayout {
            sections: vec![Rc::new(MeasuredTranscriptSection {
                activity_first_seq: 0,
                top_row: 0,
                leading_gap_height: 0,
                content_height,
                surfaces: vec![
                    TranscriptVisualEntry {
                        metadata: test_metadata(TranscriptRenderSurfaceKind::User, 0),
                        kind: TranscriptRenderSurfaceKind::User,
                        leading_gap_rows: 0,
                        placement: TranscriptBlockPlacement::StickyPromptCandidate,
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
                        hit_region: test_hit_region(0, 120, user_height),
                    },
                    TranscriptVisualEntry {
                        metadata: test_metadata(TranscriptRenderSurfaceKind::AssistantBody, 1),
                        kind: TranscriptRenderSurfaceKind::AssistantBody,
                        leading_gap_rows: 0,
                        placement: TranscriptBlockPlacement::Flow,
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
                        hit_region: test_hit_region(user_height, 120, body_height),
                    },
                ],
                lines: (0..user_height)
                    .map(|i| Line::from(format!("user line {i}")))
                    .chain((0..body_height).map(|i| Line::from(format!("body line {i}"))))
                    .collect(),
            })],
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
    fn sticky_prompt_selection_is_surface_kind_independent() {
        let mut layout = scroll_turn_layout(4, 40);
        let section = Rc::make_mut(&mut layout.sections[0]);
        section.surfaces[0].kind = TranscriptRenderSurfaceKind::AssistantBody;

        let (_, surface_idx, surface) = sticky_user_surface(&layout, 10, 20)
            .expect("typed sticky placement must not depend on surface kind or copy");

        assert_eq!(surface_idx, 0);
        assert_eq!(
            surface.placement,
            TranscriptBlockPlacement::StickyPromptCandidate
        );
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
        Rc::make_mut(&mut layout.sections[0]).surfaces[1].interaction_rows = Some(interaction_rows);
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
        let section = Rc::make_mut(&mut after.sections[0]);
        section.surfaces[0].height += 5;
        section.surfaces[1].top_offset += 5;
        section.content_height += 5;
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
        Rc::make_mut(&mut before.sections[0]).surfaces[1].selection_rows = Some(vec![
            selection_row(4, false),
            selection_row(4, true),
            selection_row(4, true),
        ]);
        let anchor = before
            .capture_content_anchor(3)
            .expect("wrapped line anchor");
        let mut after = scroll_turn_layout(1, 2);
        Rc::make_mut(&mut after.sections[0]).surfaces[1].selection_rows =
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
        Rc::make_mut(&mut before.sections[0]).surfaces[1].selection_rows = Some(vec![
            selection_row(4, false),
            selection_row(4, true),
            selection_row(4, true),
        ]);
        let anchor = before
            .capture_selection_anchor(TranscriptSelectionCell { row: 3, column: 2 })
            .expect("selection endpoint anchor");
        let mut after = scroll_turn_layout(1, 2);
        Rc::make_mut(&mut after.sections[0]).surfaces[1].selection_rows =
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
