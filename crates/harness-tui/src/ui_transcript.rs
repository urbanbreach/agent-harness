use std::cell::RefCell;
use std::path::Path;

use super::*;

use crate::text::{collapse_inline_whitespace, has_trimmed_content};
use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;
use crate::time_format::short_time_or_trimmed;

use super::ui_diff::{render_structured_diff_lines_with_hunk_offsets, StructuredDiffRenderOptions};
use super::ui_markdown::{append_rich_text_block, parse_inline_markdown_spans};
use super::ui_tool_delegation::{
    agent_spawn_context_line, agent_spawn_description, agent_spawn_title,
    hidden_delegated_child_request_ids, task_tool_child_session_id,
};
use super::ui_tool_diffs::{
    apply_patch_tool_header_metadata, collect_apply_patch_file_render_entries,
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
    tool_call_path_metadata, tool_in_path_suffix, tool_match_count_suffix, tool_path_display,
    TranscriptPathMetadata,
};
use super::ui_tool_question_todo::{
    ordered_todo_items, question_tool_subtitle, resolved_question_answer_items,
    todo_items_from_tool_call, todo_tool_title, TranscriptQuestionAnswerItem, TranscriptTodoItem,
};
use super::ui_tool_style::{
    block_tool_color, block_tool_rail_color, generic_tool_visual_style, inline_tool_color,
    task_inline_tool_color, tool_call_header_style, TranscriptToolCallVisualStyle,
};
use super::ui_tool_titles::{
    background_output_tool_subtitle, background_output_tool_title, batch_tool_title,
    format_duration_ms, generic_tool_title, is_mcp_tool_id, mcp_tool_title,
};
use super::ui_tool_visibility::{
    tool_call_should_remain_visible_without_tool_details, tool_disclosure_state,
    tool_header_disclosure_glyph, tool_hidden_from_transcript, TranscriptToolCallDisclosureState,
};
use super::ui_transcript_bash::{
    append_harness_bash_panel, shell_tool_command, shell_tool_output, shell_tool_subtitle,
    shell_tool_title_description, HarnessBashPanel, HARNESS_BASH_OUTPUT_LINE_CLAMP,
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
    blank_selection_row, lifecycle_selection_snapshot, render_transcript_selection,
    selection_rows_for_markdownish_text_block, selection_rows_for_rendered_line,
    transcript_selection_line_rows, with_cached_transcript_selection_snapshot, TranscriptSelection,
    TranscriptSelectionCacheKey, TranscriptSelectionCell, TranscriptSelectionRow,
    TranscriptSelectionSnapshot,
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
use super::ui_transcript_bash::{HARNESS_SPLIT_RAIL_GLYPH, TRANSCRIPT_COMMAND_TOOL_INDENT};

#[cfg(test)]
use super::ui_tool_delegation::subagent_profile_label;
#[cfg(test)]
use super::ui_transcript_test_helpers::{
    transcript_section_model_test_activity, transcript_section_model_test_tool_call,
    transcript_test_line_texts,
};

#[derive(Debug, Clone, Copy)]
struct TranscriptToolCardShell {
    indent: &'static str,
    rail_color: Color,
    surface: Color,
}

const THINKING_TRACE_LABEL: &str = "Thinking:";

#[derive(Debug)]
struct TranscriptLayoutCacheEntry {
    app_instance_id: u64,
    render_key: u64,
    theme: Theme,
    width: u16,
    base_surface: Color,
    layout: MeasuredTranscriptLayout,
}

thread_local! {
    static TRANSCRIPT_LAYOUT_CACHE: RefCell<Vec<TranscriptLayoutCacheEntry>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptRenderSurface {
    pub(super) kind: TranscriptRenderSurfaceKind,
    pub(super) show_outer_rail: bool,
    pub(super) rail_color: Color,
    pub(super) surface: Color,
    pub(super) lines: Vec<Line<'static>>,
    pub(super) interaction_rows: Option<Vec<Option<TranscriptInteractionRow>>>,
    pub(super) selection_rows: Option<Vec<TranscriptSelectionRow>>,
    pub(super) diff_hunk_offsets: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptRenderSurfaceKind {
    User,
    AssistantReasoning,
    AssistantBody,
    AssistantTool,
    AssistantCommandTool,
    AssistantError,
    AssistantFooter,
}

#[derive(Debug, Clone)]
struct ToolSectionRender {
    lines: Vec<Line<'static>>,
    interaction_rows: Vec<Option<TranscriptInteractionRow>>,
    diff_hunk_offsets: Vec<usize>,
}

struct BuildTurnSectionArgs<'a> {
    activity: &'a ActivityEntry,
    queued_user_message: bool,
    is_selected: bool,
    is_latest: bool,
    thinking_visible: bool,
    timestamps_visible: bool,
    show_tool_details: bool,
    show_generic_tool_output: bool,
    stacked_diffs: bool,
    session_path: Option<&'a Path>,
    app: &'a AppState,
}

#[derive(Debug, Clone)]
struct TranscriptOrderedToolCallSection {
    tool_call_id: String,
    first_seq: u64,
    section: TranscriptToolCallSection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTurnSection {
    request_id: String,
    user_message: Option<TranscriptUserMessageSection>,
    show_footer: bool,
    footer_timestamp: Option<String>,
    animation_phase: usize,
    header: TranscriptTurnHeader,
    body_blocks: Vec<TranscriptBodyBlock>,
    tool_calls: Vec<TranscriptToolCallSection>,
    thinking: Option<TranscriptLabeledTextSection>,
    error: Option<TranscriptErrorSection>,
    assistant_parts: Vec<TranscriptAssistantPart>,
    subagent_hint_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptUserMessageSection {
    text: String,
    queued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTurnHeader {
    status: ActivityStatus,
    is_selected: bool,
    profile_label: String,
    model_id: String,
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptBodyBlock {
    RichText(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLabeledTextSection {
    label: &'static str,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptToolCallSection {
    tool_call_id: String,
    child_session_id: Option<String>,
    hovered_target: Option<TranscriptMouseTarget>,
    header: TranscriptToolCallHeader,
    detail_blocks: Vec<TranscriptToolCallDetailBlock>,
    expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptToolCallHeader {
    tool_id: String,
    title: String,
    subtitle: Option<String>,
    path_metadata: Option<String>,
    icon: Option<&'static str>,
    status: ToolCallDisplayStatus,
    visual_style: TranscriptToolCallVisualStyle,
    struck_out: bool,
    disclosure_state: Option<TranscriptToolCallDisclosureState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TranscriptToolCallDetailBlock {
    Message {
        text: String,
        tone: TranscriptToolCallDetailTone,
    },
    TodoList {
        items: Vec<TranscriptTodoItem>,
    },
    BashPanel {
        command: String,
        output: String,
        description: Option<String>,
        expand_hint: Option<String>,
        tone: TranscriptToolCallDetailTone,
    },
    StructuredDiff {
        diff_content: String,
        fallback_path: Option<String>,
        force_stacked: bool,
        show_file_header: bool,
    },
    FileSection(TranscriptToolCallFileSection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallFileSection {
    tool_call_id: String,
    file_path: String,
    title: String,
    subtitle: Option<String>,
    disclosure_state: TranscriptToolCallDisclosureState,
    pub(super) detail_blocks: Vec<TranscriptToolCallDetailBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolCallDetailTone {
    Primary,
    Secondary,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptErrorSection {
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptAssistantPart {
    Reasoning(TranscriptLabeledTextSection),
    Body(TranscriptBodyBlock),
    ToolCall(Box<TranscriptToolCallSection>),
    SubagentHint,
    Error(TranscriptErrorSection),
}

const TRANSCRIPT_USER_BODY_PREFIX: &str = "  ";
const TRANSCRIPT_ASSISTANT_BODY_PREFIX: &str = "   ";
const TRANSCRIPT_REASONING_BODY_PREFIX: &str = "  ";
const TRANSCRIPT_NESTED_INDENT: &str = "  ";
const TRANSCRIPT_OPCODE_EDIT_INDENT: &str = "    ";

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
        if app.follow_mode { " (following)" } else { "" }
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
            app.last_transcript_max_scroll.set(
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
                app.follow_mode,
                app.transcript_scroll,
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
        |layout| TranscriptSelectionSnapshot {
            viewport: viewport.content,
            scroll_top: transcript_scroll_offset(
                app.follow_mode,
                app.transcript_scroll,
                layout.total_height,
                viewport.content.height,
            ),
            rows: transcript_selection_rows(layout, render_width),
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
            render_key: app.transcript_render_cache_key(),
            theme,
            area: transcript_area,
            follow_mode: app.follow_mode,
            transcript_scroll: app.transcript_scroll,
        },
        || build_transcript_selection_snapshot(app, area),
        render,
    )
}

fn transcript_selection_rows(
    layout: &MeasuredTranscriptLayout,
    width: usize,
) -> Vec<TranscriptSelectionRow> {
    if layout.total_height == 0 || width == 0 {
        return Vec::new();
    }

    let mut rows = vec![
        TranscriptSelectionRow {
            cells: vec![" ".to_string(); width],
            continues_previous: false,
            copy_offset: 0,
        };
        layout.total_height
    ];
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
                let len = row.cells.len().min(width);
                rows[target].cells[..len].clone_from_slice(&row.cells[..len]);
                rows[target].continues_previous = row.continues_previous;
                rows[target].copy_offset = row.copy_offset;
            }
        }
    }

    rows
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
                full.push(TRANSCRIPT_RAIL_GLYPH.to_string());
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

fn build_transcript_lines_for_width(
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

fn build_transcript_sections(app: &AppState) -> Vec<TranscriptTurnSection> {
    let hidden_child_request_ids = hidden_delegated_child_request_ids(app);
    let visible_activities = app
        .activities
        .iter()
        .enumerate()
        .filter(|(_, activity)| !hidden_child_request_ids.contains(activity.request_id.as_str()))
        .collect::<Vec<_>>();
    let mut turn_sections = Vec::with_capacity(visible_activities.len());
    let pending_assistant_index = visible_activities
        .iter()
        .rposition(|(_, activity)| activity.status == ActivityStatus::Streaming);

    for (visible_index, (activity_index, activity)) in visible_activities.iter().enumerate() {
        turn_sections.push(build_turn_section(BuildTurnSectionArgs {
            activity,
            queued_user_message: pending_assistant_index
                .is_some_and(|pending| visible_index > pending),
            is_selected: *activity_index == app.selected_activity_index,
            is_latest: false,
            thinking_visible: app.transcript_thinking_visible(),
            timestamps_visible: app.transcript_timestamps_visible(),
            show_tool_details: app.tool_details_visible(),
            show_generic_tool_output: app.generic_tool_output_visible(),
            stacked_diffs: app.stacked_transcript_diffs(),
            session_path: app.session_path.as_deref(),
            app,
        }));
    }

    if let Some(latest_assistant_footer_index) = turn_sections
        .iter()
        .rposition(turn_supports_assistant_footer)
    {
        let original_activity_index = visible_activities[latest_assistant_footer_index].0;
        if let Some(turn) = turn_sections.get_mut(latest_assistant_footer_index) {
            turn.show_footer = true;
            turn.footer_timestamp = app
                .transcript_timestamps_visible()
                .then_some(
                    app.activities[original_activity_index]
                        .user_timestamp
                        .as_deref(),
                )
                .flatten()
                .map(short_time_or_trimmed);
        }
    }

    turn_sections
}

fn turn_supports_assistant_footer(turn: &TranscriptTurnSection) -> bool {
    !turn.assistant_parts.is_empty() || activity_status_supports_footer_only(turn.header.status)
}

fn build_turn_section(args: BuildTurnSectionArgs<'_>) -> TranscriptTurnSection {
    let BuildTurnSectionArgs {
        activity,
        queued_user_message,
        is_selected,
        is_latest,
        thinking_visible,
        timestamps_visible,
        show_tool_details,
        show_generic_tool_output,
        stacked_diffs,
        session_path,
        app,
    } = args;

    let user_message =
        activity
            .user_message
            .as_ref()
            .map(|user_msg| TranscriptUserMessageSection {
                text: user_msg.text.clone(),
                queued: queued_user_message,
            });

    let thinking = (thinking_visible && activity_has_thinking_text(activity)).then(|| {
        TranscriptLabeledTextSection {
            label: THINKING_TRACE_LABEL,
            text: activity.thinking_text.clone(),
        }
    });

    let mut body_blocks = Vec::new();
    if !activity.transcript_text.is_empty() {
        body_blocks.push(TranscriptBodyBlock::RichText(
            activity.transcript_text.clone(),
        ));
    }

    let ordered_tool_calls = activity
        .tool_calls
        .iter()
        .filter_map(|tool_call| {
            build_tool_call_section(
                tool_call,
                app,
                show_tool_details,
                timestamps_visible,
                show_generic_tool_output,
                app.tool_output_expanded(tool_call),
                stacked_diffs,
                session_path,
            )
            .map(|section| TranscriptOrderedToolCallSection {
                tool_call_id: tool_call.tool_call_id.clone(),
                first_seq: tool_call.first_seq,
                section,
            })
        })
        .collect::<Vec<_>>();
    let tool_calls = ordered_tool_calls
        .iter()
        .map(|tool_call| tool_call.section.clone())
        .collect::<Vec<_>>();
    let error = activity
        .error_message
        .as_ref()
        .map(|text| TranscriptErrorSection { text: text.clone() });
    let assistant_parts = build_ordered_assistant_parts(
        activity,
        app,
        thinking_visible,
        thinking.clone(),
        body_blocks.clone(),
        ordered_tool_calls,
        error.clone(),
    );

    TranscriptTurnSection {
        request_id: activity.request_id.clone(),
        user_message,
        show_footer: is_latest,
        footer_timestamp: (is_latest && timestamps_visible)
            .then_some(activity.user_timestamp.as_deref())
            .flatten()
            .map(short_time_or_trimmed),
        animation_phase: app.transcript_animation_phase(),
        header: TranscriptTurnHeader {
            status: activity.status,
            is_selected,
            profile_label: activity.profile_label.clone(),
            model_id: activity.model_id.clone(),
            duration_ms: activity.duration_ms(),
        },
        body_blocks,
        tool_calls,
        thinking,
        error,
        assistant_parts,
        subagent_hint_key: app.keymap.get_binding_str(Action::SessionChildFirst),
    }
}

fn build_ordered_assistant_parts(
    activity: &ActivityEntry,
    app: &AppState,
    thinking_visible: bool,
    thinking: Option<TranscriptLabeledTextSection>,
    body_blocks: Vec<TranscriptBodyBlock>,
    ordered_tool_calls: Vec<TranscriptOrderedToolCallSection>,
    error: Option<TranscriptErrorSection>,
) -> Vec<TranscriptAssistantPart> {
    let mut event_parts = build_ordered_assistant_parts_from_events(
        activity,
        app,
        &ordered_tool_calls,
        thinking_visible,
    );
    if event_parts.is_empty() {
        let mut fallback_parts = Vec::new();
        if let Some(thinking) = thinking {
            fallback_parts.push(TranscriptAssistantPart::Reasoning(thinking));
        }
        fallback_parts.extend(body_blocks.into_iter().map(TranscriptAssistantPart::Body));
        fallback_parts.extend(
            ordered_tool_calls
                .into_iter()
                .map(|tool_call| TranscriptAssistantPart::ToolCall(Box::new(tool_call.section))),
        );
        insert_subagent_hint_after_task_tools(&mut fallback_parts);
        if let Some(error) = error {
            fallback_parts.push(TranscriptAssistantPart::Error(error));
        }
        return fallback_parts;
    }

    sync_reasoning_parts_with_activity(&mut event_parts, activity, thinking_visible);
    insert_subagent_hint_after_task_tools(&mut event_parts);

    if let Some(error) = error {
        event_parts.push(TranscriptAssistantPart::Error(error));
    }
    event_parts
}

fn insert_subagent_hint_after_task_tools(parts: &mut Vec<TranscriptAssistantPart>) {
    if parts
        .iter()
        .any(|part| matches!(part, TranscriptAssistantPart::SubagentHint))
    {
        return;
    }
    let has_task_tool = parts.iter().any(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if matches!(tool_call.header.tool_id.as_str(), "agent.spawn" | "task")
        )
    });
    if !has_task_tool {
        return;
    }
    let insert_at = parts
        .iter()
        .position(|part| matches!(part, TranscriptAssistantPart::Error(_)))
        .unwrap_or(parts.len());
    parts.insert(insert_at, TranscriptAssistantPart::SubagentHint);
}

fn sync_reasoning_parts_with_activity(
    parts: &mut Vec<TranscriptAssistantPart>,
    activity: &ActivityEntry,
    thinking_visible: bool,
) {
    if !thinking_visible {
        parts.retain(|part| !matches!(part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }

    if !activity_has_thinking_text(activity) {
        parts.retain(|part| !matches!(part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }

    let reasoning_indices = parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            matches!(part, TranscriptAssistantPart::Reasoning(_)).then_some(index)
        })
        .collect::<Vec<_>>();

    let Some(first_reasoning_index) = reasoning_indices.first().copied() else {
        return;
    };

    if reasoning_indices.len() == 1 {
        parts[first_reasoning_index] =
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: activity.thinking_text.clone(),
            });
        return;
    }

    let rendered = reasoning_indices
        .iter()
        .filter_map(|index| match parts.get(*index) {
            Some(TranscriptAssistantPart::Reasoning(reasoning)) => Some(reasoning.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if rendered == activity.thinking_text {
        return;
    }

    if let Some(remainder) = activity.thinking_text.strip_prefix(&rendered) {
        if remainder.is_empty() {
            return;
        }
        if let Some(last_reasoning_index) = reasoning_indices.last().copied() {
            if let Some(TranscriptAssistantPart::Reasoning(reasoning)) =
                parts.get_mut(last_reasoning_index)
            {
                reasoning.text.push_str(remainder);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct SequencedTranscriptAssistantPart {
    seq: u64,
    index: usize,
    part: TranscriptAssistantPart,
}

fn build_ordered_assistant_parts_from_events(
    activity: &ActivityEntry,
    app: &AppState,
    ordered_tool_calls: &[TranscriptOrderedToolCallSection],
    thinking_visible: bool,
) -> Vec<TranscriptAssistantPart> {
    let mut parts = Vec::new();
    let mut next_index = 0usize;
    let mut saw_turn_event = false;
    let mut saw_reasoning_event = false;
    let mut saw_body_event = false;
    let mut saw_tool_call = false;
    let mut pending_pre_tool_stream: Option<(u64, String)> = None;
    let mut pending_tool_calls = ordered_tool_calls
        .iter()
        .cloned()
        .map(|tool_call| (tool_call.tool_call_id.clone(), tool_call))
        .collect::<std::collections::BTreeMap<_, _>>();

    for event in app.events.iter().filter(|event| {
        event.seq >= activity.first_seq
            && event.seq <= activity.last_seq
            && turn_event_matches_activity(event, &activity.request_id)
    }) {
        match &event.payload {
            harness_core::event::EventV1::ProviderReasoningDelta(data)
                if provider_event_matches_activity(
                    event,
                    &data.request_id,
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                if thinking_visible {
                    saw_reasoning_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        TranscriptAssistantTextKind::Reasoning,
                        &data.delta,
                    );
                }
            }
            harness_core::event::EventV1::ProviderStreamDelta(data)
                if provider_event_matches_activity(
                    event,
                    &data.request_id,
                    &activity.request_id,
                ) =>
            {
                saw_turn_event = true;
                if saw_tool_call {
                    saw_body_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        TranscriptAssistantTextKind::Body,
                        &data.delta,
                    );
                } else {
                    let pending =
                        pending_pre_tool_stream.get_or_insert_with(|| (event.seq, String::new()));
                    pending.1.push_str(&data.delta);
                }
            }
            harness_core::event::EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(activity.request_id.as_str()) =>
            {
                saw_turn_event = true;
                flush_pending_pre_tool_stream(
                    &mut parts,
                    &mut next_index,
                    &mut pending_pre_tool_stream,
                    activity,
                    thinking_visible,
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                if crate::app::task_completed_updates_assistant_transcript(data)
                    && !saw_body_event
                    && has_trimmed_content(&data.result_summary)
                {
                    saw_body_event = true;
                    push_sequenced_text_part(
                        &mut parts,
                        &mut next_index,
                        event.seq,
                        TranscriptAssistantTextKind::Body,
                        &data.result_summary,
                    );
                }
            }
            harness_core::event::EventV1::ToolCallRequested(data)
                if event.correlation_id.as_deref() == Some(activity.request_id.as_str()) =>
            {
                saw_turn_event = true;
                saw_tool_call = true;
                flush_pending_pre_tool_stream(
                    &mut parts,
                    &mut next_index,
                    &mut pending_pre_tool_stream,
                    activity,
                    thinking_visible,
                    &mut saw_reasoning_event,
                    &mut saw_body_event,
                );
                if let Some(tool_call) = pending_tool_calls.remove(&data.tool_call_id) {
                    parts.push(SequencedTranscriptAssistantPart {
                        seq: event.seq,
                        index: next_index,
                        part: TranscriptAssistantPart::ToolCall(Box::new(tool_call.section)),
                    });
                    next_index += 1;
                }
            }
            _ => {}
        }
    }

    if !saw_turn_event {
        return Vec::new();
    }

    flush_pending_pre_tool_stream(
        &mut parts,
        &mut next_index,
        &mut pending_pre_tool_stream,
        activity,
        thinking_visible,
        &mut saw_reasoning_event,
        &mut saw_body_event,
    );

    if !saw_reasoning_event && !saw_body_event && activity_has_thinking_text(activity) {
        parts.push(SequencedTranscriptAssistantPart {
            seq: activity.first_seq,
            index: next_index,
            part: TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: activity.thinking_text.clone(),
            }),
        });
        next_index += 1;
    }

    if !saw_body_event && !activity.transcript_text.is_empty() {
        parts.push(SequencedTranscriptAssistantPart {
            seq: activity.last_seq,
            index: next_index,
            part: TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(
                activity.transcript_text.clone(),
            )),
        });
        next_index += 1;
    }

    for tool_call in pending_tool_calls.into_values() {
        parts.push(SequencedTranscriptAssistantPart {
            seq: tool_call.first_seq,
            index: next_index,
            part: TranscriptAssistantPart::ToolCall(Box::new(tool_call.section)),
        });
        next_index += 1;
    }

    parts.sort_by_key(|part| (part.seq, part.index));
    parts.into_iter().map(|part| part.part).collect()
}

fn flush_pending_pre_tool_stream(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    next_index: &mut usize,
    pending_pre_tool_stream: &mut Option<(u64, String)>,
    activity: &ActivityEntry,
    thinking_visible: bool,
    saw_reasoning_event: &mut bool,
    saw_body_event: &mut bool,
) {
    let Some((seq, text)) = pending_pre_tool_stream.take() else {
        return;
    };

    if text.is_empty() {
        return;
    }

    let treat_as_reasoning =
        thinking_visible && activity_has_thinking_text(activity) && activity.thinking_text == text;

    if treat_as_reasoning {
        *saw_reasoning_event = true;
        push_sequenced_text_part(
            parts,
            next_index,
            seq,
            TranscriptAssistantTextKind::Reasoning,
            &text,
        );
    } else {
        *saw_body_event = true;
        push_sequenced_text_part(
            parts,
            next_index,
            seq,
            TranscriptAssistantTextKind::Body,
            &text,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptAssistantTextKind {
    Reasoning,
    Body,
}

fn push_sequenced_text_part(
    parts: &mut Vec<SequencedTranscriptAssistantPart>,
    next_index: &mut usize,
    seq: u64,
    kind: TranscriptAssistantTextKind,
    text: &str,
) {
    if text.is_empty() {
        return;
    }

    if let Some(last) = parts.last_mut() {
        match (&mut last.part, kind) {
            (
                TranscriptAssistantPart::Reasoning(existing),
                TranscriptAssistantTextKind::Reasoning,
            ) => {
                existing.text.push_str(text);
                return;
            }
            (
                TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(existing)),
                TranscriptAssistantTextKind::Body,
            ) => {
                existing.push_str(text);
                return;
            }
            _ => {}
        }
    }

    let part = match kind {
        TranscriptAssistantTextKind::Reasoning => {
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: text.to_string(),
            })
        }
        TranscriptAssistantTextKind::Body => {
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text.to_string()))
        }
    };
    parts.push(SequencedTranscriptAssistantPart {
        seq,
        index: *next_index,
        part,
    });
    *next_index += 1;
}

#[expect(
    clippy::too_many_arguments,
    reason = "transcript tool-row assembly keeps the rendering toggles explicit at the call site"
)]
fn build_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    show_tool_details: bool,
    timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> Option<TranscriptToolCallSection> {
    if tool_hidden_from_transcript(tool_call) {
        return None;
    }

    if tool_call.status == ToolCallDisplayStatus::Succeeded
        && !show_tool_details
        && !tool_call_should_remain_visible_without_tool_details(tool_call)
    {
        return None;
    }

    let task_row = app.transcript_task_row_for_tool_call(tool_call);

    Some(build_transcript_tool_call_section(
        tool_call,
        app,
        task_row.as_ref(),
        timestamps_visible,
        show_generic_tool_output,
        tool_output_expanded,
        stacked_diffs,
        session_path,
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "tool-row assembly keeps transcript toggles and state inputs explicit at the call site"
)]
fn build_transcript_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    _timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> TranscriptToolCallSection {
    let struck_out = tool_call_denied(tool_call);
    let mut detail_blocks = Vec::new();
    let expanded = tool_output_expanded;
    let generic_output_visible = show_generic_tool_output || tool_output_expanded;
    let display_tool_id = tool_call.effective_tool_id();
    let child_session_id = task_tool_child_session_id(tool_call)
        .map(str::to_string)
        .or_else(|| {
            task_row
                .and_then(crate::app::OrchestrationTaskRow::effective_child_session_id)
                .map(str::to_string)
        });
    let error_subtitle = tool_error_subtitle(tool_call);
    let error_body = tool_error_text(tool_call);
    let question_answers = resolved_question_answer_items(tool_call);
    let todo_items = todo_items_from_tool_call(tool_call, session_path);
    let mut header_path_metadata = None;

    let animation_phase = app.transcript_animation_phase();

    let (title, icon, visual_style, uses_generic_output_visibility) = match display_tool_id {
        "fs.read" => {
            let path = tool_path_display(tool_call);
            let title = path.as_ref().map_or_else(
                || "Reading file...".to_string(),
                |path| format!("Read {path}{}", read_tool_input_suffix(tool_call)),
            );
            let icon = match tool_call.status {
                ToolCallDisplayStatus::Running => {
                    Some(transcript_streaming_spinner_frame(animation_phase))
                }
                ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued
                    if path.is_none() =>
                {
                    Some("~")
                }
                ToolCallDisplayStatus::PendingPermission
                | ToolCallDisplayStatus::Queued
                | ToolCallDisplayStatus::Succeeded
                | ToolCallDisplayStatus::Failed => Some("→"),
            };
            (title, icon, TranscriptToolCallVisualStyle::Inline, false)
        }
        "fs.glob" => (
            format!(
                "Glob \"{}\"{}{}",
                tool_summary_string(&tool_call.args_summary, &["pattern"])
                    .unwrap_or_else(|| "*".to_string()),
                tool_in_path_suffix(tool_call),
                tool_match_count_suffix(tool_call),
            ),
            Some("✱"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "fs.grep" => (
            format!(
                "Grep \"{}\"{}{}",
                tool_summary_string(&tool_call.args_summary, &["pattern"])
                    .unwrap_or_else(|| "pattern".to_string()),
                tool_in_path_suffix(tool_call),
                tool_match_count_suffix(tool_call),
            ),
            Some("✱"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "fs.ls" | "list" => (
            format!(
                "List {}",
                tool_summary_string(&tool_call.args_summary, &["path"])
                    .unwrap_or_else(|| ".".to_string()),
            ),
            Some("→"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "shell.run" | "bash" => {
            let cmd = shell_tool_command(tool_call).unwrap_or_else(|| "Shell".to_string());
            let shell_output = shell_tool_output(tool_call);
            if shell_output.is_some() {
                if let Some(output) = shell_output {
                    push_collapsible_bash_panel_block(
                        &mut detail_blocks,
                        &cmd,
                        &output,
                        shell_tool_title_description(tool_call, session_path),
                        HARNESS_BASH_OUTPUT_LINE_CLAMP,
                        expanded,
                        TranscriptToolCallDetailTone::Primary,
                    );
                }
                (
                    "Shell".to_string(),
                    None,
                    TranscriptToolCallVisualStyle::Block,
                    false,
                )
            } else {
                (
                    cmd.clone(),
                    Some("$"),
                    TranscriptToolCallVisualStyle::Inline,
                    false,
                )
            }
        }
        "edit.hashline_apply" => {
            let path = tool_call.edit_path_display();
            let title = match tool_call.edit.as_ref().map(|edit| edit.status) {
                Some(crate::app::EditDisplayStatus::Applied) => "Patch".to_string(),
                Some(crate::app::EditDisplayStatus::Rejected)
                | Some(crate::app::EditDisplayStatus::Proposed) => path
                    .as_ref()
                    .map(|_| "Edit".to_string())
                    .unwrap_or_else(|| "Preparing edit...".to_string()),
                None => path
                    .as_ref()
                    .map(|_| "Edit".to_string())
                    .unwrap_or_else(|| "Preparing edit...".to_string()),
            };

            if let Some(edit) = &tool_call.edit {
                if edit.status == crate::app::EditDisplayStatus::Applied {
                    push_tool_call_diff_blocks(
                        &mut detail_blocks,
                        tool_call,
                        app,
                        session_path,
                        stacked_diffs,
                    );
                }

                if edit.status == crate::app::EditDisplayStatus::Rejected {
                    if let Some(reason) = edit.rejection_reason.as_deref() {
                        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                            text: reason.to_string(),
                            tone: TranscriptToolCallDetailTone::Error,
                        });
                    }
                }
            }

            let icon = if title == "Preparing edit..." {
                Some("~")
            } else {
                Some("←")
            };

            (title, icon, TranscriptToolCallVisualStyle::Block, false)
        }
        "edit.hashline_scan" => (
            format!(
                "Scan {}",
                tool_path_display(tool_call).unwrap_or_else(|| "file".to_string())
            ),
            Some("→"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "agent.spawn" | "task" => {
            build_agent_spawn_tool_row(tool_call, task_row, &mut detail_blocks, animation_phase)
        }
        "background_output" => (
            background_output_tool_title(tool_call),
            Some("↻"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "fs.write" => {
            let rendered_diff = push_tool_call_diff_blocks(
                &mut detail_blocks,
                tool_call,
                app,
                session_path,
                stacked_diffs,
            );
            (
                "Write".to_string(),
                Some("←"),
                if rendered_diff {
                    TranscriptToolCallVisualStyle::Block
                } else {
                    TranscriptToolCallVisualStyle::Inline
                },
                false,
            )
        }
        "edit" => {
            let rendered_diff = push_tool_call_diff_blocks(
                &mut detail_blocks,
                tool_call,
                app,
                session_path,
                stacked_diffs,
            );
            let path = tool_path_display(tool_call);
            let preparing = !rendered_diff && path.is_none();
            let title = if preparing {
                "Preparing edit...".to_string()
            } else {
                "Edit".to_string()
            };
            let icon = if preparing { Some("~") } else { Some("←") };
            let visual_style = if rendered_diff {
                TranscriptToolCallVisualStyle::Block
            } else {
                TranscriptToolCallVisualStyle::Inline
            };
            (title, icon, visual_style, false)
        }
        "apply_patch" => {
            let rendered_diff = push_tool_call_diff_blocks(
                &mut detail_blocks,
                tool_call,
                app,
                session_path,
                stacked_diffs,
            );
            if rendered_diff {
                (
                    "Patch".to_string(),
                    Some("←"),
                    TranscriptToolCallVisualStyle::Block,
                    false,
                )
            } else {
                (
                    "Preparing patch...".to_string(),
                    Some("~"),
                    TranscriptToolCallVisualStyle::Inline,
                    false,
                )
            }
        }
        "web.fetch" => (
            format!(
                "WebFetch {}",
                tool_summary_string(&tool_call.args_summary, &["url"])
                    .unwrap_or_else(|| "url".to_string())
            ),
            Some("%"),
            TranscriptToolCallVisualStyle::Inline,
            true,
        ),
        "search.web" | "search.code" => (
            format!(
                "{} \"{}\"{}",
                if display_tool_id == "search.web" {
                    "Exa Web Search"
                } else {
                    "Exa Code Search"
                },
                tool_summary_string(&tool_call.args_summary, &["query"])
                    .unwrap_or_else(|| "query".to_string()),
                search_result_count_suffix(tool_call, display_tool_id)
            ),
            Some(if display_tool_id == "search.web" {
                "◈"
            } else {
                "◇"
            }),
            TranscriptToolCallVisualStyle::Inline,
            true,
        ),
        "todo.write" | "todowrite" => {
            if !todo_items.is_empty() {
                detail_blocks.push(TranscriptToolCallDetailBlock::TodoList {
                    items: todo_items.clone(),
                });
            }
            (
                todo_tool_title(&todo_items),
                None,
                TranscriptToolCallVisualStyle::Block,
                false,
            )
        }
        "todo.read" | "todoread" => (
            "Read todos".to_string(),
            Some("☑"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "skill.load" => (
            format!(
                "Load skill {}",
                tool_summary_string(&tool_call.args_summary, &["name"])
                    .unwrap_or_else(|| "skill".to_string())
            ),
            Some("✦"),
            TranscriptToolCallVisualStyle::Inline,
            false,
        ),
        "user.question" => {
            if question_answers.is_empty() {
                (
                    "Ask question".to_string(),
                    Some("?"),
                    TranscriptToolCallVisualStyle::Block,
                    false,
                )
            } else {
                push_question_answer_blocks(&mut detail_blocks, &question_answers);
                (
                    "Questions".to_string(),
                    None,
                    TranscriptToolCallVisualStyle::Block,
                    false,
                )
            }
        }
        "tool.batch" => (
            batch_tool_title(tool_call),
            Some("≋"),
            generic_tool_visual_style(tool_call, generic_output_visible),
            true,
        ),
        "code.lsp" => (
            generic_tool_title(tool_call, display_tool_id),
            Some("⌘"),
            generic_tool_visual_style(tool_call, generic_output_visible),
            true,
        ),
        _ if is_mcp_tool_id(display_tool_id) => (
            mcp_tool_title(tool_call, display_tool_id),
            Some("⚙"),
            generic_tool_visual_style(tool_call, generic_output_visible),
            true,
        ),
        _ => {
            let title = generic_tool_title(tool_call, display_tool_id);
            let generic_output = if tool_call.status == ToolCallDisplayStatus::Failed {
                error_body
                    .as_deref()
                    .or(tool_call.output_summary.as_deref())
            } else {
                tool_call.output_summary.as_deref()
            };
            if generic_output_visible && generic_output.is_some() {
                push_collapsible_output_block(
                    &mut detail_blocks,
                    generic_output.unwrap_or_default(),
                    3,
                    expanded,
                    if tool_call.status == ToolCallDisplayStatus::Failed {
                        TranscriptToolCallDetailTone::Error
                    } else {
                        TranscriptToolCallDetailTone::Primary
                    },
                );
                (title, None, TranscriptToolCallVisualStyle::Block, true)
            } else {
                (
                    title,
                    Some("⚙"),
                    generic_tool_visual_style(tool_call, generic_output_visible),
                    true,
                )
            }
        }
    };

    if matches!(tool_call.tool_id.as_str(), "fs.read" | "read")
        && tool_call.status == ToolCallDisplayStatus::Succeeded
        && detail_blocks.is_empty()
    {
        if let Some(path) = tool_path_display(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: format!("↳ Loaded {path}"),
                tone: TranscriptToolCallDetailTone::Secondary,
            });
        }
    }

    if detail_blocks.is_empty()
        && uses_generic_output_visibility
        && if tool_call.status == ToolCallDisplayStatus::Failed {
            error_body.is_some() || tool_call.output_summary.is_some()
        } else {
            tool_call.output_summary.is_some()
        }
        && generic_output_visible
    {
        push_collapsible_output_block(
            &mut detail_blocks,
            if tool_call.status == ToolCallDisplayStatus::Failed {
                error_body
                    .as_deref()
                    .or(tool_call.output_summary.as_deref())
            } else {
                tool_call.output_summary.as_deref()
            }
            .unwrap_or_default(),
            3,
            expanded,
            if tool_call.status == ToolCallDisplayStatus::Failed {
                TranscriptToolCallDetailTone::Error
            } else {
                TranscriptToolCallDetailTone::Primary
            },
        );
    }

    push_tool_identity_block(&mut detail_blocks, tool_call);
    push_failed_tool_error_block(&mut detail_blocks, tool_call);

    let disclosure_state = if matches!(display_tool_id, "agent.spawn" | "task")
        || uses_generic_output_visibility
            && tool_call.status == ToolCallDisplayStatus::Succeeded
            && !generic_output_visible
    {
        None
    } else {
        tool_disclosure_state(tool_call, tool_output_expanded)
    };
    let default_subtitle = match display_tool_id {
        "shell.run" | "bash" => shell_tool_subtitle(&tool_call.args_summary),
        "edit.hashline_apply" => tool_call_path_metadata(tool_call.edit_path_display().as_deref())
            .map(|metadata| {
                header_path_metadata = metadata.parent.clone();
                metadata.leaf
            }),
        "fs.write" | "edit" => tool_call_path_metadata(
            tool_summary_string(&tool_call.args_summary, &["filePath", "path"]).as_deref(),
        )
        .map(|metadata| {
            header_path_metadata = metadata.parent.clone();
            metadata.leaf
        }),
        "background_output" => background_output_tool_subtitle(tool_call),
        "apply_patch" => apply_patch_tool_header_metadata(tool_call).map(|metadata| {
            header_path_metadata = metadata.parent.clone();
            metadata.leaf
        }),
        _ => None,
    };

    TranscriptToolCallSection {
        tool_call_id: tool_call.tool_call_id.clone(),
        child_session_id,
        hovered_target: app.hovered_transcript_target().cloned(),
        header: TranscriptToolCallHeader {
            tool_id: if matches!(display_tool_id, "shell.run" | "bash") {
                display_tool_id.to_string()
            } else {
                tool_call.tool_id.clone()
            },
            title,
            subtitle: if matches!(display_tool_id, "shell.run" | "bash")
                && !tool_call_denied(tool_call)
            {
                default_subtitle
            } else if tool_call.status == ToolCallDisplayStatus::Failed {
                join_tool_subtitles(default_subtitle, error_subtitle)
            } else if display_tool_id == "user.question" {
                question_tool_subtitle(&question_answers)
            } else {
                default_subtitle
            },
            path_metadata: header_path_metadata,
            icon,
            status: tool_call.status,
            visual_style,
            struck_out,
            disclosure_state,
        },
        detail_blocks,
        expanded,
    }
}

fn push_question_answer_blocks(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    question_answers: &[TranscriptQuestionAnswerItem],
) {
    for item in question_answers {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: item.question.clone(),
            tone: TranscriptToolCallDetailTone::Primary,
        });
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: format!("↳ {}", item.answer),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
}

fn push_applied_edit_fallback_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    edit: &crate::app::EditEntry,
) {
    let summary = edit.summary.as_deref().map(collapse_inline_whitespace);
    let diff_rel_path = edit
        .diff_rel_path
        .as_deref()
        .map(collapse_inline_whitespace);
    let text = match (summary.as_deref(), diff_rel_path.as_deref()) {
        (Some(summary), Some(diff_rel_path)) => {
            format!("{summary} · Diff preview unavailable ({diff_rel_path})")
        }
        (Some(summary), None) => format!("{summary} · Diff preview unavailable"),
        (None, Some(diff_rel_path)) => format!("Diff preview unavailable · {diff_rel_path}"),
        (None, None) => "Diff preview unavailable".to_string(),
    };
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text,
        tone: TranscriptToolCallDetailTone::Secondary,
    });
}

fn push_tool_call_diff_blocks(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    session_path: Option<&Path>,
    stacked_diffs: bool,
) -> bool {
    let Some(session_path) = session_path else {
        if let Some(edit) = tool_call.edit.as_ref() {
            if edit.status == crate::app::EditDisplayStatus::Applied {
                push_applied_edit_fallback_block(detail_blocks, edit);
                return true;
            }
        }
        return false;
    };

    if tool_call.effective_tool_id() == "apply_patch" {
        let file_entries = collect_apply_patch_file_render_entries(tool_call);
        if file_entries.len() > 1 {
            return push_apply_patch_file_sections(
                detail_blocks,
                tool_call,
                app,
                session_path,
                stacked_diffs,
                &file_entries,
            );
        }
    }

    let diff_artifacts = tool_call_diff_artifacts(tool_call);
    let show_file_header = tool_call.edit.is_none() || diff_artifacts.len() > 1;
    let mut rendered = false;
    for (diff_rel_path, fallback_path) in diff_artifacts {
        rendered |= push_structured_diff_artifact_block(
            detail_blocks,
            session_path,
            &diff_rel_path,
            fallback_path.as_deref(),
            stacked_diffs,
            show_file_header,
        );
    }

    if !rendered {
        if let Some((diff_content, fallback_path)) = tool_call_inline_diff_block(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                fallback_path,
                force_stacked: stacked_diffs,
                show_file_header: tool_call.effective_tool_id() != "edit",
            });
            return true;
        }

        if tool_call.effective_tool_id() == "apply_patch" {
            let file_entries = collect_apply_patch_file_render_entries(tool_call);
            if file_entries.len() > 1 {
                return push_apply_patch_file_sections(
                    detail_blocks,
                    tool_call,
                    app,
                    session_path,
                    stacked_diffs,
                    &file_entries,
                );
            }
            if let Some(rows) = tool_call_apply_patch_file_rows(tool_call) {
                for row in rows {
                    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                        text: row,
                        tone: TranscriptToolCallDetailTone::Secondary,
                    });
                }
                return true;
            }
        }

        if let Some(edit) = tool_call.edit.as_ref() {
            if edit.status == crate::app::EditDisplayStatus::Applied {
                push_applied_edit_fallback_block(detail_blocks, edit);
                return true;
            }
        }
        if tool_call_has_diff_preview(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: "Diff preview unavailable".to_string(),
                tone: TranscriptToolCallDetailTone::Secondary,
            });
            return true;
        }
    }

    rendered
}

fn push_apply_patch_file_sections(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    app: &AppState,
    session_path: &Path,
    stacked_diffs: bool,
    file_entries: &[ApplyPatchFileRenderEntry],
) -> bool {
    if file_entries.is_empty() {
        return false;
    }

    for entry in file_entries {
        let mut file_detail_blocks = Vec::new();
        if let Some(diff_rel_path) = entry.diff_rel_path.as_deref() {
            let _ = push_structured_diff_artifact_block(
                &mut file_detail_blocks,
                session_path,
                diff_rel_path,
                Some(&entry.file_path),
                stacked_diffs,
                false,
            );
        }
        let metadata =
            tool_call_path_metadata(Some(&entry.file_path)).unwrap_or(TranscriptPathMetadata {
                leaf: entry.file_path.clone(),
                parent: None,
            });
        detail_blocks.push(TranscriptToolCallDetailBlock::FileSection(
            TranscriptToolCallFileSection {
                tool_call_id: tool_call.tool_call_id.clone(),
                file_path: entry.file_path.clone(),
                title: metadata.leaf,
                subtitle: metadata.parent,
                disclosure_state: if app
                    .patch_file_output_expanded(&tool_call.tool_call_id, &entry.file_path)
                {
                    TranscriptToolCallDisclosureState::Expanded
                } else {
                    TranscriptToolCallDisclosureState::Collapsed
                },
                detail_blocks: file_detail_blocks,
            },
        ));
    }

    true
}

fn push_structured_diff_artifact_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    session_path: &Path,
    diff_rel_path: &str,
    fallback_path: Option<&str>,
    stacked_diffs: bool,
    show_file_header: bool,
) -> bool {
    let Ok(diff_content) = std::fs::read_to_string(session_path.join(diff_rel_path)) else {
        return false;
    };
    detail_blocks.push(TranscriptToolCallDetailBlock::StructuredDiff {
        diff_content,
        fallback_path: fallback_path.map(str::to_string),
        force_stacked: stacked_diffs,
        show_file_header,
    });
    true
}

fn build_agent_spawn_tool_row(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    animation_phase: usize,
) -> (
    String,
    Option<&'static str>,
    TranscriptToolCallVisualStyle,
    bool,
) {
    let description = agent_spawn_description(tool_call).or_else(|| {
        task_row
            .and_then(|row| row.result_summary.as_deref())
            .map(collapse_inline_whitespace)
            .filter(|value| !value.is_empty())
    });
    let has_description = description.is_some();
    let title = if has_description {
        agent_spawn_title(tool_call, description)
    } else {
        "Delegating...".to_string()
    };
    if matches!(
        tool_call.status,
        ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Running
    ) {
        if let Some(line) = agent_spawn_context_line(tool_call, task_row) {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: line,
                tone: TranscriptToolCallDetailTone::Secondary,
            });
        }
    }
    let icon = match tool_call.status {
        ToolCallDisplayStatus::Running if has_description => {
            Some(transcript_streaming_spinner_frame(animation_phase))
        }
        _ if has_description => Some("│"),
        ToolCallDisplayStatus::PendingPermission
        | ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::Succeeded
        | ToolCallDisplayStatus::Failed => Some("~"),
    };
    (
        title,
        icon,
        TranscriptToolCallVisualStyle::TaskInline,
        false,
    )
}

fn push_tool_identity_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    if matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
        || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
    {
        return;
    }

    let Some(alias_source) = tool_call.resolved_alias_source_tool_id() else {
        return;
    };
    let effective = tool_call.effective_tool_id();
    if !tool_call.is_compat_alias() || alias_source == effective {
        return;
    }
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: format!("Compat alias · {alias_source} → {effective}"),
        tone: TranscriptToolCallDetailTone::Secondary,
    });
}

fn push_collapsible_output_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    output: &str,
    max_lines: usize,
    expanded: bool,
    tone: TranscriptToolCallDetailTone,
) {
    let preview = collapsible_output_preview(output, max_lines, expanded);
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: preview.output,
        tone,
    });
    if let Some(hint) = preview.expand_hint {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: hint.to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
}

fn push_collapsible_bash_panel_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    command: &str,
    output: &str,
    description: Option<String>,
    max_lines: usize,
    expanded: bool,
    tone: TranscriptToolCallDetailTone,
) {
    let preview = collapsible_bash_panel_preview(output, max_lines, expanded);

    detail_blocks.push(TranscriptToolCallDetailBlock::BashPanel {
        command: command.to_string(),
        output: preview.output,
        description,
        expand_hint: preview.expand_hint.map(str::to_string),
        tone,
    });
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
    let max_scroll = app.last_transcript_max_scroll.get();
    if max_scroll == 0 {
        return None;
    }

    let viewport = transcript_viewport_layout(context.inner_area, true);
    let scroll_top =
        current_transcript_scroll_top(app.follow_mode, app.transcript_scroll, max_scroll);
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
                    app.follow_mode,
                    app.transcript_scroll,
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

fn build_transcript_render_surfaces(
    section: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    build_turn_render_surfaces(section, theme, width, base_surface)
}

fn build_turn_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    let mut surfaces = Vec::with_capacity(3);
    if let Some(user_message) = turn.user_message.as_ref() {
        surfaces.push(build_user_render_surface(
            turn,
            user_message,
            theme,
            width,
            base_surface,
        ));
    }
    surfaces.extend(build_assistant_render_surfaces(
        turn,
        theme,
        width,
        base_surface,
    ));
    surfaces
}

