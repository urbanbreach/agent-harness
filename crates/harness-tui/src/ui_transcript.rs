use std::cell::RefCell;
use std::path::Path;

use super::*;

use crate::text::{collapse_inline_whitespace, has_trimmed_content};
use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;
use crate::time_format::short_time_or_trimmed;

use super::ui_diff::{render_structured_diff_lines_with_hunk_offsets, StructuredDiffRenderOptions};
use super::ui_markdown::{append_rich_text_block, parse_inline_markdown_spans};
use super::ui_tool_delegation::{
    agent_spawn_description, agent_spawn_subtitle, agent_spawn_title,
    hidden_delegated_child_request_ids, task_tool_child_session_id,
};
use super::ui_tool_diffs::{
    apply_patch_tool_title, collect_apply_patch_file_render_entries,
    tool_call_apply_patch_file_rows, tool_call_diff_artifacts, tool_call_has_diff_preview,
    tool_call_inline_diff_block, ApplyPatchFileRenderEntry,
};
use super::ui_tool_error::{
    push_failed_tool_error_block, tool_call_denied, tool_error_subtitle, tool_error_text,
};
use super::ui_tool_metadata::tool_summary_string;
use super::ui_tool_output::{collapsible_bash_panel_preview, collapsible_output_preview};
use super::ui_tool_paths::{
    context_group_tool_id, join_tool_subtitles, read_tool_input_suffix, search_result_count_suffix,
    tool_call_path_metadata, tool_in_path_description, tool_match_count_description,
    tool_path_display, TranscriptPathMetadata,
};
use super::ui_tool_question_todo::{
    ordered_todo_items, question_tool_title, resolved_question_answer_items,
    todo_items_from_tool_call, TranscriptTodoItem,
};
use super::ui_tool_style::{
    block_tool_color, block_tool_rail_color, generic_tool_visual_style, inline_tool_color,
    task_inline_tool_color, tool_call_header_style, TranscriptToolCallVisualStyle,
};
use super::ui_tool_titles::{
    background_output_tool_subtitle, background_output_tool_title, batch_tool_title,
    edit_tool_title, format_duration_ms, generic_tool_title, is_mcp_tool_id, mcp_tool_title,
    write_tool_title,
};
use super::ui_tool_titles_harness::{
    ast_grep_tool_title, background_cancel_tool_title, invalid_tool_title, lsp_tool_title,
    plan_enter_tool_title, plan_exit_tool_title, session_tool_title, skill_tool_title,
};
use super::ui_tool_visibility::{
    tool_call_should_remain_visible_without_tool_details, tool_disclosure_state,
    tool_header_disclosure_glyph, tool_hidden_from_transcript, TranscriptToolCallDisclosureState,
};
use super::ui_transcript_bash::{
    append_harness_bash_panel, shell_tool_command, shell_tool_output, shell_tool_title_description,
    HarnessBashPanel, HARNESS_BASH_OUTPUT_LINE_CLAMP,
};
use super::ui_transcript_events::{
    activity_has_thinking_text, provider_event_matches_activity, turn_event_matches_activity,
};
use super::ui_transcript_interaction::{
    append_nested_surface_row_with_target, append_noninteractive_rows,
    append_surface_row_with_bounded_target, append_surface_row_with_target,
    bounded_interaction_row, full_width_interaction_row, rect_contains, subagent_session_target,
    tool_header_target, transcript_mouse_target_at, transcript_surface_focused,
    transcript_target_is_hovered, NestedSurfaceChrome, TranscriptInteractionRow,
    TranscriptMouseTarget,
};
use super::ui_transcript_layout::{
    measure_transcript_layout, render_transcript_layout_surfaces,
    transcript_diff_hunk_rows_for_layout, transcript_layout_lines, MeasuredTranscriptLayout,
    MeasuredTranscriptSurface,
};
use super::ui_transcript_scrollbar::{
    current_transcript_scroll_top, render_transcript_scrollbar, transcript_scroll_offset,
    transcript_scrollbar_geometry, transcript_scrollbar_needed, transcript_viewport_layout,
    TranscriptScrollbarHit,
};
use super::ui_transcript_selection::{
    blank_selection_row, compact_selection_row, lifecycle_selection_snapshot,
    render_transcript_selection, selection_row_line_text,
    selection_rows_for_markdownish_text_block, selection_rows_for_rendered_line,
    transcript_selection_line_rows, with_cached_transcript_selection_snapshot, SelectionRow,
    TranscriptSelection, TranscriptSelectionCacheKey, TranscriptSelectionCell,
    TranscriptSelectionRow, TranscriptSelectionSnapshot,
};
use super::ui_transcript_style::{
    activity_status_supports_footer_only, assistant_footer_label, assistant_primary_label_color,
    assistant_primary_rail_color, selected_foreground_for_badge, transcript_emphasized_surface,
    transcript_nested_rail_color, transcript_streaming_spinner_frame,
};
use super::ui_transcript_surface::{
    append_nested_surface_row, append_prebuilt_nested_surface_lines, append_prebuilt_surface_lines,
    append_prefixed_wrapped_spans_line, append_surface_row, append_user_surface_text_block,
    nested_surface_prefix_width, surface_prefix_width, transcript_surface_content_width,
    transcript_surface_render_width, user_surface_line, TRANSCRIPT_RAIL_GLYPH,
};
#[path = "ui_transcript_types.rs"]
mod ui_transcript_types;

