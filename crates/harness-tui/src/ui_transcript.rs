#[cfg(test)]
use super::ui_transcript_layout::transcript_layout_lines;
// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use crate::app::{ToolCallPresentation, ToolCallPresentationStatus};
use crate::UnwrapOrAbort;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use super::*;

use crate::text::{collapse_inline_whitespace, has_trimmed_content};
use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;
use crate::time_format::short_time_or_trimmed;

use super::ui_diff::{render_structured_diff_lines_with_hunk_offsets, StructuredDiffRenderOptions};
use super::ui_markdown::{append_rich_text_block, parse_inline_markdown_spans, raw_url_length};
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
use super::ui_tool_output::collapsible_output_preview;
use super::ui_tool_paths::{
    join_tool_subtitles, read_tool_input_suffix, search_result_count_suffix,
    tool_call_path_metadata, tool_match_count_description, tool_path_display,
    TranscriptPathMetadata,
};
use super::ui_tool_question_todo::{question_tool_title, resolved_question_answer_items};
use super::ui_tool_style::{
    generic_tool_visual_style, tool_call_header_style, TranscriptToolCallVisualStyle,
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
use super::ui_tool_visibility::{tool_hidden_from_transcript, TranscriptToolCallDisclosureState};
use super::ui_transcript_bash::{
    append_harness_bash_panel, shell_tool_command, shell_tool_output, shell_tool_structured_output,
    shell_tool_title_description, HarnessBashPanel,
};
use super::ui_transcript_events::{
    activity_has_thinking_text, provider_event_matches_activity, turn_event_matches_activity,
};
use super::ui_transcript_interaction::{
    append_noninteractive_rows, append_surface_row_with_bounded_target,
    append_surface_row_with_target, bounded_interaction_row, full_width_interaction_row,
    rect_contains, subagent_session_target, tool_header_target, transcript_mouse_target_at,
    transcript_surface_focused, transcript_target_is_hovered, TranscriptInteractionRow,
    TranscriptMouseTarget,
};
#[cfg(test)]
use super::ui_transcript_layout::measure_transcript_layout;
use super::ui_transcript_layout::{
    render_transcript_layout_surfaces, transcript_diff_hunk_rows_for_layout,
    transcript_layout_has_visible_running_tool, transcript_viewport_rows, MeasuredTranscriptLayout,
    TranscriptVisualEntry,
};
use super::ui_transcript_page_flip::{transcript_scroll_position, TranscriptScrollPosition};
use super::ui_transcript_scrollbar::transcript_more_below_hit_rect;
use super::ui_transcript_scrollbar::{
    current_transcript_scroll_top, render_transcript_more_below_affordance,
    render_transcript_scrollbar, transcript_more_below_rect, transcript_scrollbar_geometry,
    transcript_scrollbar_needed, transcript_viewport_layout, TranscriptScrollbarHit,
    TranscriptScrollbarRenderSpec,
};
use super::ui_transcript_selection::{
    blank_selection_row, lifecycle_selection_snapshot, render_transcript_selection,
    selection_rows_for_markdownish_text_block, selection_rows_for_rendered_line, SelectionRow,
    TranscriptSelection, TranscriptSelectionCell, TranscriptSelectionSnapshot,
};
use super::ui_transcript_style::{
    assistant_footer_label, assistant_primary_label_color, assistant_primary_rail_color,
    blend_color, glyph_routed_streaming_spinner_frame, selected_foreground_for_badge,
    thinking_header_color, transcript_emphasized_surface, transcript_running_tool_marker_color,
};
use super::ui_transcript_surface::{
    append_prebuilt_surface_lines, append_prefixed_wrapped_spans_line, append_surface_row,
    append_user_surface_text_block, surface_prefix_width, surface_span,
    transcript_surface_content_width, transcript_surface_render_width, user_surface_line,
    wrap_surface_spans, TRANSCRIPT_RAIL_GLYPH,
};
#[path = "ui_transcript_frame.rs"]
mod ui_transcript_frame;
pub(super) use ui_transcript_frame::prepare_width;
pub(crate) use ui_transcript_frame::PreparedTranscript;

#[path = "ui_transcript_types.rs"]
mod ui_transcript_types;

#[path = "ui_transcript_groups.rs"]
mod ui_transcript_groups;

#[path = "ui_transcript_entry.rs"]
pub(in crate::ui) mod ui_transcript_entry;

#[path = "ui_transcript_render.rs"]
mod ui_transcript_render;

#[path = "ui_reasoning_markdown/body.rs"]
mod ui_reasoning_markdown_body;

#[path = "ui_transcript_tool_render.rs"]
mod ui_transcript_tool_render;

#[path = "ui_transcript_tool_hooks.rs"]
mod ui_transcript_tool_hooks;

#[path = "ui_transcript_tool_sections.rs"]
mod ui_transcript_tool_sections;

#[path = "ui_transcript_subagent.rs"]
mod ui_transcript_subagent;

#[path = "ui_transcript_parts.rs"]
mod ui_transcript_parts;

#[path = "ui_transcript_sections.rs"]
mod ui_transcript_sections;

#[path = "ui_transcript_compaction.rs"]
mod ui_transcript_compaction;

pub(crate) use ui_transcript_entry::TranscriptVisualEntryId;
pub(super) use ui_transcript_entry::{
    ResolvedTranscriptVisualEntryDraft, TranscriptVisualEntryDisplayMode,
    TranscriptVisualEntryHitRegion, TranscriptVisualEntryMetadata,
};
use ui_transcript_render::build_transcript_render_surfaces;
use ui_transcript_sections::{build_transcript_sections, prepare_transcript_sections};
#[cfg(test)]
use ui_transcript_tool_render::append_tool_call_section_lines;
#[cfg(test)]
use ui_transcript_tool_sections::{build_tool_call_section, build_transcript_tool_call_section};
pub(in crate::ui) use ui_transcript_types::TranscriptBlockPlacement;
pub(crate) use ui_transcript_types::TranscriptRenderSurfaceKind;
use ui_transcript_types::*;
pub(super) use ui_transcript_types::{
    ToolRailMotion, TranscriptToolCallDetailBlock, TranscriptToolCallDetailTone,
    TranscriptVisualEntryDraft,
};

#[cfg(test)]
use super::ui_transcript_surface::render_transcript_surface_lines;

#[cfg(test)]
use super::ui_transcript_selection::TranscriptSelectionDebugSnapshot;

#[cfg(test)]
use super::ui_tool_delegation::subagent_profile_label;
#[cfg(test)]
use super::ui_transcript_test_helpers::{
    transcript_section_model_test_activity, transcript_section_model_test_tool_call,
    transcript_test_line_texts,
};

#[derive(Clone)]
pub(crate) struct TranscriptNavigationEntry {
    pub(crate) id: TranscriptVisualEntryId,
    pub(crate) activity_first_seq: u64,
    pub(crate) kind: TranscriptRenderSurfaceKind,
    pub(crate) context_group: bool,
    pub(crate) target: Option<TranscriptMouseTarget>,
    pub(crate) top: usize,
    pub(crate) height: usize,
    pub(crate) max_scroll: usize,
    pub(crate) text: Arc<str>,
    pub(crate) source_text: Option<Arc<str>>,
}

pub(crate) fn transcript_navigation_entries(
    app: &AppState,
    area: Rect,
) -> Vec<TranscriptNavigationEntry> {
    let Some(transcript_area) = resolved_transcript_area(app, area) else {
        return Vec::new();
    };
    let context = transcript_pane_context(app, transcript_area, app.theme());
    let scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        context.inner_area.width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let viewport = transcript_viewport_layout(context.inner_area, scrollbar).content;
    with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        viewport.width,
        context.base_surface,
        |layout| {
            layout
                .sections
                .iter()
                .flat_map(|section| {
                    section
                        .surfaces
                        .iter()
                        .filter(|surface| {
                            surface.kind != TranscriptRenderSurfaceKind::AssistantFooter
                        })
                        .map(|surface| {
                            let top =
                                section.top_row + section.leading_gap_height + surface.top_offset;
                            TranscriptNavigationEntry {
                                id: surface.metadata.id,
                                activity_first_seq: section.activity_first_seq,
                                kind: surface.kind,
                                context_group: surface.metadata.context_group,
                                target: surface
                                    .interaction_rows
                                    .as_ref()
                                    .and_then(|rows| rows.iter().flatten().next())
                                    .map(|row| row.target.clone()),
                                top,
                                height: surface.height,
                                max_scroll: layout
                                    .total_height
                                    .saturating_sub(usize::from(viewport.height)),
                                text: Arc::clone(&surface.rendered_text),
                                source_text: surface.source_text.clone(),
                            }
                        })
                })
                .collect()
        },
    )
}

