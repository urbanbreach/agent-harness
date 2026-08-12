// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use crate::app::{ToolCallPresentation, ToolCallPresentationStatus};
use crate::UnwrapOrAbort;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use super::*;

use crate::text::{collapse_inline_whitespace, has_trimmed_content};
use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;
use crate::time_format::short_time_or_trimmed;

use super::ui_diff::{render_structured_diff_lines_with_hunk_offsets, StructuredDiffRenderOptions};
use super::ui_markdown::{
    append_rich_text_block, markdown_heading_text, markdown_rule, parse_inline_markdown_spans,
    raw_url_length,
};
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
    block_tool_color, generic_tool_visual_style, inline_tool_color, task_inline_tool_color,
    tool_call_header_style, TranscriptToolCallVisualStyle,
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
    transcript_diff_hunk_rows_for_layout, transcript_layout_has_visible_running_tool,
    transcript_layout_lines, transcript_viewport_rows, MeasuredTranscriptLayout,
    MeasuredTranscriptSurface,
};
use super::ui_transcript_page_flip::{transcript_scroll_position, TranscriptScrollPosition};
use super::ui_transcript_scrollbar::{
    current_transcript_scroll_top, render_transcript_more_below_affordance,
    render_transcript_scrollbar, transcript_more_below_rect, transcript_scroll_offset,
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
    assistant_footer_label, assistant_primary_label_color, assistant_primary_rail_color,
    blend_color, selected_foreground_for_badge, thinking_header_color,
    transcript_emphasized_surface, transcript_nested_rail_color,
    transcript_running_tool_marker_color, transcript_streaming_spinner_frame,
    transcript_streaming_spinner_frame_with_motion,
};
use super::ui_transcript_surface::{
    append_nested_surface_row, append_prebuilt_nested_surface_lines, append_prebuilt_surface_lines,
    append_prefixed_wrapped_spans_line, append_surface_row, append_user_surface_text_block,
    append_user_surface_text_block_with_first_line_reserve, nested_surface_prefix_width,
    surface_prefix_width, surface_span, transcript_surface_content_width,
    transcript_surface_render_width, user_surface_line, wrap_surface_spans, TRANSCRIPT_RAIL_GLYPH,
};
#[path = "ui_transcript_types.rs"]
mod ui_transcript_types;

#[path = "ui_transcript_render.rs"]
mod ui_transcript_render;

#[path = "ui_reasoning_markdown.rs"]
mod ui_reasoning_markdown;

#[path = "ui_transcript_tool_render.rs"]
mod ui_transcript_tool_render;

#[path = "ui_transcript_tool_sections.rs"]
mod ui_transcript_tool_sections;

#[path = "ui_transcript_sections.rs"]
mod ui_transcript_sections;

#[path = "ui_transcript_compaction.rs"]
mod ui_transcript_compaction;

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
    ToolRailMotion, TranscriptRenderSurface, TranscriptRenderSurfaceKind,
    TranscriptToolCallDetailBlock, TranscriptToolCallDetailTone,
};