#[path = "ui_transcript_render.rs"]
mod ui_transcript_render;

#[path = "ui_transcript_tool_render.rs"]
mod ui_transcript_tool_render;

#[path = "ui_transcript_tool_sections.rs"]
mod ui_transcript_tool_sections;

#[path = "ui_transcript_sections.rs"]
mod ui_transcript_sections;

use ui_transcript_render::build_transcript_render_surfaces;
use ui_transcript_sections::build_transcript_sections;
#[cfg(test)]
use ui_transcript_tool_render::append_tool_call_section_lines;
#[cfg(test)]
use ui_transcript_tool_sections::{
    build_agent_spawn_tool_row, build_tool_call_section, build_transcript_tool_call_section,
};
use ui_transcript_types::*;
pub(super) use ui_transcript_types::{
    TranscriptRenderSurface, TranscriptRenderSurfaceKind, TranscriptToolCallDetailBlock,
    TranscriptToolCallDetailTone,
};

#[cfg(test)]
use super::ui_transcript_surface::visible_surface_lines;

#[cfg(test)]
use super::ui_transcript_layout::MeasuredTranscriptSection;

#[cfg(test)]
use super::ui_transcript_surface::transcript_surface_leading_gap;

#[cfg(test)]
use super::ui_transcript_selection::{
    reset_transcript_selection_cache_metrics_for_test,
    transcript_selection_cache_build_count_for_test, TranscriptSelectionDebugSnapshot,
};

#[cfg(test)]
use super::ui_tool_delegation::subagent_profile_label;
#[cfg(test)]
use super::ui_transcript_test_helpers::{
    transcript_section_model_test_activity, transcript_section_model_test_tool_call,
    transcript_test_line_texts,
};

thread_local! {
    static TRANSCRIPT_LAYOUT_CACHE: RefCell<Vec<TranscriptLayoutCacheEntry>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let context = transcript_pane_context(app, area, theme);

    if !app.replay_mode {
        let inner_area = context.inner_area;

        if app.startup_shell_visible() {
            render_startup_lifecycle_surface(frame, app, inner_area, theme);
            let selection_snapshot = app.transcript_selection().and_then(|_| {
                app.last_frame_area().and_then(|frame_area| {
                    with_transcript_selection_snapshot(app, frame_area, Clone::clone)
                })
            });
            render_transcript_selection(
                frame,
                app.transcript_selection(),
                selection_snapshot.as_ref(),
                selection_snapshot
                    .as_ref()
                    .map_or(inner_area, |snapshot| snapshot.viewport),
                theme,
            );
            return;
        }

        if live_empty_state_visible(app) {
            render_live_empty_state(frame, app, inner_area, theme);
            let selection_snapshot = app.transcript_selection().and_then(|_| {
                app.last_frame_area().and_then(|frame_area| {
                    with_transcript_selection_snapshot(app, frame_area, Clone::clone)
                })
            });
            render_transcript_selection(
                frame,
                app.transcript_selection(),
                selection_snapshot.as_ref(),
                selection_snapshot
                    .as_ref()
                    .map_or(inner_area, |snapshot| snapshot.viewport),
                theme,
            );
            return;
        }

        render_measured_transcript_pane(frame, app, inner_area, theme, context.base_surface);
        return;
    }

    if app.active_tab == Tab::Run {
        render_measured_transcript_pane(
            frame,
            app,
            context.inner_area,
            theme,
            context.base_surface,
        );
        return;
    }

    frame.render_widget(context.block.expect("replay transcript block"), area);

    if live_empty_state_visible(app) {
        render_live_empty_state(frame, app, context.inner_area, theme);
        return;
    }

    render_measured_transcript_pane(frame, app, context.inner_area, theme, context.base_surface);
}