fn build_user_render_surface(
    turn: &TranscriptTurnSection,
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> TranscriptRenderSurface {
    let surface = transcript_emphasized_surface(theme, base_surface);
    let render_width = transcript_surface_render_width(width, TranscriptRenderSurfaceKind::User);
    let content_width = transcript_surface_content_width(render_width, true);
    let mut lines = vec![user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        Style::default().fg(theme.text.primary),
        surface,
    )];
    append_user_surface_text_block(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        TRANSCRIPT_USER_BODY_PREFIX,
        content_width,
        surface,
    );
    if user_msg.queued {
        let agent_accent = theme.agent_accent(&turn.header.profile_label);
        lines.push(user_surface_line(
            TRANSCRIPT_USER_BODY_PREFIX,
            vec![Span::styled(
                " QUEUED ".to_string(),
                Style::default()
                    .fg(selected_foreground_for_badge(agent_accent, theme))
                    .bg(agent_accent)
                    .add_modifier(Modifier::BOLD),
            )],
            Style::default().fg(theme.text.secondary),
            surface,
        ));
    }
    lines.push(user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        Style::default().fg(theme.text.primary),
        surface,
    ));

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::User,
        show_outer_rail: true,
        rail_color: theme.agent_accent(&turn.header.profile_label),
        surface,
        lines,
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
    }
}