pub(crate) fn transcript_entry_scroll_top(
    app: &AppState,
    area: Rect,
    entry_top: usize,
) -> Option<usize> {
    let context = transcript_pane_context(app, resolved_transcript_area(app, area)?, app.theme());
    let scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        context.inner_area.width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let viewport = transcript_viewport_layout(context.inner_area, scrollbar).content;
    Some(with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        viewport.width,
        context.base_surface,
        |layout| {
            let mut top = entry_top;
            for _ in 0..3 {
                let rows = transcript_viewport_rows(layout, usize::from(viewport.height), top);
                top = entry_top.saturating_sub(rows.sticky_height);
            }
            top
        },
    ))
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

    frame.render_widget(context.block.unwrap_or_abort(), area);

    if live_empty_state_visible(app) {
        render_live_empty_state(frame, app, context.inner_area, theme);
        return;
    }

    render_measured_transcript_pane(frame, app, context.inner_area, theme, context.base_surface);
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptPaneContext<'a> {
    pub(super) inner_area: Rect,
    pub(super) base_surface: Color,
    block: Option<ratatui::widgets::Block<'a>>,
}

pub(super) fn transcript_pane_context<'a>(
    app: &AppState,
    area: Rect,
    theme: &'a Theme,
) -> TranscriptPaneContext<'a> {
    let area = Rect {
        height: area.height.saturating_sub(u16::from(
            app.transcript_view.search_editing || !app.transcript_view.search_query.is_empty(),
        )),
        ..area
    };
    if !app.replay_mode {
        let startup_or_empty = app.startup_shell_visible() || live_empty_state_visible(app);
        let horizontal_gutter = if startup_or_empty {
            0
        } else {
            theme.live_shell.rhythm.transcript_gutter_x
        };
        let vertical_gutter = if startup_or_empty {
            0
        } else {
            theme.live_shell.rhythm.transcript_gutter_y
        };
        let mut inner_area = inset_rect(area, horizontal_gutter, 0);
        inner_area.y = inner_area
            .y
            .saturating_add(vertical_gutter.min(area.height));
        inner_area.height = inner_area
            .height
            // The native pane keeps its own bottom gutter. The more-below
            // affordance uses this row even when the live status dock is gone.
            .saturating_sub(vertical_gutter.saturating_mul(2));
        return TranscriptPaneContext {
            inner_area,
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
        if app.transcript_following() {
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
    if app.transcript_view.search_editing || !app.transcript_view.search_query.is_empty() {
        let state = &app.transcript_view;
        let label = if state.search_match_count == 0 {
            format!("/{} · no results", state.search_query)
        } else {
            format!(
                "/{} · {}/{} · n/N next/previous",
                state.search_query,
                state.search_match + 1,
                state.search_match_count
            )
        };
        let footer = Rect::new(inner_area.x, inner_area.bottom(), inner_area.width, 1);
        frame.render_widget(Clear, footer);
        frame.render_widget(
            Paragraph::new(label).style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(theme.surface.canvas),
            ),
            footer,
        );
    }
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
            if layout.sections.is_empty() {
                if app.active_permission_view().is_some() {
                    render_transcript_scrollbar(
                        frame,
                        theme,
                        TranscriptScrollbarRenderSpec {
                            viewport,
                            scroll_top: 0,
                            max_scroll: 0,
                            base_surface: empty_surface,
                            following: app.transcript_following(),
                            drag_active: app.transcript_scrollbar_dragging(),
                        },
                    );
                    return;
                }

                if app.replay_mode {
                    frame.render_widget(
                        Paragraph::new(Text::from(vec![Line::from(Span::styled(
                            "Waiting for first turn…",
                            Style::default().fg(theme.text.secondary),
                        ))]))
                        .style(panel_style(empty_surface, theme.text.primary))
                        .wrap(Wrap { trim: false }),
                        viewport.content,
                    );
                }
                render_transcript_scrollbar(
                    frame,
                    theme,
                    TranscriptScrollbarRenderSpec {
                        viewport,
                        scroll_top: 0,
                        max_scroll: 0,
                        base_surface: empty_surface,
                        following: app.transcript_following(),
                        drag_active: app.transcript_scrollbar_dragging(),
                    },
                );
                return;
            }

            let transcript_scroll = transcript_scroll_top(app, layout, viewport.content.height);
            let TranscriptScrollPosition {
                top: transcript_scroll,
                max_scroll,
                page_flip,
            } = transcript_scroll_position(
                app.transcript_page_flip_state(),
                layout,
                viewport.content.height,
                transcript_scroll,
            );
            let surface_area = transcript_surface_area(
                app,
                viewport.content,
                TranscriptScrollPosition {
                    top: transcript_scroll,
                    max_scroll,
                    page_flip,
                },
            );
            render_integrated_timeline(frame, app, surface_area);
            render_transcript_layout_surfaces(
                frame,
                layout,
                surface_area,
                transcript_scroll,
                app.transcript_animation_phase(),
                theme,
            );
            if let Some(seq) = app.rewind_dim_from_seq() {
                dim_rewind_transcript(frame, layout, surface_area, transcript_scroll, seq, theme);
            }
            if app.focus == Focus::Details && !app.todo_pane_focused() {
                super::ui_transcript_layout::render_selected_transcript_entry(
                    frame,
                    layout,
                    surface_area,
                    transcript_scroll,
                    app.transcript_view.selected_entry,
                    theme,
                );
            }
            render_transcript_selection(
                frame,
                app.transcript_selection(),
                selection_snapshot.as_ref(),
                surface_area,
                theme,
            );
            render_response_position_affordance(frame, surface_area, app);
            render_transcript_scrollbar(
                frame,
                theme,
                TranscriptScrollbarRenderSpec {
                    viewport,
                    scroll_top: transcript_scroll,
                    max_scroll,
                    base_surface: empty_surface,
                    following: app.transcript_following(),
                    drag_active: app.transcript_scrollbar_dragging(),
                },
            );
            render_transcript_more_below_affordance(
                frame,
                transcript_more_below_area(app, viewport.content),
                transcript_scroll,
                max_scroll,
                theme,
                empty_surface,
                app.transcript_view.return_to_live_hovered,
            );
        },
    );
}

