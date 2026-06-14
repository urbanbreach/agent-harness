use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    Frame,
};
#[cfg(debug_assertions)]
use std::cell::Cell;

use crate::theme::Theme;

use super::ui_transcript::TranscriptRenderSurface;
use super::ui_transcript_interaction::TranscriptInteractionRow;
use super::ui_transcript_selection::TranscriptSelectionRow;
use super::ui_transcript_style::decorate_transcript_spinner_line;
use super::ui_transcript_surface::{
    render_transcript_surface, render_transcript_surface_lines, transcript_surface_content_width,
    transcript_surface_leading_gap, transcript_surface_render_width,
};

#[cfg(debug_assertions)]
thread_local! {
    static TRANSCRIPT_SECTION_MEASURE_COUNT: Cell<usize> = const { Cell::new(0) };
}

const TRANSCRIPT_SECTION_GAP_HEIGHT: usize = 1;

#[cfg_attr(not(test), allow(dead_code))]
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
    pub(super) top_offset: usize,
    pub(super) height: usize,
    pub(super) width: u16,
    pub(super) show_outer_rail: bool,
    pub(super) rail_color: Color,
    pub(super) surface: Color,
    pub(super) lines: Vec<Line<'static>>,
    pub(super) interaction_rows: Option<Vec<Option<TranscriptInteractionRow>>>,
    pub(super) selection_rows: Option<Vec<TranscriptSelectionRow>>,
    pub(super) diff_hunk_offsets: Vec<usize>,
}

pub(super) fn measure_transcript_layout<Section>(
    sections: &[Section],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    render_surfaces: impl FnMut(&Section, &Theme, u16, Color) -> Vec<TranscriptRenderSurface>,
) -> MeasuredTranscriptLayout {
    measure_transcript_layout_without_cache(sections, theme, width, base_surface, render_surfaces)
}

pub(super) fn measure_transcript_layout_without_cache<Section>(
    sections: &[Section],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    mut render_surfaces: impl FnMut(&Section, &Theme, u16, Color) -> Vec<TranscriptRenderSurface>,
) -> MeasuredTranscriptLayout {
    let mut top_row = 0;
    let mut measured_sections = Vec::with_capacity(sections.len());

    for section in sections {
        #[cfg(debug_assertions)]
        TRANSCRIPT_SECTION_MEASURE_COUNT.with(|count| {
            count.set(count.get().saturating_add(1));
        });
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
                top_offset,
                height,
                width: render_width,
                show_outer_rail: surface.show_outer_rail,
                rail_color: surface.rail_color,
                surface: surface.surface,
                lines: surface.lines,
                interaction_rows: surface.interaction_rows,
                selection_rows: surface.selection_rows,
                diff_hunk_offsets: surface.diff_hunk_offsets,
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
    animation_phase: usize,
) -> Vec<Line<'static>> {
    let mut lines = layout
        .rendered_lines()
        .into_iter()
        .map(|line| decorate_transcript_spinner_line(line, animation_phase))
        .collect::<Vec<_>>();

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
    animation_phase: usize,
) {
    let viewport_height = usize::from(area.height);
    if viewport_height == 0 || area.width == 0 {
        return;
    }

    let viewport_bottom = scroll_top.saturating_add(viewport_height);
    for section in &layout.sections {
        let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
        for surface in &section.surfaces {
            let surface_top = section_content_top.saturating_add(surface.top_offset);
            let surface_bottom = surface_top.saturating_add(surface.height);
            if surface_bottom <= scroll_top || surface_top >= viewport_bottom {
                continue;
            }

            let visible_top = surface_top.max(scroll_top);
            let visible_bottom = surface_bottom.min(viewport_bottom);
            let local_scroll = visible_top.saturating_sub(surface_top);
            let y_offset = visible_top.saturating_sub(scroll_top);
            let visible_height = visible_bottom.saturating_sub(visible_top);
            let surface_rect = Rect::new(
                area.x,
                area.y
                    .saturating_add(u16::try_from(y_offset).unwrap_or(u16::MAX)),
                surface.width.min(area.width),
                u16::try_from(visible_height).unwrap_or(u16::MAX),
            );
            render_transcript_surface(
                frame,
                surface,
                surface_rect,
                local_scroll,
                theme,
                animation_phase,
            );
        }
    }
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

#[cfg(debug_assertions)]
pub(crate) fn reset_transcript_layout_measurement_metrics_for_test() {
    TRANSCRIPT_SECTION_MEASURE_COUNT.with(|count| count.set(0));
}

#[cfg(debug_assertions)]
pub(crate) fn transcript_layout_section_build_count_for_test() -> usize {
    TRANSCRIPT_SECTION_MEASURE_COUNT.with(Cell::get)
}