#[derive(Debug, Clone)]
struct TranscriptPaneContext<'a> {
    inner_area: Rect,
    base_surface: Color,
    block: Option<ratatui::widgets::Block<'a>>,
}

fn transcript_pane_context<'a>(
    app: &AppState,
    area: Rect,
    theme: &'a Theme,
) -> TranscriptPaneContext<'a> {
    if !app.replay_mode {
        let vertical_gutter = if app.startup_shell_visible() || live_empty_state_visible(app) {
            0
        } else {
            theme.live_shell.rhythm.transcript_gutter_y
        };
        return TranscriptPaneContext {
            inner_area: inset_rect(
                area,
                theme.live_shell.rhythm.transcript_gutter_x,
                vertical_gutter,
            ),
            base_surface: theme.surface.shell,
            block: None,
        };
    }

    if app.active_tab == Tab::Run {
        let base_surface = if app.current_subagent_session_present() {
            theme.surface.shell
        } else {
            theme.surface.panel
        };
        return TranscriptPaneContext {
            inner_area: inset_rect(
                area,
                theme.live_shell.rhythm.transcript_gutter_x,
                theme.live_shell.rhythm.transcript_gutter_y,
            ),
            base_surface,
            block: None,
        };
    }

    let is_focused = transcript_surface_focused(app);
    let title = format!(
        "Transcript{}{}",
        if is_focused { " (focused)" } else { "" },
        if app.transcript_view.follow_mode {
            " (following)"
        } else {
            ""
        }
    );
    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);
    TranscriptPaneContext {
        inner_area: inset_rect(
            block.inner(area),
            theme.live_shell.rhythm.transcript_gutter_x,
            theme.live_shell.rhythm.transcript_gutter_y,
        ),
        base_surface: surface,
        block: Some(block),
    }
}

fn render_measured_transcript_pane(
    frame: &mut Frame,
    app: &AppState,
    inner_area: Rect,
    theme: &Theme,
    empty_surface: Color,
) {
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        theme,
        inner_area.width,
        empty_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, inner_area),
    );
    let viewport = transcript_viewport_layout(inner_area, show_scrollbar);
    let render_width = if show_scrollbar {
        viewport.content.width
    } else {
        inner_area.width
    };
    let selection_snapshot = app.transcript_selection().and_then(|_| {
        app.last_frame_area().and_then(|frame_area| {
            with_transcript_selection_snapshot(app, frame_area, Clone::clone)
        })
    });

    with_measured_transcript_layout_for_width_on_surface(
        app,
        theme,
        render_width,
        empty_surface,
        |layout| {
            app.transcript_view.last_transcript_max_scroll.set(
                layout
                    .total_height
                    .saturating_sub(usize::from(viewport.content.height)),
            );
            if layout.sections.is_empty() {
                if app.active_permission_view().is_some() {
                    render_transcript_scrollbar(
                        frame,
                        viewport,
                        0,
                        0,
                        theme,
                        empty_surface,
                        app.transcript_scrollbar_dragging(),
                    );
                    return;
                }

                frame.render_widget(
                    Paragraph::new(Text::from(vec![Line::from(Span::styled(
                        "Waiting for first turn…",
                        Style::default().fg(theme.text.secondary),
                    ))]))
                    .style(panel_style(empty_surface, theme.text.primary))
                    .wrap(Wrap { trim: false }),
                    viewport.content,
                );
                render_transcript_scrollbar(
                    frame,
                    viewport,
                    0,
                    0,
                    theme,
                    empty_surface,
                    app.transcript_scrollbar_dragging(),
                );
                return;
            }

            let transcript_scroll = transcript_scroll_offset(
                app.transcript_view.follow_mode,
                app.transcript_view.transcript_scroll,
                layout.total_height,
                viewport.content.height,
            );
            render_transcript_layout_surfaces(
                frame,
                layout,
                viewport.content,
                transcript_scroll,
                theme,
            );
            render_transcript_selection(
                frame,
                app.transcript_selection(),
                selection_snapshot.as_ref(),
                viewport.content,
                theme,
            );
            render_transcript_scrollbar(
                frame,
                viewport,
                transcript_scroll,
                layout
                    .total_height
                    .saturating_sub(usize::from(viewport.content.height)),
                theme,
                empty_surface,
                app.transcript_scrollbar_dragging(),
            );
        },
    );
}