#[cfg(test)]
use super::ui_transcript_surface::{render_transcript_surface_lines, visible_surface_lines};

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
    #[cfg(test)]
    static TRANSCRIPT_SECTION_RENDER_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn reset_transcript_section_render_count_for_test() {
    TRANSCRIPT_SECTION_RENDER_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn transcript_section_render_count_for_test() -> usize {
    TRANSCRIPT_SECTION_RENDER_COUNT.with(Cell::get)
}

fn record_transcript_section_render_for_test() {
    #[cfg(test)]
    TRANSCRIPT_SECTION_RENDER_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

fn cached_measured_section(
    index: usize,
    section: &TranscriptTurnSection,
    previous: Option<&TranscriptLayoutCacheEntry>,
) -> Option<Rc<MeasuredTranscriptSection>> {
    let previous = previous?;
    transcript_section_cache_matches(previous.sections.get(index)?, section)
        .then(|| previous.layout.sections.get(index).cloned())
        .flatten()
}

fn transcript_section_cache_matches(
    candidate: &TranscriptTurnSection,
    section: &TranscriptTurnSection,
) -> bool {
    candidate.activity_first_seq == section.activity_first_seq
        && candidate.request_id == section.request_id
        && candidate.user_message == section.user_message
        && candidate.show_footer == section.show_footer
        && candidate.footer_timestamp == section.footer_timestamp
        && candidate.reasoning_expanded == section.reasoning_expanded
        && candidate.header == section.header
        && candidate.body_blocks == section.body_blocks
        && transcript_tool_calls_cache_match(&candidate.tool_calls, &section.tool_calls)
        && candidate.thinking == section.thinking
        && candidate.error == section.error
        && transcript_assistant_parts_cache_match(
            &candidate.assistant_parts,
            &section.assistant_parts,
        )
}

fn transcript_tool_calls_cache_match(
    candidate: &[TranscriptToolCallSection],
    section: &[TranscriptToolCallSection],
) -> bool {
    candidate.len() == section.len()
        && candidate
            .iter()
            .zip(section)
            .all(|(candidate, section)| transcript_tool_call_cache_matches(candidate, section))
}

fn transcript_tool_call_cache_matches(
    candidate: &TranscriptToolCallSection,
    section: &TranscriptToolCallSection,
) -> bool {
    candidate.tool_call_id == section.tool_call_id
        && candidate.coalesced_tool_call_ids == section.coalesced_tool_call_ids
        && candidate.child_session_id == section.child_session_id
        && candidate.hovered_target == section.hovered_target
        && transcript_tool_header_cache_matches(&candidate.header, &section.header)
        && candidate.detail_blocks == section.detail_blocks
        && candidate.details_collapsed_by_default == section.details_collapsed_by_default
        && candidate.details_preview_visible == section.details_preview_visible
        && candidate.expanded == section.expanded
        && transcript_tool_rail_motion_cache_matches(candidate.rail_motion, section.rail_motion)
}

fn transcript_tool_header_cache_matches(
    candidate: &TranscriptToolCallHeader,
    section: &TranscriptToolCallHeader,
) -> bool {
    candidate.tool_id == section.tool_id
        && candidate.title == section.title
        && candidate.subtitle == section.subtitle
        && candidate.path_metadata == section.path_metadata
        && (candidate.icon == section.icon
            || (candidate.presentation.status == ToolCallPresentationStatus::Running
                && section.presentation.status == ToolCallPresentationStatus::Running))
        && candidate.presentation == section.presentation
        && candidate.visual_style == section.visual_style
        && candidate.struck_out == section.struck_out
        && candidate.disclosure_state == section.disclosure_state
}

fn transcript_tool_rail_motion_cache_matches(
    candidate: ToolRailMotion,
    section: ToolRailMotion,
) -> bool {
    match (candidate, section) {
        (ToolRailMotion::Running { .. }, ToolRailMotion::Running { .. })
        | (ToolRailMotion::FinishFlash { .. }, ToolRailMotion::FinishFlash { .. }) => true,
        _ => candidate == section,
    }
}

fn transcript_assistant_parts_cache_match(
    candidate: &[TranscriptAssistantPart],
    section: &[TranscriptAssistantPart],
) -> bool {
    candidate.len() == section.len()
        && candidate
            .iter()
            .zip(section)
            .all(|(candidate, section)| match (candidate, section) {
                (
                    TranscriptAssistantPart::ToolCall(candidate),
                    TranscriptAssistantPart::ToolCall(section),
                ) => transcript_tool_call_cache_matches(candidate, section),
                _ => candidate == section,
            })
}

pub(super) fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if let Some(viewer) = app.transcript_viewer() {
        let surface = viewer.render_surface(area);
        crate::transcript_block_viewer::render_to_buffer(frame.buffer_mut(), area, &surface);
        return;
    }
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
        return TranscriptPaneContext {
            inner_area: inset_rect(area, horizontal_gutter, vertical_gutter),
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
                        viewport,
                        0,
                        0,
                        theme,
                        empty_surface,
                        app.transcript_scrollbar_dragging(),
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
                    viewport,
                    0,
                    0,
                    theme,
                    empty_surface,
                    app.transcript_scrollbar_dragging(),
                );
                return;
            }

            let regular_max = layout
                .total_height
                .saturating_sub(usize::from(viewport.content.height));
            app.transcript_view
                .last_transcript_viewport_height
                .set(usize::from(viewport.content.height));
            app.transcript_view.record_measured_max_scroll(regular_max);
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
            app.set_transcript_page_flip_state(page_flip);
            app.transcript_view.record_measured_max_scroll(max_scroll);
            let surface_area = transcript_surface_area(
                viewport.content,
                TranscriptScrollPosition {
                    top: transcript_scroll,
                    max_scroll,
                    page_flip,
                },
            );
            app.record_visible_running_tool_motion(transcript_layout_has_visible_running_tool(
                layout,
                usize::from(surface_area.height),
                transcript_scroll,
            ));
            render_integrated_timeline(frame, app, surface_area);
            render_transcript_layout_surfaces(
                frame,
                layout,
                surface_area,
                transcript_scroll,
                app.transcript_animation_phase(),
                theme,
            );
            render_transcript_selection(
                frame,
                app.transcript_selection(),
                selection_snapshot.as_ref(),
                surface_area,
                theme,
            );
            render_transcript_scrollbar(
                frame,
                viewport,
                transcript_scroll,
                max_scroll,
                theme,
                empty_surface,
                app.transcript_scrollbar_dragging(),
            );
            render_transcript_more_below_affordance(
                frame,
                viewport.content,
                transcript_scroll,
                max_scroll,
                theme,
                empty_surface,
            );
        },
    );
}

