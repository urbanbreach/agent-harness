use ratatui::{
    layout::Rect,
    style::Color,
    text::{Line, Span},
};

use crate::app::{AppState, Focus, Tab};
use crate::layout::FrameLayoutPlan;

use super::ui_transcript_layout::MeasuredTranscriptLayout;
use super::ui_transcript_surface::{append_nested_surface_row, append_surface_row};
use super::WheelTarget;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TranscriptMouseTarget {
    Tool {
        tool_call_id: String,
    },
    ToolGroup {
        tool_call_ids: Vec<String>,
    },
    PatchFile {
        tool_call_id: String,
        file_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptInteractionRow {
    pub(super) target: TranscriptMouseTarget,
    pub(super) hit_start: u16,
    pub(super) hit_width: u16,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NestedSurfaceChrome<'a> {
    pub(super) indent: &'a str,
    pub(super) rail_color: Color,
    pub(super) surface: Color,
}

pub(super) fn transcript_surface_focused(app: &AppState) -> bool {
    !app.replay_mode
        && app.active_tab == Tab::Run
        && app.focus == Focus::Details
        && !app.details_drawer_open()
}

pub(super) fn tool_header_target(
    tool_call_id: &str,
    has_disclosure: bool,
) -> Option<TranscriptMouseTarget> {
    has_disclosure.then(|| TranscriptMouseTarget::Tool {
        tool_call_id: tool_call_id.to_string(),
    })
}

pub fn hovered_wheel_target(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<WheelTarget> {
    let hit_areas = FrameLayoutPlan::for_app(app, area).wheel_hit_areas;
    if hit_areas
        .inspector
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return Some(WheelTarget::Inspector);
    }
    if hit_areas
        .overlay
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return None;
    }
    if hit_areas
        .terminal_panel
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return Some(WheelTarget::Terminal);
    }
    hit_areas
        .transcript
        .filter(|area| rect_contains(*area, column, row))
        .map(|_| WheelTarget::Transcript)
}

pub(super) fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

pub(super) fn transcript_mouse_target_at(
    layout: &MeasuredTranscriptLayout,
    viewport: Rect,
    scroll_top: usize,
    column: u16,
    row: u16,
) -> Option<TranscriptMouseTarget> {
    if !rect_contains(viewport, column, row) {
        return None;
    }

    let absolute_row = scroll_top.saturating_add(usize::from(row.saturating_sub(viewport.y)));
    for section in &layout.sections {
        let section_content_top = section.top_row.saturating_add(section.leading_gap_height);
        for surface in &section.surfaces {
            let surface_top = section_content_top.saturating_add(surface.top_offset);
            let surface_bottom = surface_top.saturating_add(surface.height);
            if absolute_row < surface_top || absolute_row >= surface_bottom {
                continue;
            }

            let local_row = absolute_row.saturating_sub(surface_top);
            if let Some(interaction) = surface
                .interaction_rows
                .as_ref()
                .and_then(|targets| targets.get(local_row))
                .cloned()
                .flatten()
            {
                let local_column = column.saturating_sub(viewport.x);
                if local_column >= interaction.hit_start
                    && local_column < interaction.hit_start.saturating_add(interaction.hit_width)
                {
                    return Some(interaction.target);
                }
            }
        }
    }

    None
}

pub(super) fn full_width_interaction_row(
    target: TranscriptMouseTarget,
) -> TranscriptInteractionRow {
    TranscriptInteractionRow {
        target,
        hit_start: 0,
        hit_width: u16::MAX,
    }
}

pub(super) fn bounded_interaction_row(
    target: Option<TranscriptMouseTarget>,
    line: &Line<'_>,
) -> Option<TranscriptInteractionRow> {
    let (hit_start, hit_width) = line_hit_bounds_without_trailing_fill(line);
    target.map(|target| TranscriptInteractionRow {
        target,
        hit_start,
        hit_width,
    })
}

pub(super) fn append_surface_row_with_target(
    lines: &mut Vec<Line<'static>>,
    interaction_rows: &mut Vec<Option<TranscriptInteractionRow>>,
    target: Option<TranscriptMouseTarget>,
    indent: &str,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let start = lines.len();
    append_surface_row(lines, indent, surface, content_spans, width);
    append_full_width_interaction_rows(lines, interaction_rows, target, start);
}

pub(super) fn append_surface_row_with_bounded_target(
    lines: &mut Vec<Line<'static>>,
    interaction_rows: &mut Vec<Option<TranscriptInteractionRow>>,
    target: Option<TranscriptMouseTarget>,
    indent: &str,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let start = lines.len();
    append_surface_row(lines, indent, surface, content_spans, width);
    for line in &lines[start..] {
        interaction_rows.push(bounded_interaction_row(target.clone(), line));
    }
}

pub(super) fn append_nested_surface_row_with_target(
    lines: &mut Vec<Line<'static>>,
    interaction_rows: &mut Vec<Option<TranscriptInteractionRow>>,
    target: Option<TranscriptMouseTarget>,
    chrome: NestedSurfaceChrome<'_>,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let start = lines.len();
    append_nested_surface_row(
        lines,
        chrome.indent,
        chrome.rail_color,
        chrome.surface,
        content_spans,
        width,
    );
    append_full_width_interaction_rows(lines, interaction_rows, target, start);
}

pub(super) fn append_noninteractive_rows(
    lines: &[Line<'static>],
    interaction_rows: &mut Vec<Option<TranscriptInteractionRow>>,
    start: usize,
) {
    let added = lines.len().saturating_sub(start);
    interaction_rows.extend(std::iter::repeat_n(None, added));
}

fn append_full_width_interaction_rows(
    lines: &[Line<'static>],
    interaction_rows: &mut Vec<Option<TranscriptInteractionRow>>,
    target: Option<TranscriptMouseTarget>,
    start: usize,
) {
    let added = lines.len().saturating_sub(start);
    interaction_rows.extend(std::iter::repeat_n(
        target.map(full_width_interaction_row),
        added,
    ));
}

fn line_hit_bounds_without_trailing_fill(line: &Line<'_>) -> (u16, u16) {
    let total_width = line
        .spans
        .iter()
        .fold(0usize, |width, span| width.saturating_add(span.width()));
    let mut leading_blank_width = 0usize;
    let mut found_content = false;
    for span in &line.spans {
        for ch in span.content.chars() {
            if ch != ' ' {
                found_content = true;
                break;
            }
            leading_blank_width =
                leading_blank_width.saturating_add(Line::from(ch.to_string()).width());
        }
        if found_content {
            break;
        }
    }
    let trailing_fill_width = line
        .spans
        .last()
        .filter(|span| span.style.bg.is_some() && span.content.chars().all(|ch| ch == ' '))
        .map_or(0, Span::width);
    let hit_end = total_width.saturating_sub(trailing_fill_width);
    if hit_end <= leading_blank_width {
        return (0, 1);
    }

    (
        u16::try_from(leading_blank_width).unwrap_or(u16::MAX),
        u16::try_from(hit_end.saturating_sub(leading_blank_width)).unwrap_or(u16::MAX),
    )
}