fn build_assistant_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    let agent_accent = theme.agent_accent(&turn.header.profile_label);
    let (assistant_icon, assistant_color, assistant_status) = match turn.header.status {
        ActivityStatus::Queued => ("◇", agent_accent, "queued"),
        ActivityStatus::Streaming => (
            transcript_streaming_spinner_frame(turn.animation_phase),
            agent_accent,
            "active",
        ),
        ActivityStatus::Done => ("▪", agent_accent, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };

    let mut surfaces = Vec::new();
    let footer_target = assistant_footer_target_index(turn);
    let mut index = 0;

    while index < turn.assistant_parts.len() {
        let part = &turn.assistant_parts[index];
        let previous = index
            .checked_sub(1)
            .and_then(|prev| turn.assistant_parts.get(prev));
        let group_len = match part {
            TranscriptAssistantPart::ToolCall(_) => {
                context_tool_group_len(&turn.assistant_parts[index..])
            }
            _ => 0,
        };

        if group_len > 1
            && context_tool_group_complete(&turn.assistant_parts[index..index + group_len])
        {
            let tool_calls = turn.assistant_parts[index..index + group_len]
                .iter()
                .filter_map(|part| match part {
                    TranscriptAssistantPart::ToolCall(tool_call) => Some(tool_call.as_ref()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            surfaces.push(build_context_tool_group_render_surface(
                turn,
                &tool_calls,
                theme,
                width,
                base_surface,
                footer_target
                    .map(|target| target >= index && target < index + group_len)
                    .unwrap_or(false),
                assistant_icon,
                assistant_color,
                assistant_status,
            ));
            index += group_len;
            continue;
        }

        let append_footer = footer_target == Some(index);
        surfaces.push(build_assistant_part_render_surface(
            turn,
            part,
            theme,
            width,
            base_surface,
            assistant_part_needs_leading_gap(previous, part),
            append_footer,
            assistant_icon,
            assistant_color,
            assistant_status,
        ));
        index += 1;
    }

    if footer_target == Some(turn.assistant_parts.len()) {
        surfaces.push(build_footer_only_render_surface(
            turn,
            theme,
            base_surface,
            assistant_icon,
            assistant_color,
            assistant_status,
        ));
    }
    surfaces
}

fn assistant_footer_target_index(turn: &TranscriptTurnSection) -> Option<usize> {
    if !turn.show_footer {
        return None;
    }
    if turn.assistant_parts.is_empty() {
        return (turn.user_message.is_some()
            || activity_status_supports_footer_only(turn.header.status))
        .then_some(0);
    }

    Some(turn.assistant_parts.len() - 1)
}

fn assistant_part_needs_leading_gap(
    previous: Option<&TranscriptAssistantPart>,
    current: &TranscriptAssistantPart,
) -> bool {
    matches!(
        (previous, current),
        (
            Some(TranscriptAssistantPart::Reasoning(_)),
            TranscriptAssistantPart::Body(_)
        ) | (
            Some(TranscriptAssistantPart::ToolCall(_)),
            TranscriptAssistantPart::Reasoning(_)
        )
    )
}

fn context_tool_group_len(parts: &[TranscriptAssistantPart]) -> usize {
    parts
        .iter()
        .take_while(|part| {
            matches!(
                part,
                TranscriptAssistantPart::ToolCall(tool_call)
                    if context_group_tool_id(&tool_call.header.tool_id)
            )
        })
        .count()
}

fn context_tool_group_complete(parts: &[TranscriptAssistantPart]) -> bool {
    parts.iter().all(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if tool_call.header.status == ToolCallDisplayStatus::Succeeded
        )
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "assistant turn surface assembly needs the footer styling inputs in one place"
)]
fn build_assistant_part_render_surface(
    turn: &TranscriptTurnSection,
    part: &TranscriptAssistantPart,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    prepend_gap: bool,
    append_footer: bool,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    let mut lines = Vec::new();
    let (
        kind,
        show_outer_rail,
        rail_color,
        surface,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
    ) = match part {
        TranscriptAssistantPart::Reasoning(thinking) => {
            if prepend_gap {
                lines.push(Line::default());
            }
            append_reasoning_block(
                &mut lines,
                thinking,
                theme,
                transcript_surface_content_width(width, true),
            );
            (
                TranscriptRenderSurfaceKind::AssistantReasoning,
                true,
                theme.border.subtle,
                base_surface,
                None,
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Body(block) => {
            if prepend_gap {
                lines.push(Line::default());
            }
            let selection_rows = match block {
                TranscriptBodyBlock::RichText(text) if !text.contains("```") => {
                    Some(selection_rows_for_markdownish_text_block(
                        text,
                        theme.text.primary,
                        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                        theme,
                        transcript_surface_content_width(width, false),
                    ))
                }
                _ => None,
            };
            let TranscriptBodyBlock::RichText(text) = block;
            append_rich_text_block(
                &mut lines,
                text,
                theme.text.primary,
                TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                theme,
                transcript_surface_content_width(width, false),
            );
            (
                TranscriptRenderSurfaceKind::AssistantBody,
                false,
                assistant_primary_rail_color(turn.header.status, &turn.header.profile_label, theme),
                base_surface,
                None,
                selection_rows,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::ToolCall(tool_call) => {
            let kind = if shell_tool_uses_harness_bash_card(tool_call) {
                TranscriptRenderSurfaceKind::AssistantCommandTool
            } else {
                TranscriptRenderSurfaceKind::AssistantTool
            };
            let render_width = transcript_surface_render_width(width, kind);
            let render =
                append_tool_call_section_lines(tool_call, theme, render_width, base_surface);
            lines = render.lines;
            (
                kind,
                false,
                transcript_nested_rail_color(theme),
                base_surface,
                Some(render.interaction_rows),
                None,
                render.diff_hunk_offsets,
            )
        }
        TranscriptAssistantPart::SubagentHint => {
            let hint_line_count =
                append_subagent_hint_line(&mut lines, turn, theme, width, base_surface);
            (
                TranscriptRenderSurfaceKind::AssistantTool,
                false,
                transcript_nested_rail_color(theme),
                base_surface,
                Some(vec![
                    Some(full_width_interaction_row(
                        TranscriptMouseTarget::FirstSubagentSession
                    ));
                    hint_line_count
                ]),
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Error(error) => {
            append_assistant_error_box(&mut lines, &error.text, theme, width, base_surface);
            (
                TranscriptRenderSurfaceKind::AssistantError,
                false,
                theme.status.error,
                base_surface,
                None,
                None,
                Vec::new(),
            )
        }
    };

    let mut interaction_rows = interaction_rows;
    let mut selection_rows = selection_rows;

    if let Some(rows) = interaction_rows.as_mut() {
        if prepend_gap {
            rows.insert(0, None);
        }
    }

    if let Some(rows) = selection_rows.as_mut() {
        if prepend_gap {
            rows.insert(0, blank_selection_row(width));
        }
    }

    if append_footer {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty()) {
            lines.push(Line::default());
            if let Some(rows) = interaction_rows.as_mut() {
                rows.push(None);
            }
            if let Some(rows) = selection_rows.as_mut() {
                rows.push(blank_selection_row(width));
            }
        }
        let footer_line = build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
        );
        if let Some(rows) = selection_rows.as_mut() {
            rows.extend(selection_rows_for_rendered_line(&footer_line, width));
        }
        if let Some(rows) = interaction_rows.as_mut() {
            rows.push(None);
        }
        lines.push(footer_line);
    }

    TranscriptRenderSurface {
        kind,
        show_outer_rail,
        rail_color,
        surface,
        lines,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
    }
}

fn append_subagent_hint_line(
    lines: &mut Vec<Line<'static>>,
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> usize {
    let start = lines.len();
    let keybind = turn.subagent_hint_key.as_str();
    let spans = vec![
        Span::styled(keybind.to_string(), Style::default().fg(theme.text.primary)),
        Span::styled(" view subagents", muted_meta_style(theme)),
    ];
    append_surface_row(
        lines,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        base_surface,
        spans,
        transcript_surface_content_width(width, false),
    );
    lines.len().saturating_sub(start)
}

fn build_footer_only_render_surface(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    base_surface: Color,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::AssistantFooter,
        show_outer_rail: false,
        rail_color: assistant_primary_rail_color(
            turn.header.status,
            &turn.header.profile_label,
            theme,
        ),
        surface: base_surface,
        lines: vec![build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
        )],
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
    }
}

fn append_reasoning_block(
    lines: &mut Vec<Line<'static>>,
    thinking: &TranscriptLabeledTextSection,
    theme: &Theme,
    width: u16,
) {
    let label_style = Style::default()
        .fg(theme.text.secondary)
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::ITALIC);
    let reasoning_style = Style::default()
        .fg(theme.text.secondary)
        .add_modifier(Modifier::DIM);
    let mut rendered_any_line = false;

    for (index, row) in thinking.text.lines().enumerate() {
        let mut spans = Vec::new();
        if index == 0 {
            spans.push(Span::styled(thinking.label.to_string(), label_style));
            if !row.is_empty() {
                spans.push(Span::styled(" ".to_string(), reasoning_style));
            }
        }
        if !row.is_empty() {
            spans.extend(parse_inline_markdown_spans(
                row,
                reasoning_style,
                theme.text.secondary,
                theme,
            ));
        }
        append_prefixed_wrapped_spans_line(
            lines,
            TRANSCRIPT_REASONING_BODY_PREFIX,
            reasoning_style,
            spans,
            width,
        );
        rendered_any_line = true;
    }

    if !rendered_any_line {
        append_prefixed_wrapped_spans_line(
            lines,
            TRANSCRIPT_REASONING_BODY_PREFIX,
            reasoning_style,
            vec![Span::styled(thinking.label.to_string(), label_style)],
            width,
        );
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "context tool grouping needs the same surface/footer inputs as other transcript builders"
)]
fn build_context_tool_group_render_surface(
    turn: &TranscriptTurnSection,
    tool_calls: &[&TranscriptToolCallSection],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    append_footer: bool,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    let surface = base_surface;
    let mut lines = Vec::new();
    let group_expanded = tool_calls.iter().any(|tool_call| tool_call.expanded);
    let group_disclosure = if tool_calls.is_empty() {
        None
    } else if group_expanded {
        Some(TranscriptToolCallDisclosureState::Expanded)
    } else {
        Some(TranscriptToolCallDisclosureState::Collapsed)
    };
    let (reads, searches, lists, busy) = tool_calls.iter().fold(
        (0usize, 0usize, 0usize, false),
        |(reads, searches, lists, busy), tool_call| {
            let (reads, searches, lists) = match tool_call.header.tool_id.as_str() {
                "fs.read" | "read" => (reads + 1, searches, lists),
                "fs.glob" | "glob" | "fs.grep" | "grep" => (reads, searches + 1, lists),
                "fs.ls" | "list" => (reads, searches, lists + 1),
                _ => (reads, searches, lists),
            };
            (
                reads,
                searches,
                lists,
                busy || !matches!(tool_call.header.status, ToolCallDisplayStatus::Succeeded),
            )
        },
    );

    let mut summary = vec![Span::styled(
        if busy {
            "Gathering context".to_string()
        } else {
            "Gathered context".to_string()
        },
        Style::default().fg(theme.text.primary),
    )];
    let mut counts = Vec::new();
    if reads > 0 {
        counts.push(format!("{reads} read{}", if reads == 1 { "" } else { "s" }));
    }
    if searches > 0 {
        counts.push(format!(
            "{searches} search{}",
            if searches == 1 { "" } else { "es" }
        ));
    }
    if lists > 0 {
        counts.push(format!("{lists} list{}", if lists == 1 { "" } else { "s" }));
    }
    if !counts.is_empty() {
        summary.push(Span::styled(" · ", muted_meta_style(theme)));
        summary.push(Span::styled(counts.join(" · "), muted_meta_style(theme)));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(group_disclosure) {
        summary.push(Span::styled("  ", muted_meta_style(theme)));
        summary.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    append_surface_row(
        &mut lines,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        surface,
        summary,
        transcript_surface_content_width(width, false),
    );

    if group_expanded {
        let detail_prefix = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  ");
        for tool_call in tool_calls {
            let mut spans = vec![Span::styled(
                tool_call.header.title.clone(),
                Style::default().fg(theme.text.primary),
            )];
            if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
                spans.push(Span::styled(" · ", muted_meta_style(theme)));
                spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
            }
            append_surface_row(
                &mut lines,
                &detail_prefix,
                surface,
                spans,
                transcript_surface_content_width(width, false),
            );
        }
    }

    if append_footer {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty()) {
            lines.push(Line::default());
        }
        lines.push(build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
        ));
    }

    let mut interaction_rows = vec![None; lines.len()];
    if !interaction_rows.is_empty() {
        interaction_rows[0] = Some(full_width_interaction_row(
            TranscriptMouseTarget::ToolGroup {
                tool_call_ids: tool_calls
                    .iter()
                    .map(|tool_call| tool_call.tool_call_id.clone())
                    .collect(),
            },
        ));
    }

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::AssistantTool,
        show_outer_rail: false,
        rail_color: transcript_nested_rail_color(theme),
        surface,
        lines,
        interaction_rows: Some(interaction_rows),
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
    }
}

fn append_tool_call_section_lines(
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> ToolSectionRender {
    let mut render = ToolSectionRender {
        lines: Vec::new(),
        interaction_rows: Vec::new(),
        diff_hunk_offsets: Vec::new(),
    };
    match tool_call.header.visual_style {
        TranscriptToolCallVisualStyle::Inline => {
            append_inline_tool_section_lines(&mut render, tool_call, theme, width, base_surface)
        }
        TranscriptToolCallVisualStyle::TaskInline => append_task_inline_tool_section_lines(
            &mut render,
            tool_call,
            theme,
            width,
            base_surface,
        ),
        TranscriptToolCallVisualStyle::Block => {
            append_block_tool_section_lines(&mut render, tool_call, theme, width, base_surface)
        }
    }
    render
}

fn append_inline_tool_section_lines(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let fg = inline_tool_color(tool_call.header.status, theme);
    let style = tool_call_header_style(tool_call.header.struck_out, fg);

    let mut spans = Vec::new();
    if let Some(icon) = tool_call.header.icon {
        spans.push(Span::styled(format!("{icon} "), style));
    }
    spans.push(Span::styled(tool_call.header.title.clone(), style));
    if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(tool_call.header.disclosure_state) {
        spans.push(Span::styled("  ", muted_meta_style(theme)));
        spans.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    append_surface_row_with_target(
        &mut render.lines,
        &mut render.interaction_rows,
        tool_header_target(
            &tool_call.tool_call_id,
            tool_call.header.disclosure_state.is_some(),
        ),
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        base_surface,
        spans,
        transcript_surface_content_width(width, false),
    );

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, None);
}

fn append_task_inline_tool_section_lines(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let target = subagent_session_target(tool_call.child_session_id.as_deref()).or_else(|| {
        Some(TranscriptMouseTarget::Tool {
            tool_call_id: tool_call.tool_call_id.clone(),
        })
    });
    let hovered = transcript_target_is_hovered(target.as_ref(), tool_call.hovered_target.as_ref());
    let fg = task_inline_tool_color(tool_call.header.status, theme, hovered && target.is_some());
    let style = tool_call_header_style(tool_call.header.struck_out, fg);
    let surface = base_surface;
    let mut spans = Vec::new();
    if let Some(icon) = tool_call.header.icon {
        spans.push(Span::styled(icon.to_string(), style));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(tool_call.header.title.clone(), style));

    append_surface_row_with_bounded_target(
        &mut render.lines,
        &mut render.interaction_rows,
        target.clone(),
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        surface,
        spans,
        transcript_surface_content_width(width, false),
    );

    for detail_block in &tool_call.detail_blocks {
        match detail_block {
            TranscriptToolCallDetailBlock::Message { text, tone } => {
                let detail_style = match tone {
                    TranscriptToolCallDetailTone::Error => Style::default().fg(theme.status.error),
                    TranscriptToolCallDetailTone::Primary
                    | TranscriptToolCallDetailTone::Secondary => style,
                };
                for row in text.split('\n') {
                    let spans = if row.is_empty() {
                        Vec::new()
                    } else {
                        vec![Span::styled(row.to_string(), detail_style)]
                    };
                    append_surface_row_with_bounded_target(
                        &mut render.lines,
                        &mut render.interaction_rows,
                        target.clone(),
                        TRANSCRIPT_OPCODE_EDIT_INDENT,
                        surface,
                        spans,
                        transcript_surface_content_width(width, false),
                    );
                }
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
                        tool_call_id: tool_call.tool_call_id.clone(),
                        child_session_id: tool_call.child_session_id.clone(),
                        hovered_target: tool_call.hovered_target.clone(),
                        header: tool_call.header.clone(),
                        detail_blocks: vec![detail_block.clone()],
                        expanded: tool_call.expanded,
                    },
                    theme,
                    width,
                    base_surface,
                    None,
                );
            }
        }
    }
}

fn append_block_tool_section_lines(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    if shell_tool_uses_harness_bash_card(tool_call) {
        append_shell_tool_harness_card(render, tool_call, theme, width);
        return;
    }

    let surface = base_surface;
    let card_shell = (tool_call.header.status == ToolCallDisplayStatus::Failed).then_some(
        TranscriptToolCardShell {
            indent: TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            rail_color: block_tool_rail_color(tool_call.header.status, theme),
            surface,
        },
    );
    let title_style = tool_call_header_style(
        tool_call.header.struck_out,
        block_tool_color(tool_call.header.status, theme),
    );
    let header_target = tool_header_target(
        &tool_call.tool_call_id,
        tool_call.header.disclosure_state.is_some(),
    );

    let mut title_spans = Vec::new();
    if let Some(icon) = tool_call.header.icon {
        title_spans.push(Span::styled(format!("{icon} "), title_style));
    }
    title_spans.push(Span::styled(tool_call.header.title.clone(), title_style));
    if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
        title_spans.push(Span::styled(" · ", muted_meta_style(theme)));
        title_spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(tool_call.header.disclosure_state) {
        title_spans.push(Span::styled("  ", muted_meta_style(theme)));
        title_spans.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    if let Some(shell) = card_shell {
        append_nested_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target.clone(),
            NestedSurfaceChrome {
                indent: shell.indent,
                rail_color: shell.rail_color,
                surface: shell.surface,
            },
            title_spans,
            transcript_surface_content_width(width, false),
        );
    } else {
        append_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target.clone(),
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            surface,
            title_spans,
            transcript_surface_content_width(width, false),
        );
    }

    if let Some(path_metadata) = tool_call.header.path_metadata.as_deref() {
        let path_spans = vec![Span::styled(
            path_metadata.to_string(),
            muted_meta_style(theme),
        )];
        if let Some(shell) = card_shell {
            append_nested_surface_row_with_target(
                &mut render.lines,
                &mut render.interaction_rows,
                header_target,
                NestedSurfaceChrome {
                    indent: shell.indent,
                    rail_color: shell.rail_color,
                    surface: shell.surface,
                },
                path_spans,
                transcript_surface_content_width(width, false),
            );
        } else {
            append_surface_row_with_target(
                &mut render.lines,
                &mut render.interaction_rows,
                header_target,
                TRANSCRIPT_OPCODE_EDIT_INDENT,
                surface,
                path_spans,
                transcript_surface_content_width(width, false),
            );
        }
    }

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, card_shell);
}