fn render_integrated_timeline(frame: &mut Frame, app: &AppState, area: Rect) {
    if !app.transcript_following() || app.transcript_page_flip_scroll_top().is_some() {
        return;
    }
    let Some(view) = app.transcript_view_model() else {
        return;
    };
    for marker in &view.timeline.marker_rects {
        if marker.rect.x < area.x
            || marker.rect.y < area.y
            || marker.rect.right() > area.right()
            || marker.rect.bottom() > area.bottom()
        {
            continue;
        }
        let Some(turn) = view
            .turns
            .iter()
            .find(|turn| turn.turn_id() == marker.turn_id)
        else {
            continue;
        };
        let scroll_top = view.scroll_top.floor().to_string().parse::<usize>();
        let interaction = if scroll_top == Ok(view.timeline.scroll_top)
            && view.screen.focus_follow().focus
                == crate::transcript_identity::TranscriptFocus::Timeline
            && view.screen.focus_follow().follow
        {
            crate::transcript_timeline::MarkerInteraction::Active
        } else {
            crate::transcript_timeline::MarkerInteraction::Normal
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                marker.label.clone(),
                turn.marker.style(interaction, app.theme()).ratatui_style(),
            ))),
            marker.rect,
        );
    }
}

fn transcript_surface_area(viewport: Rect, scroll_position: TranscriptScrollPosition) -> Rect {
    if scroll_position.max_scroll > 0 && scroll_position.top < scroll_position.max_scroll {
        return Rect::new(
            viewport.x,
            viewport.y,
            viewport.width,
            viewport.height.saturating_sub(1),
        );
    }
    viewport
}

fn transcript_scroll_top(
    app: &AppState,
    layout: &MeasuredTranscriptLayout,
    viewport_height: u16,
) -> usize {
    if let Some(scroll_top) = app.transcript_page_flip_scroll_top() {
        return scroll_top;
    }
    let max_scroll = layout
        .total_height
        .saturating_sub(usize::from(viewport_height));
    let viewport = app.transcript_view.measured_viewport();
    if viewport.is_following() {
        app.transcript_view.measured_anchor.set(None);
        return max_scroll;
    }
    let scroll_top = app
        .transcript_view
        .measured_anchor
        .get()
        .and_then(|anchor| layout.resolve_content_anchor(anchor))
        .unwrap_or_else(|| viewport.top())
        .min(max_scroll);
    app.transcript_view
        .set_resolved_measured_top(scroll_top, max_scroll);
    app.transcript_view
        .measured_anchor
        .set(layout.capture_content_anchor(scroll_top));
    scroll_top
}