fn render_integrated_timeline(frame: &mut Frame, app: &AppState, area: Rect) {
    if !app.transcript_following() || app.transcript_page_flip_scroll_top().is_some() {
        return;
    }
    let Some(outline) = app.transcript_outline.as_ref() else {
        return;
    };
    for (rect, marker) in outline.markers() {
        if rect.x < area.x
            || rect.y < area.y
            || rect.right() > area.right()
            || rect.bottom() > area.bottom()
        {
            continue;
        }
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                marker.glyph(),
                marker
                    .style(
                        crate::transcript_timeline::MarkerInteraction::Normal,
                        app.theme(),
                    )
                    .ratatui_style(),
            ))),
            rect,
        );
    }
}

fn render_response_position_affordance(frame: &mut Frame, area: Rect, app: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(position) = app.transcript_view.response_position.or_else(|| {
        app.transcript_outline
            .as_ref()
            .and_then(|outline| outline.response_position())
    }) else {
        return;
    };
    let theme = app.theme();
    let label = format!("Harness {}/{}", position.index, position.total);
    let width = u16::try_from(label.len())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let rect = Rect::new(area.right().saturating_sub(width), area.y, width, 1);
    frame.render_widget(
        Paragraph::new(label).alignment(Alignment::Right).style(
            Style::default()
                .fg(theme.text.accent)
                .bg(theme.surface.shell),
        ),
        rect,
    );
}