fn build_transcript_selection_snapshot(
    app: &AppState,
    area: Rect,
) -> Option<TranscriptSelectionSnapshot> {
    let transcript_area = FrameLayoutPlan::for_app(app, area).transcript?;
    let context = transcript_pane_context(app, transcript_area, app.theme());
    if app.startup_shell_visible() {
        return lifecycle_selection_snapshot(
            super::ui_lifecycle::startup_lifecycle_selection_surface(
                app,
                context.inner_area,
                app.theme(),
            )?,
        );
    }
    if live_empty_state_visible(app) {
        return lifecycle_selection_snapshot(
            super::ui_lifecycle::live_empty_state_selection_surface(
                app,
                context.inner_area,
                app.theme(),
            )?,
        );
    }

    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        context.inner_area.width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let viewport = transcript_viewport_layout(context.inner_area, show_scrollbar);
    let render_width = usize::from(if show_scrollbar {
        viewport.content.width
    } else {
        context.inner_area.width
    });

    Some(with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        u16::try_from(render_width).unwrap_or(u16::MAX),
        context.base_surface,
        |layout| {
            let (rows, line_texts, continues_previous) =
                transcript_selection_rows(layout, render_width);
            TranscriptSelectionSnapshot {
                viewport: viewport.content,
                scroll_top: transcript_scroll_offset(
                    app.transcript_view.follow_mode,
                    app.transcript_view.transcript_scroll,
                    layout.total_height,
                    viewport.content.height,
                ),
                rows,
                line_texts,
                continues_previous,
                row_width: render_width,
            }
        },
    ))
}

fn with_transcript_selection_snapshot<R>(
    app: &AppState,
    area: Rect,
    render: impl FnOnce(&TranscriptSelectionSnapshot) -> R,
) -> Option<R> {
    let theme = *app.theme();
    let transcript_area = FrameLayoutPlan::for_app(app, area).transcript?;
    let context = transcript_pane_context(app, transcript_area, &theme);
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        context.inner_area.width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let render_width = transcript_viewport_layout(context.inner_area, show_scrollbar)
        .content
        .width;

    with_cached_transcript_selection_snapshot(
        TranscriptSelectionCacheKey {
            render_width,
            app_instance_id: app.transcript_cache_instance_id(),
            render_key: app.transcript_selection_cache_key(),
            theme,
            area: transcript_area,
            follow_mode: app.transcript_view.follow_mode,
            transcript_scroll: app.transcript_view.transcript_scroll,
        },
        || build_transcript_selection_snapshot(app, area),
        render,
    )
}

fn transcript_selection_rows(
    layout: &MeasuredTranscriptLayout,
    width: usize,
) -> (Vec<SelectionRow>, Vec<String>, Vec<bool>) {
    if layout.total_height == 0 || width == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let mut rows: Vec<SelectionRow> = (0..layout.total_height)
        .map(|line_index| SelectionRow {
            line_index,
            start_cell: 1,
            end_cell: 0,
        })
        .collect();
    let mut line_texts = vec![" ".repeat(width); layout.total_height];
    let mut continues_previous = vec![false; layout.total_height];

    for section in &layout.sections {
        let section_top = section.top_row.saturating_add(section.leading_gap_height);
        for surface in &section.surfaces {
            let surface_top = section_top.saturating_add(surface.top_offset);
            for (offset, row) in transcript_surface_selection_rows(surface)
                .into_iter()
                .enumerate()
            {
                let target = surface_top.saturating_add(offset);
                if target >= rows.len() {
                    break;
                }
                rows[target] = compact_selection_row(&row, target);
                line_texts[target] = selection_row_line_text(&row);
                continues_previous[target] = row.continues_previous;
            }
        }
    }

    (rows, line_texts, continues_previous)
}