fn build_transcript_selection_snapshot(
    app: &AppState,
    area: Rect,
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
            let (rows, line_texts, continues_previous) =
                transcript_selection_rows(layout, render_width);
            let scroll_position = transcript_scroll_position(
                app.transcript_page_flip_state(),
                layout,
                viewport.content.height,
                transcript_scroll_top(app, layout, viewport.content.height),
            );
            let selection_viewport = transcript_surface_area(viewport.content, scroll_position);
            let viewport_rows = transcript_viewport_rows(
                layout,
                usize::from(selection_viewport.height),
                scroll_position.top,
            );
            TranscriptSelectionSnapshot {
                viewport: selection_viewport,
                visible_rows: (0..usize::from(selection_viewport.height))
                    .filter_map(|row| viewport_rows.absolute_row(row))
                    .collect(),
                rows,
                line_texts,
                continues_previous,
                row_width: render_width,
                resolved_selection: Cell::new(None),
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
            .get()
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
                    if let Some((anchor, focus)) =
                        app.transcript_view.transcript_selection_anchors.get()
                    {
                        return Some(TranscriptSelection {
                            anchor: layout.resolve_selection_anchor(anchor)?,
                            focus: layout.resolve_selection_anchor(focus)?,
                        });
                    }
                    let anchors = (
                        layout.capture_selection_anchor(selection.anchor)?,
                        layout.capture_selection_anchor(selection.focus)?,
                    );
                    app.transcript_view
                        .transcript_selection_anchors
                        .set(Some(anchors));
                    Some(selection)
                })
            },
        )
    };

    with_cached_transcript_selection_snapshot(
        TranscriptSelectionCacheKey {
            render_width,
            app_instance_id: app.transcript_cache_instance_id(),
            render_key: app.transcript_selection_cache_key(),
            theme,
            area: transcript_area,
            follow_mode: app.transcript_following(),
            transcript_scroll: app.transcript_scroll_offset(),
        },
        || build_transcript_selection_snapshot(app, area),
        |snapshot| {
            snapshot.resolved_selection.set(resolved_selection);
            render(snapshot)
        },
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
        let previous_cache = cache.borrow();
        let previous = previous_cache.iter().rev().find(|entry| {
            entry.app_instance_id == app_instance_id
                && entry.theme == *theme
                && entry.width == width
                && entry.base_surface == base_surface
        });
        let layout = measure_transcript_layout(
            &sections,
            theme,
            width,
            base_surface,
            |section| section.activity_first_seq,
            |index, section| cached_measured_section(index, section, previous),
            |section, theme, width, base_surface| {
                record_transcript_section_render_for_test();
                build_transcript_render_surfaces(section, theme, width, base_surface)
            },
        );
        drop(previous_cache);

        {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| {
                entry.app_instance_id != app_instance_id
                    || entry.theme != *theme
                    || entry.width != width
                    || entry.base_surface != base_surface
            });
            cache.push(TranscriptLayoutCacheEntry {
                app_instance_id,
                render_key,
                theme: *theme,
                width,
                base_surface,
                sections,
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
            .unwrap_or_abort();
        render(&entry.layout)
    })
}

pub(crate) fn transcript_scrollbar_hit(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptScrollbarHit> {
    let context = transcript_pane_context(app, resolved_transcript_area(app, area)?, app.theme());
    let max_scroll = app.transcript_view.last_transcript_max_scroll.get();
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
    if !app.active_turn_in_progress() || app.transcript_following() {
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
            transcript_more_below_rect(viewport, scroll_position.top, scroll_position.max_scroll)
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
        if app
            .transcript_view
            .transcript_selection_anchors
            .get()
            .is_some()
            && snapshot.resolved_selection.get().is_none()
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
fn resolved_transcript_area(app: &AppState, area: Rect) -> Option<Rect> {
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
            let surface_viewport = transcript_surface_area(viewport, scroll_position);
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
    app.transcript_view_model()
        .and_then(|view| view.hit_map.hit_test(column, row))
}

pub(crate) fn transcript_selection_text(
    app: &AppState,
    area: Rect,
    selection: TranscriptSelection,
) -> Option<String> {
    with_transcript_selection_snapshot(app, area, |snapshot| snapshot.selection_text(selection))
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
    for tool in sections.iter().flat_map(|turn| turn.tool_calls.iter()) {
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
pub(crate) fn transcript_selection_row_count(app: &AppState, area: Rect) -> Option<usize> {
    with_transcript_selection_snapshot(app, area, |snapshot| snapshot.rows.len())
}

#[cfg(test)]
#[path = "ui_transcript_exact_tests.rs"]
mod ui_transcript_exact_tests;
#[cfg(test)]
pub(crate) use ui_transcript_exact_tests::*;
#[cfg(test)]
#[path = "ui_transcript_streaming_tests.rs"]
mod streaming_tests;
#[cfg(test)]
#[path = "ui_transcript_tests.rs"]
mod tests;