pub(super) fn transcript_surface_area(
    app: &AppState,
    viewport: Rect,
    scroll_position: TranscriptScrollPosition,
) -> Rect {
    if !transcript_more_below_uses_gap(app)
        && scroll_position.max_scroll > 0
        && scroll_position.top < scroll_position.max_scroll
    {
        return Rect::new(
            viewport.x,
            viewport.y,
            viewport.width,
            viewport.height.saturating_sub(1),
        );
    }
    viewport
}

// The live shell's existing prompt gap owns this affordance. Showing it must
// not change the scrollback viewport or its half-page navigation distance.
fn transcript_more_below_area(app: &AppState, viewport: Rect) -> Rect {
    Rect {
        height: viewport
            .height
            .saturating_add(u16::from(transcript_more_below_uses_gap(app))),
        ..viewport
    }
}

fn transcript_more_below_uses_gap(app: &AppState) -> bool {
    !app.replay_mode
        && !app.transcript_view.search_editing
        && app.transcript_view.search_query.is_empty()
        && app
            .last_frame_area()
            .is_none_or(|area| crate::layout::composer_footer_spacer_rows(area.height) > 0)
}

fn transcript_scroll_top(
    app: &AppState,
    layout: &MeasuredTranscriptLayout,
    viewport_height: u16,
) -> usize {
    if let Some(scroll_top) = app.transcript_page_flip_scroll_top() {
        return scroll_top;
    }
    super::ui_transcript_viewport::resolve_viewport(app, layout, viewport_height)
        .0
        .top()
}