fn shell_tool_uses_harness_bash_card(tool_call: &TranscriptToolCallSection) -> bool {
    matches!(tool_call.header.tool_id.as_str(), "shell.run" | "bash")
        && tool_call.detail_blocks.iter().any(|detail_block| {
            matches!(
                detail_block,
                TranscriptToolCallDetailBlock::BashPanel { .. }
            )
        })
}

fn append_shell_tool_harness_card(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
) {
    for detail_block in &tool_call.detail_blocks {
        let start = render.lines.len();
        match detail_block {
            TranscriptToolCallDetailBlock::BashPanel {
                command,
                output,
                description,
                expand_hint,
                tone,
            } => {
                append_harness_bash_panel(
                    &mut render.lines,
                    HarnessBashPanel {
                        command,
                        output,
                        description: description.as_deref(),
                        expand_hint: expand_hint.as_deref(),
                        tone: *tone,
                    },
                    theme,
                    width,
                );
                let target = tool_header_target(
                    &tool_call.tool_call_id,
                    tool_call.header.disclosure_state.is_some(),
                );
                for line in &render.lines[start..] {
                    let interaction = expand_hint
                        .as_deref()
                        .filter(|hint| line.spans.iter().any(|span| span.content.contains(*hint)))
                        .and(target.clone())
                        .and_then(|target| bounded_interaction_row(Some(target), line));
                    render.interaction_rows.push(interaction);
                }
            }
            TranscriptToolCallDetailBlock::Message { text, tone } => {
                append_tool_call_message_block(
                    &mut render.lines,
                    text,
                    *tone,
                    theme,
                    width,
                    theme.surface.panel,
                    None,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
                        tool_call_id: tool_call.tool_call_id.clone(),
                        child_session_id: tool_call.child_session_id.clone(),
                        hovered_target: tool_call.hovered_target.clone(),
                        header: tool_call.header.clone(),
                        detail_blocks: vec![detail_block.clone()],
                        expanded: tool_call.expanded,
                    },
                    theme,
                    width,
                    theme.surface.panel,
                    None,
                );
            }
        }
    }
}