fn transcript_surface_selection_rows(
    surface: &MeasuredTranscriptSurface,
) -> Vec<TranscriptSelectionRow> {
    if let Some(rows) = surface.selection_rows.clone() {
        return rows;
    }

    let surface_width = usize::from(surface.width.max(1));
    let content_width = usize::from(transcript_surface_content_width(
        surface.width,
        surface.show_outer_rail,
    ))
    .max(1);
    let mut rows = Vec::new();

    for line in &surface.lines {
        let content_rows = transcript_selection_line_rows(line, content_width);
        if surface.show_outer_rail {
            rows.extend(content_rows.into_iter().enumerate().map(|(idx, mut row)| {
                let mut full = Vec::with_capacity(surface_width);
                full.push(surface.rail_glyph.to_string());
                full.append(&mut row);
                full.truncate(surface_width);
                if full.len() < surface_width {
                    full.resize(surface_width, " ".to_string());
                }
                TranscriptSelectionRow {
                    cells: full,
                    continues_previous: idx > 0,
                    copy_offset: 1,
                }
            }));
        } else {
            rows.extend(content_rows.into_iter().enumerate().map(|(idx, mut row)| {
                row.truncate(surface_width);
                if row.len() < surface_width {
                    row.resize(surface_width, " ".to_string());
                }
                TranscriptSelectionRow {
                    cells: row,
                    continues_previous: idx > 0,
                    copy_offset: 0,
                }
            }));
        }
    }

    if rows.is_empty() {
        rows.push(TranscriptSelectionRow {
            cells: vec![" ".to_string(); surface_width],
            continues_previous: false,
            copy_offset: 0,
        });
    }

    rows
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_transcript_lines(app: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    build_transcript_lines_for_width(app, theme, DIFF_SIDE_BY_SIDE_MIN_WIDTH.saturating_sub(1))
}

pub(crate) fn build_transcript_lines_for_width(
    app: &AppState,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    with_measured_transcript_layout_for_width_on_surface(
        app,
        theme,
        width,
        theme.surface.shell,
        |layout| transcript_layout_lines(layout, theme),
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn build_measured_transcript_layout_for_width(
    app: &AppState,
    theme: &Theme,
    width: u16,
) -> MeasuredTranscriptLayout {
    build_measured_transcript_layout_for_width_on_surface(app, theme, width, theme.surface.shell)
}

#[cfg_attr(not(test), allow(dead_code))]
fn build_measured_transcript_layout_for_width_on_surface(
    app: &AppState,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> MeasuredTranscriptLayout {
    with_measured_transcript_layout_for_width_on_surface(
        app,
        theme,
        width,
        base_surface,
        Clone::clone,
    )
}

fn with_measured_transcript_layout_for_width_on_surface<R>(
    app: &AppState,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    render: impl FnOnce(&MeasuredTranscriptLayout) -> R,
) -> R {
    let app_instance_id = app.transcript_cache_instance_id();
    let render_key = app.transcript_render_cache_key();

    TRANSCRIPT_LAYOUT_CACHE.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some(entry) = cache.iter().find(|entry| {
                entry.app_instance_id == app_instance_id
                    && entry.render_key == render_key
                    && entry.theme == *theme
                    && entry.width == width
                    && entry.base_surface == base_surface
            }) {
                return render(&entry.layout);
            }
        }

        let sections = build_transcript_sections(app);
        let layout = measure_transcript_layout(
            &sections,
            theme,
            width,
            base_surface,
            build_transcript_render_surfaces,
        );

        {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| {
                entry.app_instance_id != app_instance_id || entry.render_key == render_key
            });
            cache.push(TranscriptLayoutCacheEntry {
                app_instance_id,
                render_key,
                theme: *theme,
                width,
                base_surface,
                layout,
            });
            if cache.len() > 4 {
                let overflow = cache.len().saturating_sub(4);
                cache.drain(0..overflow);
            }
        }

        let cache = cache.borrow();
        let entry = cache
            .iter()
            .find(|entry| {
                entry.app_instance_id == app_instance_id
                    && entry.render_key == render_key
                    && entry.theme == *theme
                    && entry.width == width
                    && entry.base_surface == base_surface
            })
            .expect("cached transcript layout should be present after insertion");
        render(&entry.layout)
    })
}