fn build_transcript_selection_snapshot(
    app: &AppState,
    area: Rect,
    resolved_selection: Option<TranscriptSelection>,
) -> Option<TranscriptSelectionSnapshot> {
    let transcript_area = resolved_transcript_area(app, area)?;
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
            let scroll_position = transcript_scroll_position(
                app.transcript_page_flip_state(),
                layout,
                viewport.content.height,
                transcript_scroll_top(app, layout, viewport.content.height),
            );
            let selection_viewport =
                transcript_surface_area(app, viewport.content, scroll_position);
            let viewport_rows = transcript_viewport_rows(
                layout,
                usize::from(selection_viewport.height),
                scroll_position.top,
            );
            let visible_rows = (0..usize::from(selection_viewport.height))
                .map(|row| viewport_rows.absolute_row(row))
                .collect::<Vec<_>>();
            let mut requested = visible_rows
                .iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            if let Some(selection) = resolved_selection {
                let last = layout.total_height.saturating_sub(1);
                requested.extend(
                    selection.anchor.row.min(selection.focus.row).min(last)
                        ..=selection.anchor.row.max(selection.focus.row).min(last),
                );
            }
            TranscriptSelectionSnapshot {
                viewport: selection_viewport,
                rows: transcript_selection_rows(layout, render_width, &requested),
                visible_rows,
                total_rows: layout.total_height,
                row_width: render_width,
                resolved_selection,
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
    let transcript_area = resolved_transcript_area(app, area)?;
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

    let resolved_selection = if app.startup_shell_visible() || live_empty_state_visible(app) {
        app.transcript_view
            .transcript_selection_anchors
            .is_none()
            .then(|| app.transcript_selection())
            .flatten()
    } else {
        with_measured_transcript_layout_for_width_on_surface(
            app,
            &theme,
            render_width,
            context.base_surface,
            |layout| {
                app.transcript_selection().and_then(|selection| {
                    if let Some((anchor, focus)) = app.transcript_view.transcript_selection_anchors
                    {
                        return Some(TranscriptSelection {
                            anchor: layout.resolve_selection_anchor(anchor)?,
                            focus: layout.resolve_selection_anchor(focus)?,
                        });
                    }
                    layout.capture_selection_anchor(selection.anchor)?;
                    layout.capture_selection_anchor(selection.focus)?;
                    Some(selection)
                })
            },
        )
    };

    let mut snapshot = build_transcript_selection_snapshot(app, area, resolved_selection)?;
    snapshot.resolved_selection = resolved_selection;
    Some(render(&snapshot))
}

pub(super) fn transcript_hyperlinks(
    layout: &MeasuredTranscriptLayout,
    viewport: Rect,
    scroll_top: usize,
) -> Vec<crate::terminal::FrameHyperlink> {
    let viewport_rows = transcript_viewport_rows(layout, usize::from(viewport.height), scroll_top);
    let mut links = Vec::new();
    for local_row in 0..usize::from(viewport.height) {
        let Some(absolute_row) = viewport_rows.absolute_row(local_row) else {
            continue;
        };
        let Some(row) = transcript_selection_row_at(layout, absolute_row) else {
            continue;
        };
        for link in &row.links {
            let Ok(start_column) = u16::try_from(link.start_cell) else {
                continue;
            };
            let Ok(end_column) = u16::try_from(link.end_cell) else {
                continue;
            };
            links.push(crate::terminal::FrameHyperlink {
                row: viewport
                    .y
                    .saturating_add(u16::try_from(local_row).unwrap_or(u16::MAX)),
                start_column: viewport.x.saturating_add(start_column),
                end_column: viewport.x.saturating_add(end_column).min(viewport.right()),
                destination: link.destination.clone(),
            });
        }
    }
    links
}

fn transcript_selection_row_at(
    layout: &MeasuredTranscriptLayout,
    absolute_row: usize,
) -> Option<&SelectionRow> {
    let section_index = layout
        .sections
        .partition_point(|section| section.top_row <= absolute_row)
        .checked_sub(1)?;
    let section = layout.sections.get(section_index)?;
    let section_top = section.top_row.saturating_add(section.leading_gap_height);
    let content_row = absolute_row.checked_sub(section_top)?;
    let surface = section.surfaces.iter().find(|surface| {
        content_row >= surface.top_offset
            && content_row < surface.top_offset.saturating_add(surface.height)
    })?;
    surface
        .selection_rows
        .get(content_row.saturating_sub(surface.top_offset))
}

fn transcript_selection_rows(
    layout: &MeasuredTranscriptLayout,
    width: usize,
    requested: &BTreeSet<usize>,
) -> Vec<SelectionRow> {
    if layout.total_height == 0 || width == 0 {
        return Vec::new();
    }
    let mut rows = requested
        .range(..layout.total_height)
        .map(|&line_index| SelectionRow {
            line_index,
            text: " ".repeat(width),
            width,
            continues_previous: false,
            copy_joiner: None,
            start_cell: 1,
            end_cell: 0,
            links: Vec::new(),
        })
        .collect::<Vec<_>>();
    let sections = rows
        .iter()
        .filter_map(|row| {
            layout
                .sections
                .partition_point(|section| section.top_row <= row.line_index)
                .checked_sub(1)
        })
        .collect::<BTreeSet<_>>();
    for index in sections {
        let section = &layout.sections[index];
        let section_top = section.top_row.saturating_add(section.leading_gap_height);
        for surface in &section.surfaces {
            let top = section_top.saturating_add(surface.top_offset);
            let start = rows.partition_point(|row| row.line_index < top);
            let end =
                rows.partition_point(|row| row.line_index < top.saturating_add(surface.height));
            if start == end {
                continue;
            }
            let content = &surface.selection_rows;
            for target in &mut rows[start..end] {
                if let Some(row) = content.get(target.line_index - top) {
                    let line_index = target.line_index;
                    *target = row.clone();
                    target.line_index = line_index;
                }
            }
        }
    }
    rows
}

#[cfg(test)]
pub(crate) fn build_transcript_lines(app: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    build_transcript_lines_for_width(app, theme, DIFF_SIDE_BY_SIDE_MIN_WIDTH.saturating_sub(1))
}

#[cfg(test)]
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
        |layout| transcript_layout_lines(layout, app.transcript_animation_phase(), theme),
    )
}