fn append_tool_call_detail_blocks(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    for detail_block in &tool_call.detail_blocks {
        let start = render.lines.len();
        match detail_block {
            TranscriptToolCallDetailBlock::Message { text, tone } => {
                append_tool_call_message_block(
                    &mut render.lines,
                    text,
                    *tone,
                    theme,
                    width,
                    base_surface,
                    card_shell,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::TodoList { items } => {
                append_tool_call_todo_list(
                    &mut render.lines,
                    items,
                    theme,
                    width,
                    base_surface,
                    card_shell,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::BashPanel {
                command,
                output,
                description,
                expand_hint,
                tone,
            } => {
                append_harness_bash_panel(
                    &mut render.lines,
                    HarnessBashPanel {
                        command,
                        output,
                        description: description.as_deref(),
                        expand_hint: expand_hint.as_deref(),
                        tone: *tone,
                    },
                    theme,
                    width,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                fallback_path,
                force_stacked,
                show_file_header,
            } => {
                append_tool_call_diff_block(
                    render,
                    diff_content,
                    fallback_path.as_deref(),
                    *force_stacked,
                    *show_file_header,
                    theme,
                    width,
                    base_surface,
                    card_shell,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::FileSection(file_section) => {
                append_tool_call_file_section(
                    render,
                    file_section,
                    theme,
                    width,
                    base_surface,
                    card_shell,
                );
            }
        }
    }
}

fn append_tool_call_file_section(
    render: &mut ToolSectionRender,
    file_section: &TranscriptToolCallFileSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    let header_target = Some(TranscriptMouseTarget::PatchFile {
        tool_call_id: file_section.tool_call_id.clone(),
        file_path: file_section.file_path.clone(),
    });
    let mut spans = vec![Span::styled(
        file_section.title.clone(),
        Style::default().fg(theme.text.primary),
    )];
    if let Some(subtitle) = file_section.subtitle.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }
    spans.push(Span::styled("  ", muted_meta_style(theme)));
    spans.push(Span::styled(
        tool_header_disclosure_glyph(Some(file_section.disclosure_state)).unwrap_or("▸"),
        muted_meta_style(theme),
    ));

    let file_surface = card_shell
        .map(|shell| shell.surface)
        .unwrap_or(base_surface);
    if let Some(shell) = card_shell {
        append_nested_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target,
            NestedSurfaceChrome {
                indent: shell.indent,
                rail_color: shell.rail_color,
                surface: shell.surface,
            },
            spans,
            transcript_surface_content_width(width, false),
        );
    } else {
        append_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            file_surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }

    if file_section.disclosure_state == TranscriptToolCallDisclosureState::Expanded {
        let nested_tool = TranscriptToolCallSection {
            tool_call_id: file_section.tool_call_id.clone(),
            child_session_id: None,
            hovered_target: None,
            header: TranscriptToolCallHeader {
                tool_id: String::new(),
                title: String::new(),
                subtitle: None,
                path_metadata: None,
                icon: None,
                status: ToolCallDisplayStatus::Succeeded,
                visual_style: TranscriptToolCallVisualStyle::Block,
                struck_out: false,
                disclosure_state: None,
            },
            detail_blocks: file_section.detail_blocks.clone(),
            expanded: true,
        };
        append_tool_call_detail_blocks(
            render,
            &nested_tool,
            theme,
            width,
            base_surface,
            card_shell,
        );
    }
}

fn append_tool_call_message_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    tone: TranscriptToolCallDetailTone,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    let style = match tone {
        TranscriptToolCallDetailTone::Primary => Style::default().fg(theme.text.primary),
        TranscriptToolCallDetailTone::Secondary => subdued_payload_style(theme),
        TranscriptToolCallDetailTone::Error => Style::default().fg(theme.status.error),
    };

    let indent = TRANSCRIPT_OPCODE_EDIT_INDENT;

    let surface = card_shell
        .map(|shell| shell.surface)
        .unwrap_or(base_surface);

    for row in text.split('\n') {
        let spans = if row.is_empty() {
            Vec::new()
        } else {
            vec![Span::styled(row.to_string(), style)]
        };
        if let Some(shell) = card_shell {
            append_nested_surface_row(
                lines,
                shell.indent,
                shell.rail_color,
                shell.surface,
                spans,
                transcript_surface_content_width(width, false),
            );
        } else {
            append_surface_row(
                lines,
                indent,
                surface,
                spans,
                transcript_surface_content_width(width, false),
            );
        }
    }
}

fn append_assistant_error_box(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let surface = base_surface;
    append_nested_surface_row(
        lines,
        TRANSCRIPT_NESTED_INDENT,
        theme.status.error,
        surface,
        vec![Span::styled(
            text.trim().to_string(),
            Style::default().fg(theme.text.secondary),
        )],
        width,
    );
}

fn append_tool_call_todo_list(
    lines: &mut Vec<Line<'static>>,
    items: &[TranscriptTodoItem],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    let surface = card_shell
        .map(|shell| shell.surface)
        .unwrap_or(base_surface);
    let render_width = transcript_surface_content_width(width, false);
    let ordered = ordered_todo_items(items);
    for item in ordered {
        let marker_style = item.status.style(theme);
        let content_style = item.status.content_style(theme);
        let spans = vec![
            Span::styled(format!("{} ", item.status.checkbox_glyph()), marker_style),
            Span::styled(item.content.clone(), content_style),
        ];
        if let Some(shell) = card_shell {
            append_nested_surface_row(
                lines,
                shell.indent,
                shell.rail_color,
                shell.surface,
                spans,
                render_width,
            );
        } else {
            append_surface_row(
                lines,
                TRANSCRIPT_OPCODE_EDIT_INDENT,
                surface,
                spans,
                render_width,
            );
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tool diff rendering keeps transcript shell styling explicit at the call site"
)]
fn append_tool_call_diff_block(
    render: &mut ToolSectionRender,
    diff_content: &str,
    fallback_path: Option<&str>,
    force_stacked: bool,
    show_file_header: bool,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    let nested_width = transcript_surface_content_width(width, false);
    let content_width = card_shell
        .map(|shell| {
            nested_width.saturating_sub(
                u16::try_from(nested_surface_prefix_width(shell.indent)).unwrap_or(u16::MAX),
            )
        })
        .unwrap_or_else(|| {
            nested_width.saturating_sub(
                u16::try_from(surface_prefix_width(TRANSCRIPT_OPCODE_EDIT_INDENT))
                    .unwrap_or(u16::MAX),
            )
        })
        .max(1);
    if let Some((diff_lines, hunk_offsets)) = render_structured_diff_lines_with_hunk_offsets(
        diff_content,
        fallback_path,
        "",
        content_width,
        StructuredDiffRenderOptions {
            force_stacked,
            highlight_intraline: false,
            highlight_syntax: true,
            show_file_header,
            show_hunk_header: false,
        },
        theme,
    ) {
        let start = render.lines.len();
        if let Some(shell) = card_shell {
            append_prebuilt_nested_surface_lines(
                &mut render.lines,
                shell.indent,
                shell.rail_color,
                shell.surface,
                diff_lines,
                nested_width,
            );
        } else {
            append_prebuilt_surface_lines(
                &mut render.lines,
                TRANSCRIPT_OPCODE_EDIT_INDENT,
                base_surface,
                diff_lines,
                nested_width,
            );
        }
        render.diff_hunk_offsets.extend(
            hunk_offsets
                .into_iter()
                .map(|offset| start.saturating_add(offset)),
        );
    }
}

fn build_assistant_footer_line(
    turn: &TranscriptTurnSection,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
    theme: &Theme,
) -> Line<'static> {
    let mut spans = vec![Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string())];
    spans.push(Span::styled(
        format!("{} ", assistant_icon),
        Style::default().fg(assistant_color),
    ));
    spans.push(Span::styled(
        assistant_footer_label(&turn.header.profile_label),
        Style::default().fg(assistant_primary_label_color(turn.header.status, theme)),
    ));
    if has_trimmed_content(&turn.header.model_id) {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(
            turn.header.model_id.clone(),
            muted_meta_style(theme),
        ));
    }
    if matches!(
        turn.header.status,
        ActivityStatus::Done | ActivityStatus::Error
    ) {
        if let Some(duration_ms) = turn.header.duration_ms {
            spans.push(Span::styled(" · ", muted_meta_style(theme)));
            spans.push(Span::styled(
                format_duration_ms(duration_ms),
                muted_meta_style(theme),
            ));
        }
    }
    if turn.header.status != ActivityStatus::Done {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(
            assistant_status.to_string(),
            Style::default().fg(assistant_color),
        ));
    }
    if let Some(timestamp) = turn.footer_timestamp.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(timestamp.to_string(), muted_meta_style(theme)));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "ui_transcript_exact_tests.rs"]