pub(crate) fn transcript_scrollbar_hit(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptScrollbarHit> {
    let context = transcript_pane_context(
        app,
        FrameLayoutPlan::for_app(app, area).transcript?,
        app.theme(),
    );
    let max_scroll = app.transcript_view.last_transcript_max_scroll.get();
    if max_scroll == 0 {
        return None;
    }

    let viewport = transcript_viewport_layout(context.inner_area, true);
    let scroll_top = current_transcript_scroll_top(
        app.transcript_view.follow_mode,
        app.transcript_view.transcript_scroll,
        max_scroll,
    );
    let geometry = transcript_scrollbar_geometry(viewport, scroll_top, max_scroll)?;
    rect_contains(geometry.lane, column, row).then_some(geometry)
}

pub(crate) fn transcript_selection_cell(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptSelectionCell> {
    with_transcript_selection_snapshot(app, area, |snapshot| snapshot.hit(column, row)).flatten()
}

pub(crate) fn transcript_diff_hunk_rows(app: &AppState, area: Rect) -> Vec<usize> {
    let theme = *app.theme();
    let Some(transcript_area) = FrameLayoutPlan::for_app(app, area).transcript else {
        return Vec::new();
    };
    let context = transcript_pane_context(app, transcript_area, &theme);
    if app.startup_shell_visible() || live_empty_state_visible(app) {
        return Vec::new();
    }

    let full_width = context.inner_area.width;
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        full_width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let render_width = transcript_viewport_layout(context.inner_area, show_scrollbar)
        .content
        .width;

    with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        render_width,
        context.base_surface,
        transcript_diff_hunk_rows_for_layout,
    )
}

pub(crate) fn transcript_mouse_target(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptMouseTarget> {
    let theme = *app.theme();
    let transcript_area = FrameLayoutPlan::for_app(app, area).transcript?;
    let context = transcript_pane_context(app, transcript_area, &theme);
    if app.startup_shell_visible() || live_empty_state_visible(app) {
        return None;
    }

    let full_width = context.inner_area.width;
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        full_width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let viewport_layout = transcript_viewport_layout(context.inner_area, show_scrollbar);
    let render_width = viewport_layout.content.width;
    let viewport = viewport_layout.content;

    with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        render_width,
        context.base_surface,
        |layout| {
            transcript_mouse_target_at(
                layout,
                viewport,
                transcript_scroll_offset(
                    app.transcript_view.follow_mode,
                    app.transcript_view.transcript_scroll,
                    layout.total_height,
                    viewport.height,
                ),
                column,
                row,
            )
        },
    )
}

pub(crate) fn transcript_selection_text(
    app: &AppState,
    area: Rect,
    selection: TranscriptSelection,
) -> Option<String> {
    with_transcript_selection_snapshot(app, area, |snapshot| snapshot.selection_text(selection))
        .flatten()
}

#[cfg(test)]
pub(crate) fn transcript_selection_debug_snapshot(
    app: &AppState,
    area: Rect,
) -> Option<TranscriptSelectionDebugSnapshot> {
    with_transcript_selection_snapshot(app, area, |snapshot| TranscriptSelectionDebugSnapshot {
        viewport: snapshot.viewport,
        rows: snapshot.visible_rows(),
    })
}

#[cfg(test)]
pub(crate) fn transcript_selection_row_count(app: &AppState, area: Rect) -> Option<usize> {
    with_transcript_selection_snapshot(app, area, |snapshot| snapshot.rows.len())
}

#[cfg(test)]
#[path = "ui_transcript_exact_tests.rs"]
mod ui_transcript_exact_tests;
#[cfg(test)]
pub(crate) use ui_transcript_exact_tests::*;

#[cfg(test)]
#[path = "ui_transcript_tests.rs"]
mod tests;