fn build_measured_transcript_layout_for_width(
    app: &AppState,
    theme: &Theme,
    width: u16,
) -> MeasuredTranscriptLayout {
    build_measured_transcript_layout_for_width_on_surface(app, theme, width, theme.surface.shell)
}

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

pub(super) fn with_measured_transcript_layout_for_width_on_surface<R>(
    app: &AppState,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    render: impl FnOnce(&MeasuredTranscriptLayout) -> R,
) -> R {
    ui_transcript_frame::with_layout(app, theme, width, base_surface, render)
}

pub(crate) fn transcript_scrollbar_hit(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptScrollbarHit> {
    let context = transcript_pane_context(app, resolved_transcript_area(app, area)?, app.theme());
    let max_scroll = app.transcript_view.viewport.max_scroll();
    if max_scroll == 0 {
        return None;
    }

    let viewport = transcript_viewport_layout(context.inner_area, true);
    let scroll_top = app.transcript_page_flip_scroll_top().unwrap_or_else(|| {
        current_transcript_scroll_top(
            app.transcript_following(),
            app.transcript_scroll_offset(),
            max_scroll,
        )
    });
    let geometry = transcript_scrollbar_geometry(viewport, scroll_top, max_scroll)?;
    rect_contains(geometry.lane, column, row).then_some(geometry)
}

pub(crate) fn transcript_return_to_live_hit(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> bool {
    if app.transcript_following() {
        return false;
    }
    let theme = *app.theme();
    let Some(transcript_area) = resolved_transcript_area(app, area) else {
        return false;
    };
    let context = transcript_pane_context(app, transcript_area, &theme);
    let full_width = context.inner_area.width;
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        full_width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let viewport = transcript_viewport_layout(context.inner_area, show_scrollbar).content;

    with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        viewport.width,
        context.base_surface,
        |layout| {
            let scroll_position = transcript_scroll_position(
                app.transcript_page_flip_state(),
                layout,
                viewport.height,
                transcript_scroll_top(app, layout, viewport.height),
            );
            transcript_more_below_hit_rect(
                transcript_more_below_area(app, viewport),
                scroll_position.top,
                scroll_position.max_scroll,
            )
            .is_some_and(|target| rect_contains(target, column, row))
        },
    )
}

pub(crate) fn transcript_selection_cell(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptSelectionCell> {
    with_transcript_selection_snapshot(app, area, |snapshot| {
        if app.transcript_view.transcript_selection_anchors.is_some()
            && snapshot.resolved_selection.is_none()
        {
            return None;
        }
        snapshot.hit(column, row)
    })
    .flatten()
}

pub(crate) fn transcript_diff_hunk_rows(app: &AppState, area: Rect) -> Vec<usize> {
    let theme = *app.theme();
    let Some(transcript_area) = resolved_transcript_area(app, area) else {
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

/// Resolve the transcript pane area used for paint and hit-testing.
/// Live run shell paints breadcrumb inside `plan.transcript` and shrinks the
/// remaining pane; interaction paths must use the same shrink or hitboxes
/// land two rows above painted content.
pub(super) fn resolved_transcript_area(app: &AppState, area: Rect) -> Option<Rect> {
    let transcript_area = FrameLayoutPlan::for_app(app, area).transcript?;
    if app.replay_mode || app.startup_shell_visible() {
        return Some(transcript_area);
    }
    Some(super::ui_lifecycle::live_transcript_area_with_breadcrumb(
        transcript_area,
    ))
}

pub(crate) fn transcript_mouse_target(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptMouseTarget> {
    let theme = *app.theme();
    let transcript_area = resolved_transcript_area(app, area)?;
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
            let scroll_position = transcript_scroll_position(
                app.transcript_page_flip_state(),
                layout,
                viewport.height,
                transcript_scroll_top(app, layout, viewport.height),
            );
            let surface_viewport = transcript_surface_area(app, viewport, scroll_position);
            let viewport_rows = transcript_viewport_rows(
                layout,
                usize::from(surface_viewport.height),
                scroll_position.top,
            );
            transcript_mouse_target_at(layout, surface_viewport, viewport_rows, column, row)
        },
    )
}

pub(crate) fn transcript_timeline_turn_at(
    app: &AppState,
    _area: Rect,
    column: u16,
    row: u16,
) -> Option<crate::transcript_identity::TurnId> {
    if !app.transcript_following() || app.transcript_page_flip_scroll_top().is_some() {
        return None;
    }
    app.transcript_outline
        .as_ref()?
        .markers()
        .find(|(rect, _)| rect.contains((column, row).into()))
        .map(|(_, marker)| marker.turn_id)
}

pub(crate) fn transcript_selection_text(
    app: &AppState,
    area: Rect,
    selection: TranscriptSelection,
) -> Option<String> {
    with_transcript_selection_snapshot(app, area, |snapshot| snapshot.selection_text(selection))
        .flatten()
}

pub(crate) fn transcript_selection_text_with_destinations(
    app: &AppState,
    area: Rect,
    selection: TranscriptSelection,
) -> Option<String> {
    with_transcript_selection_snapshot(app, area, |snapshot| {
        snapshot.selection_text_with_destinations(selection)
    })
    .flatten()
}

pub(crate) fn transcript_selection_patch_text(
    app: &AppState,
    area: Rect,
    selection: TranscriptSelection,
) -> Option<String> {
    let selected = transcript_selection_text(app, area, selection)?;
    let sections = build_transcript_sections(app);
    let mut patches = Vec::new();
    for tool in sections.iter().flat_map(|turn| {
        turn.assistant_parts.iter().filter_map(|part| match part {
            TranscriptAssistantPart::ToolCall(tool) => Some(tool.as_ref()),
            _ => None,
        })
    }) {
        if tool.details_visible() {
            collect_selected_diff_patches(&tool.detail_blocks, &selected, &mut patches);
        }
    }
    if patches.is_empty() {
        None
    } else {
        Some(
            patches
                .into_iter()
                .map(|patch| {
                    if patch.ends_with('\n') {
                        patch
                    } else {
                        format!("{patch}\n")
                    }
                })
                .collect(),
        )
    }
}

fn collect_selected_diff_patches(
    blocks: &[TranscriptToolCallDetailBlock],
    selected: &str,
    patches: &mut Vec<String>,
) {
    for block in blocks {
        match block {
            TranscriptToolCallDetailBlock::StructuredDiff { diff_content, .. }
                if unified_patch_change_is_selected(diff_content, selected) =>
            {
                if !patches.contains(diff_content) {
                    patches.push(diff_content.clone());
                }
            }
            TranscriptToolCallDetailBlock::FileSection(section)
                if section.disclosure_state == TranscriptToolCallDisclosureState::Expanded =>
            {
                collect_selected_diff_patches(&section.detail_blocks, selected, patches);
            }
            _ => {}
        }
    }
}

fn unified_patch_change_is_selected(patch: &str, selected: &str) -> bool {
    patch.lines().any(|line| {
        let Some(body) = line.strip_prefix(['+', '-']) else {
            return false;
        };
        !line.starts_with("+++")
            && !line.starts_with("---")
            && !body.is_empty()
            && selected.contains(body)
    })
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
mod response_position_tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn response_position_affordance_paints_index_and_total_with_contrast() {
        // arrange
        let mut app = AppState::new_live(None, false, None);
        app.transcript_view.response_position =
            Some(crate::transcript_timeline::ResponsePosition { index: 2, total: 3 });
        let backend = TestBackend::new(30, 2);
        let mut terminal = Terminal::new(backend).unwrap_or_abort();

        // act
        terminal
            .draw(|frame| {
                render_response_position_affordance(frame, Rect::new(0, 0, 30, 2), &app);
            })
            .unwrap_or_abort();

        // assert
        let buffer = terminal.backend().buffer();
        let row = (0..30).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
        let index_start = row.find("2/3").unwrap_or_abort();
        for x in index_start..index_start + 3 {
            let cell = &buffer[(u16::try_from(x).unwrap_or_abort(), 0)];
            assert_ne!(cell.fg, cell.bg);
        }
    }
}

#[cfg(test)]
#[path = "ui_transcript_streaming_tests.rs"]
mod streaming_tests;
#[cfg(test)]
#[path = "ui_transcript_tests.rs"]
mod tests;

fn dim_rewind_transcript(
    frame: &mut Frame,
    layout: &super::ui_transcript_layout::MeasuredTranscriptLayout,
    surface_area: Rect,
    transcript_scroll: usize,
    seq: u64,
    theme: &Theme,
) {
    for index in layout.visible_sections(transcript_scroll, usize::from(surface_area.height)) {
        let section = &layout.sections[index];
        if section.activity_first_seq < seq {
            continue;
        }
        for surface_index in 0..section.surfaces.len() {
            if let Some(placement) =
                super::ui_transcript_layout::transcript_visual_entry_viewport_placement(
                    layout,
                    surface_area,
                    transcript_scroll,
                    index,
                    surface_index,
                )
            {
                dim_rewind_rect(frame.buffer_mut(), placement.rect, theme);
            }
        }
    }
}

fn dim_rewind_rect(buffer: &mut ratatui::buffer::Buffer, area: Rect, theme: &Theme) {
    let gray = if theme.is_dark() { 88 } else { 165 };
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                if theme.surface.canvas == ratatui::style::Color::Reset {
                    cell.modifier.insert(ratatui::style::Modifier::DIM);
                    cell.modifier.remove(ratatui::style::Modifier::BOLD);
                } else {
                    cell.fg = crate::theme::quantize_color(
                        ratatui::style::Color::Rgb(gray, gray, gray),
                        theme.color_level(),
                    );
                }
            }
        }
    }
}