mod ui_transcript_exact_tests;
#[cfg(test)]
pub(crate) use ui_transcript_exact_tests::*;

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::UserMessageSubmittedEvent;

    #[test]
    fn transcript_test_activity_helper_has_required_defaults() {
        let entry = transcript_section_model_test_activity(
            "request-helper",
            ActivityStatus::Done,
            "assistant reply",
        );

        assert_eq!(entry.request_id, "request-helper");
        assert_eq!(entry.status, ActivityStatus::Done);
        assert_eq!(entry.transcript_text, "assistant reply");
        assert!(entry.tool_calls.is_empty());
        assert_eq!(entry.first_seq, 1);
        assert_eq!(entry.last_seq, 1);
    }

    #[test]
    fn transcript_test_tool_call_helper_has_queued_defaults() {
        let tool_call = transcript_section_model_test_tool_call("tool-helper", "fs.read");

        assert_eq!(tool_call.tool_call_id, "tool-helper");
        assert_eq!(tool_call.tool_id, "fs.read");
        assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);
        assert!(tool_call.output_summary.is_none());
        assert!(tool_call.artifact_refs.is_empty());
    }

    #[test]
    fn transcript_test_line_texts_joins_spans() {
        let texts = transcript_test_line_texts(vec![
            Line::from(vec![Span::raw("hello"), Span::raw(" world")]),
            Line::from(vec![Span::raw("again")]),
        ]);

        assert_eq!(texts, vec!["hello world", "again"]);
    }

    #[test]
    fn transcript_section_model_preserves_activity_order() {
        exact_test_transcript_section_model_preserves_activity_order();
    }

    #[test]
    fn transcript_section_model_keeps_nested_tool_and_error_blocks() {
        exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks();
    }

    #[cfg(test)]
    #[test]
    fn transcript_reasoning_precedes_answer_and_tool_rows() {
        exact_test_transcript_reasoning_precedes_answer_and_tool_rows();
    }

    #[test]
    fn transcript_follow_mode_uses_measured_surface_heights() {
        exact_test_transcript_follow_mode_uses_measured_surface_heights();
    }

    #[test]
    fn failed_tool_cards_parse_legacy_error_copy() {
        super::failed_tool_cards_parse_legacy_error_copy();
    }

    #[test]
    fn failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators() {
        super::failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators();
    }

    #[test]
    fn denied_tool_cards_use_denied_subtitle() {
        super::denied_tool_cards_use_denied_subtitle();
    }

    #[test]
    fn denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon() {
        super::denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon();
    }

    #[test]
    fn generic_failed_tool_messages_do_not_split_arbitrary_prefixes() {
        super::generic_failed_tool_messages_do_not_split_arbitrary_prefixes();
    }

    #[test]
    fn failed_tool_cards_fallback_when_error_details_are_missing() {
        super::failed_tool_cards_fallback_when_error_details_are_missing();
    }

    #[test]
    fn transcript_pending_permission_stays_after_last_activity() {
        exact_test_transcript_pending_permission_stays_after_last_activity();
    }

    #[test]
    fn transcript_layout_cache_invalidates_when_animation_frame_changes() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-streaming-cache".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.selected_activity_index = 0;

        let initial_lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));
        assert!(initial_lines
            .iter()
            .any(|line| line.contains("⠋ Assistant · gpt-5.4-mini · active")));

        app.advance_transcript_animation_phase();

        let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));
        assert!(updated_lines
            .iter()
            .any(|line| line.contains("⠙ Assistant · gpt-5.4-mini · active")));
    }

    #[test]
    fn transcript_layout_cache_invalidates_when_theme_changes() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-theme-cache".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-theme-cache".to_string(),
                text: "theme-sensitive prompt".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "reply".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.selected_activity_index = 0;

        let initial_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
        let initial_surface = initial_layout.sections[0].surfaces[0].surface;

        let mut alternate_theme = *app.theme();
        alternate_theme.surface.panel = Color::Rgb(0x22, 0x33, 0x44);
        app.set_theme_for_test(alternate_theme);

        let updated_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
        let updated_surface = updated_layout.sections[0].surfaces[0].surface;

        assert_ne!(initial_surface, updated_surface);
        assert_eq!(updated_surface, alternate_theme.surface.panel);
    }

    #[test]
    fn pending_permission_sections_render_warning_turn_container() {
        let mut app = AppState::default();
        app.ingest_event(harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: "evt_pending_permission_group".to_string(),
            seq: 1,
            run_id: "run_pending_permission_group".to_string(),
            mono_ms: 0,
            ts: None,
            actor: harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Supervisor,
                None,
            ),
            correlation_id: Some("tool_call_pending_permission_group".to_string()),
            causation_id: None,
            stream_key: Some("tool_call_pending_permission_group".to_string()),
            payload: harness_core::event::EventV1::PermissionRequested(
                harness_core::event::PermissionRequestedEvent {
                    permission_id: "perm_pending_permission_group".to_string(),
                    kind: "edit_fs".to_string(),
                    tool_call_id: Some("tool_call_pending_permission_group".to_string()),
                    summary: "Apply hashline edit to demo.txt".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 30_000,
                    default_decision: harness_core::event::PermissionDecision::Deny,
                },
            ),
        });

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert_eq!(lines[0], "Waiting for first turn…");
        assert!(lines
            .iter()
            .all(|line| !line.contains("permission checkpoint")
                && !line.contains("Apply hashline edit to demo.txt")));
    }

    #[test]
    fn streaming_assistant_footer_uses_reserved_active_label() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-streaming-header".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        let footer_row = lines
            .iter()
            .position(|line| line.contains("Assistant · gpt-5.4-mini · active"))
            .expect("streaming assistant footer row");
        assert_eq!(footer_row, 0);
        assert_eq!(lines[footer_row], "   ⠋ Assistant · gpt-5.4-mini · active");
    }

    #[test]
    fn only_latest_turn_renders_footer_metadata() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            ActivityEntry {
                request_id: "request-old-footer".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-old".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Done,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-old-footer".to_string(),
                    text: "first".to_string(),
                }),
                user_timestamp: Some("2026-03-19T09:44:00Z".to_string()),
                request_data: None,
                thinking_text: String::new(),
                transcript_text: "first reply".to_string(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
            },
            ActivityEntry {
                request_id: "request-new-footer".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-new".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Done,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-new-footer".to_string(),
                    text: "second".to_string(),
                }),
                user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
                request_data: None,
                thinking_text: String::new(),
                transcript_text: "second reply".to_string(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 2,
                last_seq: 2,
                first_mono_ms: 2,
                last_mono_ms: 2,
            },
        ]);
        app.selected_activity_index = 1;
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in "show timestamps".chars() {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("Assistant ·"))
                .count(),
            1
        );
        assert!(lines.iter().all(|line| !line.contains("gpt-old")));
        assert!(lines.iter().any(|line| line.contains("gpt-new")));
        assert!(lines.iter().all(|line| !line.contains("09:44")));
        assert!(lines.iter().any(|line| line.contains("09:45")));
    }

    #[test]
    fn tool_only_turns_render_standalone_assistant_footer() {
        let mut app = AppState::default();
        let mut entry = transcript_section_model_test_activity(
            "request-tool-only-footer",
            ActivityStatus::Done,
            "",
        );
        entry.tool_calls.push(crate::app::ToolCallEntry {
            tool_call_id: "call-tool-only-footer".to_string(),
            tool_id: "fs.read".to_string(),
            canonical_tool_id: None,
            alias_source_tool_id: None,
            resolved_tool_identity: None,
            args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
            args_digest: "digest-tool-only-footer".to_string(),
            lifecycle_state: None,
            status: ToolCallDisplayStatus::Succeeded,
            output_summary: Some("24 lines read from src/ui.rs".to_string()),
            output_digest: Some("digest-tool-only-output".to_string()),
            output_json: None,
            truncated_output: Some("24 lines read from src/ui.rs".to_string()),
            edit: None,
            lineage: None,
            artifact_refs: Vec::new(),
            timing_elapsed_ms: Some(2),
            permissions: Vec::new(),
            first_seq: 10,
            last_seq: 11,
            first_mono_ms: 10,
            last_mono_ms: 11,
            first_timestamp: None,
            last_timestamp: None,
        });
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(lines.iter().any(|line| line.contains("Read src/ui.rs")));
        assert!(lines.iter().any(|line| line.contains("Assistant ·")));
    }

    #[test]
    fn completed_latest_turn_keeps_footer_after_streaming_finishes() {
        let mut app = AppState::default();
        let mut entry = transcript_section_model_test_activity(
            "request-footer-finish",
            ActivityStatus::Streaming,
            "completed reply",
        );
        entry.user_timestamp = Some("2026-03-19T09:45:00Z".to_string());
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.selected_activity_index = 0;
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in "show timestamps".chars() {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        let streaming_lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));
        assert!(streaming_lines
            .iter()
            .any(|line| line.contains("Assistant · gpt-5.4-mini · active")));

        app.activities[0].status = ActivityStatus::Done;
        app.mark_transcript_dirty_for_test();

        let completed_lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(completed_lines
            .iter()
            .any(|line| line.contains("Assistant · gpt-5.4-mini")));
        assert!(completed_lines.iter().any(|line| line.contains("09:45")));
        assert!(completed_lines
            .iter()
            .all(|line| !line.contains("Assistant · gpt-5.4-mini · active")));
    }

    #[test]
    fn latest_completed_footer_follows_rendered_assistant_parts() {
        fn event(
            seq: u64,
            correlation_id: &str,
            payload: harness_core::event::EventV1,
        ) -> harness_core::event::EventEnvelopeV1 {
            harness_core::event::EventEnvelopeV1 {
                schema_version: harness_core::event::SCHEMA_VERSION,
                event_id: format!("evt_footer_rendered_parts_{seq:04}"),
                seq,
                run_id: "run_footer_rendered_parts".to_string(),
                mono_ms: seq * 100,
                ts: Some("2026-03-22T15:00:00Z".to_string()),
                actor: harness_core::event::EventActor::new(
                    harness_core::event::ActorKind::Worker,
                    Some("agent_parent".to_string()),
                ),
                correlation_id: Some(correlation_id.to_string()),
                causation_id: None,
                stream_key: None,
                payload,
            }
        }

        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(event(
            1,
            "req_footer_rendered_parts",
            harness_core::event::EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_footer_rendered_parts".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "Keep footer visible".to_string(),
                    request_digest: "digest-footer-rendered-parts".to_string(),
                    metadata: None,
                },
            ),
        ));
        app.ingest_event(event(
            2,
            "req_footer_rendered_parts",
            harness_core::event::EventV1::ProviderStreamDelta(
                harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_footer_rendered_parts".to_string(),
                    delta: "assistant reply from ordered events".to_string(),
                },
            ),
        ));
        app.ingest_event(event(
            3,
            "req_footer_rendered_parts",
            harness_core::event::EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: "req_footer_rendered_parts".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-footer-rendered-parts-out".to_string()),
                    usage: None,
                    metadata: None,
                },
            ),
        ));

        app.activities[0].transcript_text.clear();

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            96,
        ));

        assert!(lines
            .iter()
            .any(|line| line.contains("assistant reply from ordered events")));
        assert!(lines
            .iter()
            .any(|line| line.contains("Assistant · gpt-5.4-mini")));
    }

    #[test]
    fn user_message_surface_keeps_timestamp_in_latest_footer_only() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-user-padding".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-user-padding".to_string(),
                text: "hello".to_string(),
            }),
            user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "reply".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in "show timestamps".chars() {
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(!lines.iter().any(|line| line.contains("› You")));
        assert!(lines
            .iter()
            .all(|line| !(line.starts_with('┃') && line.contains("09:45"))));
        assert!(lines.iter().any(|line| line.contains("09:45")));
        assert!(lines
            .iter()
            .any(|line| line == "   ▪ Assistant · gpt-5.4-mini · 0ms · 09:45"));
        assert!(lines.iter().any(|line| line.contains("hello")));
        assert!(lines.iter().any(|line| line.contains("reply")));
    }

    #[test]
    fn reasoning_summary_renders_as_nested_inset_block() {
        let mut app = AppState::default();
        let mut entry = transcript_section_model_test_activity(
            "request-reasoning-inset",
            ActivityStatus::Done,
            "answer",
        );
        entry.thinking_text = "Matching harness response spacing".to_string();
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(lines
            .iter()
            .any(|line| line.contains("Matching harness response spacing")));
        assert!(lines.iter().any(|line| line.is_empty()));
        assert!(lines.iter().any(|line| line.contains("answer")));
    }

    #[test]
    fn fenced_code_blocks_render_frameless_with_highlighting() {
        let mut app = AppState::default();
        app.activities =
            std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
                "request-code-panel",
                ActivityStatus::Done,
                "Before\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nAfter",
            )]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            100,
        ));

        let function_row = lines
            .iter()
            .find(|line| line.contains("fn main()"))
            .expect("function row");
        let println_row = lines
            .iter()
            .find(|line| line.contains("println!(\"hi\")"))
            .expect("println row");

        assert!(lines.iter().any(|line| line.contains("Before")));
        assert!(
            !function_row.contains('┃') && !println_row.contains('┃'),
            "fenced code should keep syntax-highlighted content in flow without a nested frame\n{lines:#?}"
        );
        assert!(lines.iter().any(|line| line.contains("After")));
    }

    #[test]
    fn transcript_turn_sections_keep_exactly_one_blank_row_between_sections() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            transcript_section_model_test_activity("request-a", ActivityStatus::Done, "first"),
            transcript_section_model_test_activity("request-b", ActivityStatus::Done, "second"),
        ]);
        app.selected_activity_index = 1;

        let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);

        assert_eq!(layout.sections.len(), 2);
        assert_eq!(layout.sections[0].leading_gap_height, 0);
        assert_eq!(layout.sections[1].leading_gap_height, 1);
    }

    #[test]
    fn assistant_tool_surfaces_keep_same_trailing_gap_as_text_boxes() {
        let mut activity = transcript_section_model_test_activity(
            "request-shell-alignment",
            ActivityStatus::Done,
            "I’ll run a harmless shell command.",
        );
        activity.user_message = Some(UserMessageSubmittedEvent {
            request_id: "request-shell-alignment".to_string(),
            text: "test out some tools".to_string(),
        });

        let mut shell_call = transcript_section_model_test_tool_call("tc-shell-alignment", "bash");
        shell_call.args_summary = r#"{"command":"printf 'bash smoke test ok\n'","description":"Run harmless shell smoke test"}"#.to_string();
        shell_call.status = ToolCallDisplayStatus::Succeeded;
        shell_call.output_summary = Some("bash smoke test ok".to_string());
        activity.tool_calls.push(shell_call);

        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![activity]);
        app.selected_activity_index = 0;

        let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);
        let surfaces = &layout.sections[0].surfaces;
        let user_surface = surfaces
            .iter()
            .find(|surface| surface.show_outer_rail)
            .expect("user surface");
        let tool_surface = surfaces
            .iter()
            .find(|surface| {
                transcript_test_line_texts(surface.lines.clone())
                    .iter()
                    .any(|line| line.contains("bash smoke test ok"))
            })
            .expect("assistant tool surface");
        let tool_lines = transcript_test_line_texts(tool_surface.lines.clone());
        let tool_interactions = tool_surface
            .interaction_rows
            .as_ref()
            .expect("tool surface interaction rows");

        assert_eq!(user_surface.width, 78);
        assert_eq!(tool_surface.width, user_surface.width);
        let title_row = tool_lines
            .iter()
            .position(|line| line.contains("# Run harmless shell smoke test"))
            .expect("title line");
        let command_row = tool_lines
            .iter()
            .position(|line| line.contains("$ printf 'bash smoke test ok"))
            .expect("command line");
        let output_row = tool_lines
            .iter()
            .enumerate()
            .find_map(|(index, line)| {
                (!line.contains("$ printf") && line.contains("bash smoke test ok")).then_some(index)
            })
            .expect("output line");
        let title_column = tool_lines[title_row]
            .find("# Run harmless shell smoke test")
            .expect("title column");
        let command_column = tool_lines[command_row]
            .find("$ printf 'bash smoke test ok")
            .expect("command column");
        let output_column = tool_lines[output_row]
            .find("bash smoke test ok")
            .expect("output column");
        assert_eq!(title_column, command_column);
        assert_eq!(output_column, command_column);
        assert_eq!(tool_interactions[title_row], None);
        assert_eq!(tool_interactions[command_row], None);
        assert_eq!(tool_interactions[output_row], None);
        assert!(
            tool_lines.iter().any(|line| line.starts_with(&format!(
                "{TRANSCRIPT_COMMAND_TOOL_INDENT}{HARNESS_SPLIT_RAIL_GLYPH}"
            )) && line.contains("bash smoke test ok")),
            "command card rail should align with transcript text box edge\n{tool_lines:#?}"
        );
        assert!(
            tool_surface
                .lines
                .iter()
                .all(|line| line.width() <= usize::from(tool_surface.width)),
            "tool card lines should be built for the same visual width as the rendered surface"
        );

        let area = Rect::new(0, 0, 100, 30);
        let snapshot = transcript_selection_debug_snapshot(&app, area)
            .expect("shell command card selection snapshot");
        let title_row = snapshot
            .rows
            .iter()
            .position(|line| line.contains("# Run harmless shell smoke test"))
            .expect("selectable title row");
        let output_row = snapshot
            .rows
            .iter()
            .position(|line| line.contains("bash smoke test ok"))
            .expect("selectable output row");
        let copied = transcript_selection_text(
            &app,
            area,
            TranscriptSelection {
                anchor: TranscriptSelectionCell {
                    row: title_row,
                    column: 0,
                },
                focus: TranscriptSelectionCell {
                    row: output_row,
                    column: usize::from(snapshot.viewport.width.saturating_sub(1)),
                },
            },
        )
        .expect("shell command card selection copies text");
        assert!(
            copied.starts_with("# Run harmless shell smoke test"),
            "copied shell card text should skip visual rail/padding: {copied:?}"
        );
        assert!(copied.contains("$ printf 'bash smoke test ok"));
        assert!(copied.contains("bash smoke test ok"));
        assert!(!copied.contains(HARNESS_SPLIT_RAIL_GLYPH));
    }

    #[test]
    fn assistant_tool_surface_spacing_matches_shell_rhythm() {
        assert_eq!(
            transcript_surface_leading_gap(
                Some(TranscriptRenderSurfaceKind::AssistantBody),
                TranscriptRenderSurfaceKind::AssistantTool,
            ),
            0,
            "assistant text should hand off to tool rows without an extra blank terminal row"
        );
        assert_eq!(
            transcript_surface_leading_gap(
                Some(TranscriptRenderSurfaceKind::AssistantTool),
                TranscriptRenderSurfaceKind::AssistantBody,
            ),
            1,
            "tool rows should still leave a single section break before the next assistant text block"
        );
        assert_eq!(
            transcript_surface_leading_gap(
                Some(TranscriptRenderSurfaceKind::AssistantReasoning),
                TranscriptRenderSurfaceKind::AssistantBody,
            ),
            0,
            "reasoning-to-body spacing is carried by the nested blank row inside the body surface"
        );
        assert_eq!(
            transcript_surface_leading_gap(
                Some(TranscriptRenderSurfaceKind::AssistantTool),
                TranscriptRenderSurfaceKind::AssistantReasoning,
            ),
            0,
            "tool-to-reasoning spacing is carried by the reasoning block itself so the rendered gap stays single-row"
        );
    }

    #[test]
    fn reasoning_to_answer_transition_uses_single_blank_row() {
        let mut app = AppState::default();
        let mut entry = transcript_section_model_test_activity(
            "request-reasoning-gap",
            ActivityStatus::Done,
            "answer",
        );
        entry.thinking_text = "reasoning".to_string();
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));
        let reasoning_row = lines
            .iter()
            .position(|line| line.contains("reasoning"))
            .expect("reasoning row");
        let answer_row = lines
            .iter()
            .position(|line| line.contains("answer"))
            .expect("answer row");

        assert_eq!(
            answer_row,
            reasoning_row + 2,
            "reasoning and answer should be separated by exactly one blank terminal row\n{lines:#?}"
        );
        assert!(lines[reasoning_row + 1].is_empty());
    }

    #[test]
    fn streaming_assistant_footer_spinner_uses_deterministic_braille_frames() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-streaming-spinner".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.selected_activity_index = 0;

        let first = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));
        app.advance_transcript_animation_phase();
        let second = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert_eq!(first[0], "   ⠋ Assistant · gpt-5.4-mini · active");
        assert_eq!(second[0], "   ⠙ Assistant · gpt-5.4-mini · active");
    }

    #[test]
    fn queued_runtime_status_without_pending_assistant_does_not_render_user_badge_or_footer() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            ActivityEntry {
                request_id: "request-complete".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Done,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-complete".to_string(),
                    text: "completed turn".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                transcript_text: "done".to_string(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
            },
            ActivityEntry {
                request_id: "request-queued-followup".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Queued,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-queued-followup".to_string(),
                    text: "follow up after the active turn finished".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                transcript_text: String::new(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 2,
                last_seq: 2,
                first_mono_ms: 2,
                last_mono_ms: 2,
            },
        ]);
        app.selected_activity_index = 1;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(
            lines.iter().all(|line| !line.contains(" QUEUED ")),
            "runtime queued status alone should not show the Harness queued badge: {lines:#?}"
        );
        assert!(
            lines
                .iter()
                .all(|line| !line.contains("Assistant · gpt-5.4-mini · queued")),
            "runtime queued status alone should not render a queued assistant footer: {lines:#?}"
        );
    }

    #[test]
    fn streaming_turn_with_own_user_message_does_not_render_queued_badge() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-started-followup".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-started-followup".to_string(),
                text: "follow up now running".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(
            lines.iter().all(|line| !line.contains(" QUEUED ")),
            "a turn should not mark its own user message as queued once it is the active assistant turn: {lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Assistant · gpt-5.4-mini · active")),
            "streaming follow-up should keep the active assistant footer: {lines:#?}"
        );
    }

    #[test]
    fn queued_user_followup_keeps_active_footer_on_streaming_turn() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            ActivityEntry {
                request_id: "request-active".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Streaming,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-active".to_string(),
                    text: "active turn".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                transcript_text: String::new(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
            },
            ActivityEntry {
                request_id: "request-queued-followup".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Queued,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-queued-followup".to_string(),
                    text: "follow up while the current turn is still running".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                transcript_text: String::new(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 2,
                last_seq: 2,
                first_mono_ms: 2,
                last_mono_ms: 2,
            },
        ]);
        app.selected_activity_index = 1;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert!(
            lines
                .iter()
                .any(|line| line.contains("Assistant · gpt-5.4-mini · active")),
            "active assistant footer should stay on the in-flight turn: {lines:#?}"
        );
        assert!(
            lines.iter().any(|line| line.contains(" QUEUED ")),
            "queued follow-up should still show the user badge: {lines:#?}"
        );
        assert!(
            lines
                .iter()
                .all(|line| !line.contains("Assistant · gpt-5.4-mini · queued")),
            "queued follow-up should not steal the assistant footer: {lines:#?}"
        );
    }

    #[test]
    fn transcript_wrapping_respects_display_width_for_wide_glyphs() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-wide-wrap".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-wide-wrap".to_string(),
                text: "wrap this diff body".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "漢字🙂漢字🙂漢字🙂 漢字🙂漢字🙂漢字🙂 漢字🙂漢字🙂漢字🙂".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        }]);
        app.selected_activity_index = 0;

        let lines = build_transcript_lines_for_width(&app, &Theme::default(), 20);
        assert!(
            lines
                .iter()
                .filter(|line| {
                    line.spans
                        .iter()
                        .any(|span| span.content.contains('漢') || span.content.contains('🙂'))
                })
                .all(|line| line.width() <= 20),
            "transcript rows should honor visible width: {:#?}",
            transcript_test_line_texts(lines)
        );
    }

    #[test]
    fn transcript_selection_snapshot_cache_reuses_repeated_hit_tests() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "req_selection_cache".to_string(),
            profile_label: "default".to_string(),
            model_id: "model-1".to_string(),
            provider_id: "default".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(UserMessageSubmittedEvent {
                request_id: "req_selection_cache".to_string(),
                text: "Select this".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "Cache this selection text across repeated hit tests".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 2,
            first_mono_ms: 1,
            last_mono_ms: 2,
        }]);
        app.selected_activity_index = 0;

        let area = Rect::new(0, 0, 140, 40);
        let snapshot = transcript_selection_debug_snapshot(&app, area).expect("selection snapshot");
        let row = snapshot
            .rows
            .iter()
            .position(|line| line.contains("selection text"))
            .expect("selection text row");
        let column = snapshot.rows[row]
            .find("selection")
            .expect("selection text column");

        reset_transcript_selection_cache_metrics_for_test();

        for offset in 0..6 {
            assert!(transcript_selection_cell(
                &app,
                area,
                snapshot.viewport.x + u16::try_from(column + offset).expect("column fits"),
                snapshot.viewport.y + u16::try_from(row).expect("row fits"),
            )
            .is_some());
        }

        assert_eq!(transcript_selection_cache_build_count_for_test(), 1);
    }

    #[test]
    fn startup_lifecycle_text_participates_in_selection_copy() {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_launch_metadata(
            crate::app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4")
                .with_mode_label("Demo"),
        );

        let area = Rect::new(0, 0, 100, 24);
        let snapshot = transcript_selection_debug_snapshot(&app, area)
            .expect("startup lifecycle selection snapshot");
        let purpose = app.theme().live_shell.startup.new_session_purpose;
        assert!(!snapshot.rows.iter().any(|line| line.contains(purpose)));
        assert!(!snapshot.rows.iter().any(|line| line.contains("Launch:")));
        assert!(!snapshot.rows.iter().any(|line| line.contains("Provider")));
        let row = snapshot
            .rows
            .iter()
            .position(|line| line.contains("┏━╸"))
            .expect("startup logo row is selectable");

        let hit = transcript_selection_cell(
            &app,
            area,
            snapshot.viewport.x,
            snapshot.viewport.y + u16::try_from(row).expect("row fits"),
        )
        .expect("startup row accepts selection hits");
        assert_eq!(hit.row, row);

        let copied = transcript_selection_text(
            &app,
            area,
            TranscriptSelection {
                anchor: TranscriptSelectionCell { row, column: 0 },
                focus: TranscriptSelectionCell {
                    row,
                    column: usize::from(snapshot.viewport.width.saturating_sub(1)),
                },
            },
        )
        .expect("startup text copies from selection");
        assert!(copied.contains("┏━╸"));
    }

    #[test]
    fn live_empty_state_text_participates_in_selection_copy() {
        let mut app = AppState::new_live(None, false, None);
        app.set_launch_metadata(
            crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
                .with_mode_label("Demo"),
        );

        let area = Rect::new(0, 0, 100, 24);
        let snapshot = transcript_selection_debug_snapshot(&app, area)
            .expect("empty-state lifecycle selection snapshot");
        let value_prop = app.theme().live_shell.empty_state.value_prop;
        let row = snapshot
            .rows
            .iter()
            .position(|line| line.contains(value_prop))
            .expect("empty-state value prop row is selectable");

        assert!(transcript_selection_cell(
            &app,
            area,
            snapshot.viewport.x,
            snapshot.viewport.y + u16::try_from(row).expect("row fits"),
        )
        .is_some());

        let copied = transcript_selection_text(
            &app,
            area,
            TranscriptSelection {
                anchor: TranscriptSelectionCell { row, column: 0 },
                focus: TranscriptSelectionCell {
                    row,
                    column: usize::from(snapshot.viewport.width.saturating_sub(1)),
                },
            },
        )
        .expect("empty-state text copies from selection");
        assert_eq!(copied, value_prop);
    }

    #[test]
    fn live_empty_state_wrapped_examples_participate_in_selection_copy() {
        let mut app = AppState::new_live(None, false, None);
        app.set_launch_metadata(
            crate::app::LaunchMetadata::from_model_ref("worker", "mock:model-1")
                .with_mode_label("Demo"),
        );

        let area = Rect::new(0, 0, 80, 24);
        let snapshot = transcript_selection_debug_snapshot(&app, area)
            .expect("empty-state lifecycle selection snapshot");
        let prompts = &app.theme().live_shell.empty_state.example_prompts;
        let first_example_row = snapshot
            .rows
            .iter()
            .position(|line| line.contains(prompts[0].prompt))
            .expect("first example prompt row is selectable");
        let wrapped_example_row = first_example_row.saturating_add(1);
        assert!(
            snapshot.rows[wrapped_example_row].contains(prompts[1].prompt)
                || snapshot.rows[wrapped_example_row].contains(prompts[2].prompt)
                || snapshot.rows[wrapped_example_row].contains("review")
                || snapshot.rows[wrapped_example_row].contains("latest"),
            "wrapped examples row should carry visible prompt text: {:?}",
            snapshot.rows[wrapped_example_row]
        );

        let copied = transcript_selection_text(
            &app,
            area,
            TranscriptSelection {
                anchor: TranscriptSelectionCell {
                    row: wrapped_example_row,
                    column: 0,
                },
                focus: TranscriptSelectionCell {
                    row: wrapped_example_row,
                    column: usize::from(snapshot.viewport.width.saturating_sub(1)),
                },
            },
        )
        .expect("wrapped example text copies from selection");
        assert!(
            copied.contains(prompts[1].prompt)
                || copied.contains(prompts[2].prompt)
                || copied.contains("review")
                || copied.contains("latest"),
            "copied: {copied:?}"
        );
    }
}
