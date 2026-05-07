use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Theme as SyntectTheme};
use syntect::parsing::SyntaxSet;

use super::*;

use crate::text::{
    has_trimmed_content, non_empty_trimmed, replace_control_chars_except_tabs,
    trimmed_json_string_field,
};
use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;
use crate::time_format::short_time_or_trimmed;

use super::ui_secondary::{
    format_detail_payload, render_structured_diff_lines, render_structured_diff_lines_with_options,
    StructuredDiffRenderOptions,
};

#[derive(Debug, Clone, Copy)]
struct TranscriptToolCardShell {
    indent: &'static str,
    rail_color: Color,
    surface: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptQuestionAnswerItem {
    question: String,
    answer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTodoItem {
    content: String,
    status: TranscriptTodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptTodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

const THINKING_TRACE_LABEL: &str = "Thinking:";

struct SyntaxHighlightAssets {
    syntax_set: SyntaxSet,
    theme: SyntectTheme,
}

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
    static TRANSCRIPT_SELECTION_CACHE: RefCell<Vec<TranscriptSelectionCacheEntry>> = const { RefCell::new(Vec::new()) };

    #[cfg(test)]
    static TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeasuredTranscriptSectionKind {
    Turn,
    PendingPermission,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
struct MeasuredTranscriptSection {
    kind: MeasuredTranscriptSectionKind,
    top_row: usize,
    leading_gap_height: usize,
    content_height: usize,
    surfaces: Vec<MeasuredTranscriptSurface>,
    lines: Vec<Line<'static>>,
}

impl MeasuredTranscriptSection {
    fn total_height(&self) -> usize {
        self.leading_gap_height + self.content_height
    }
}

#[derive(Debug, Clone, Default)]
struct MeasuredTranscriptLayout {
    sections: Vec<MeasuredTranscriptSection>,
    total_height: usize,
}

#[derive(Debug, Clone)]
struct TranscriptRenderSurface {
    kind: TranscriptRenderSurfaceKind,
    show_outer_rail: bool,
    rail_color: Color,
    surface: Color,
    lines: Vec<Line<'static>>,
    interaction_rows: Option<Vec<Option<TranscriptMouseTarget>>>,
    selection_rows: Option<Vec<TranscriptSelectionRow>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptRenderSurfaceKind {
    User,
    AssistantReasoning,
    AssistantBody,
    AssistantTool,
    AssistantError,
    AssistantFooter,
    PendingPermission,
}

#[derive(Debug, Clone)]
struct MeasuredTranscriptSurface {
    top_offset: usize,
    height: usize,
    width: u16,
    show_outer_rail: bool,
    rail_color: Color,
    surface: Color,
    lines: Vec<Line<'static>>,
    interaction_rows: Option<Vec<Option<TranscriptMouseTarget>>>,
    selection_rows: Option<Vec<TranscriptSelectionRow>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
struct ToolSectionRender {
    lines: Vec<Line<'static>>,
    interaction_rows: Vec<Option<TranscriptMouseTarget>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptPathMetadata {
    leaf: String,
    parent: Option<String>,
}

#[derive(Debug, Clone)]
struct ApplyPatchFileRenderEntry {
    file_path: String,
    diff_rel_path: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct TranscriptViewportLayout {
    content: Rect,
    scrollbar_chrome: Option<Rect>,
    scrollbar_lane: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptScrollbarHit {
    pub lane: Rect,
    pub track: Rect,
    pub thumb: Rect,
    pub max_scroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TranscriptSelectionCell {
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranscriptSelection {
    pub anchor: TranscriptSelectionCell,
    pub focus: TranscriptSelectionCell,
}

impl TranscriptSelection {
    fn normalized(self) -> (TranscriptSelectionCell, TranscriptSelectionCell) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone)]
struct TranscriptSelectionSnapshot {
    viewport: Rect,
    scroll_top: usize,
    rows: Vec<TranscriptSelectionRow>,
}

#[derive(Debug, Clone)]
struct TranscriptSelectionCacheEntry {
    render_width: u16,
    app_instance_id: u64,
    render_key: u64,
    theme: Theme,
    area: Rect,
    follow_mode: bool,
    transcript_scroll: usize,
    snapshot: TranscriptSelectionSnapshot,
}

#[derive(Debug, Clone)]
struct TranscriptSelectionRow {
    cells: Vec<String>,
    continues_previous: bool,
    copy_offset: usize,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TranscriptSelectionDebugSnapshot {
    pub viewport: Rect,
    pub rows: Vec<String>,
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

impl TranscriptSelectionSnapshot {
    fn hit(&self, column: u16, row: u16) -> Option<TranscriptSelectionCell> {
        if !rect_contains(self.viewport, column, row) {
            return None;
        }

        Some(TranscriptSelectionCell {
            row: self
                .scroll_top
                .saturating_add(usize::from(row.saturating_sub(self.viewport.y))),
            column: usize::from(column.saturating_sub(self.viewport.x)),
        })
    }

    fn selection_text(&self, selection: TranscriptSelection) -> Option<String> {
        let (start, end) = selection.normalized();
        if start == end || self.rows.is_empty() {
            return None;
        }

        let last_row = self.rows.len().saturating_sub(1);
        let start_row = start.row.min(last_row);
        let end_row = end.row.min(last_row);
        if start_row > end_row {
            return None;
        }

        let mut lines = Vec::new();
        for row_idx in start_row..=end_row {
            let row = self.rows.get(row_idx)?;
            if row.cells.is_empty() {
                lines.push(String::new());
                continue;
            }
            let content_start = selection_row_content_start(row);
            let Some(content_end) = selection_row_content_end(row, content_start) else {
                if row_idx == start_row || !row.continues_previous || lines.is_empty() {
                    lines.push(String::new());
                }
                continue;
            };

            let row_start = if row_idx == start_row {
                start
                    .column
                    .max(content_start.max(row.copy_offset))
                    .min(row.cells.len().saturating_sub(1))
            } else {
                content_start.max(row.copy_offset)
            };
            let row_end = if row_idx == end_row {
                end.column.min(content_end)
            } else {
                content_end
            };
            if row_start > row_end {
                lines.push(String::new());
                continue;
            }

            let mut text = String::new();
            for cell in row
                .cells
                .iter()
                .skip(row_start)
                .take(row_end - row_start + 1)
            {
                text.push_str(cell);
            }
            if row_idx != start_row && row.continues_previous && !lines.is_empty() {
                let continuation = text.trim_start_matches(' ');
                let current = lines.last_mut().expect("continuation has previous line");
                if !continuation.is_empty() && !current.ends_with(char::is_whitespace) {
                    current.push(' ');
                }
                current.push_str(continuation);
            } else {
                lines.push(text);
            }
        }

        Some(lines.join("\n"))
    }

    #[cfg(test)]
    fn visible_rows(&self) -> Vec<String> {
        self.rows
            .iter()
            .skip(self.scroll_top)
            .take(usize::from(self.viewport.height))
            .map(|row| row.cells.join(""))
            .collect()
    }
}

fn selection_row_content_start(row: &TranscriptSelectionRow) -> usize {
    usize::from(
        row.cells
            .first()
            .is_some_and(|cell| cell.as_str() == TRANSCRIPT_RAIL_GLYPH),
    )
}

fn selection_row_content_end(row: &TranscriptSelectionRow, content_start: usize) -> Option<usize> {
    row.cells
        .iter()
        .enumerate()
        .rev()
        .find(|(idx, cell)| *idx >= content_start && !cell.is_empty() && cell.as_str() != " ")
        .map(|(idx, _)| idx)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptSection {
    Turn(Box<TranscriptTurnSection>),
    PendingPermission(TranscriptPendingPermissionSection),
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
    profile_label: &'a str,
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
enum TranscriptToolCallDetailBlock {
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
struct TranscriptToolCallFileSection {
    tool_call_id: String,
    file_path: String,
    title: String,
    subtitle: Option<String>,
    disclosure_state: TranscriptToolCallDisclosureState,
    detail_blocks: Vec<TranscriptToolCallDetailBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptToolCallVisualStyle {
    Inline,
    TaskInline,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptToolCallDetailTone {
    Primary,
    Secondary,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptToolCallDisclosureState {
    Collapsed,
    Expanded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptErrorSection {
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptAssistantPart {
    Reasoning(TranscriptLabeledTextSection),
    Body(TranscriptBodyBlock),
    ToolCall(TranscriptToolCallSection),
    Error(TranscriptErrorSection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptPendingPermissionSection {
    summary: String,
}

enum ParsedTextBlock {
    Plain(String),
    Code {
        language: Option<String>,
        body: String,
        raw: String,
    },
}

const TRANSCRIPT_SURFACE_RAIL_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH: u16 = 0;
const TRANSCRIPT_SCROLLBAR_TRACK_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_THUMB_WIDTH: u16 = 1;
const TRANSCRIPT_SCROLLBAR_MIN_THUMB_HEIGHT: usize = 3;
const TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH: u16 = 2;
const TRANSCRIPT_SURFACE_BODY_PREFIX: &str = " ";
const TRANSCRIPT_USER_BODY_PREFIX: &str = "  ";
const TRANSCRIPT_ASSISTANT_BODY_PREFIX: &str = "   ";
const TRANSCRIPT_REASONING_BODY_PREFIX: &str = "  ";
const TRANSCRIPT_NESTED_INDENT: &str = "  ";
const TRANSCRIPT_OPCODE_EDIT_INDENT: &str = "    ";
const TRANSCRIPT_RAIL_GLYPH: &str = "┃";
const TRANSCRIPT_SCROLLBAR_THUMB_GLYPH: &str = "█";
const TRANSCRIPT_SECTION_GAP_HEIGHT: usize = 1;
const HARNESS_BASH_OUTPUT_LINE_CLAMP: usize = 10;
const HARNESS_BLOCK_TOOL_MARGIN_TOP: usize = 1;
const HARNESS_BLOCK_TOOL_PADDING_TOP: usize = 1;
const HARNESS_BLOCK_TOOL_PADDING_BOTTOM: usize = 1;
const HARNESS_BLOCK_TOOL_PADDING_LEFT: usize = 2;
const HARNESS_BLOCK_TOOL_TITLE_PADDING_LEFT: usize = 3;
const HARNESS_BLOCK_TOOL_GAP: usize = 1;
const HARNESS_SPLIT_RAIL_GLYPH: &str = "┃";
const HARNESS_SPLIT_RAIL_WIDTH: usize = 1;
const TRANSCRIPT_BRAILLE_SPINNER_FRAMES: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
        return TranscriptPaneContext {
            inner_area: inset_rect(
                area,
                theme.live_shell.rhythm.transcript_gutter_x,
                theme.live_shell.rhythm.transcript_gutter_y,
            ),
            base_surface: theme.surface.panel,
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
        |layout| transcript_scrollbar_needed(layout, inner_area),
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

            let transcript_scroll = transcript_scroll_offset(app, layout, viewport.content);
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
        |layout| transcript_scrollbar_needed(layout, context.inner_area),
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
            scroll_top: transcript_scroll_offset(app, layout, viewport.content),
            rows: transcript_selection_rows(layout, render_width),
        },
    ))
}

fn with_transcript_selection_snapshot<R>(
    app: &AppState,
    area: Rect,
    render: impl FnOnce(&TranscriptSelectionSnapshot) -> R,
) -> Option<R> {
    let app_instance_id = app.transcript_cache_instance_id();
    let render_key = app.transcript_render_cache_key();
    let theme = *app.theme();
    let follow_mode = app.follow_mode;
    let transcript_scroll = app.transcript_scroll;
    let transcript_area = area;
    let context = transcript_pane_context(app, transcript_area, &theme);
    let full_width = context.inner_area.width;
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        full_width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout, context.inner_area),
    );
    let render_width_u16 = if show_scrollbar {
        full_width
            .saturating_sub(TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH + TRANSCRIPT_SCROLLBAR_TRACK_WIDTH)
    } else {
        full_width
    };

    TRANSCRIPT_SELECTION_CACHE.with(|cache| {
        {
            let cache = cache.borrow();
            if let Some(entry) = cache.iter().find(|entry| {
                entry.app_instance_id == app_instance_id
                    && entry.render_key == render_key
                    && entry.theme == theme
                    && entry.area == area
                    && entry.follow_mode == follow_mode
                    && entry.transcript_scroll == transcript_scroll
                    && entry.render_width == render_width_u16
            }) {
                return Some(render(&entry.snapshot));
            }
        }

        let snapshot = build_transcript_selection_snapshot(app, area)?;

        #[cfg(test)]
        TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT
            .with(|count| count.set(count.get().saturating_add(1)));

        {
            let mut cache = cache.borrow_mut();
            cache.retain(|entry| {
                entry.app_instance_id != app_instance_id || entry.render_key == render_key
            });
            cache.push(TranscriptSelectionCacheEntry {
                app_instance_id,
                render_key,
                theme,
                area,
                follow_mode,
                transcript_scroll,
                render_width: render_width_u16,
                snapshot,
            });
            if cache.len() > 8 {
                let overflow = cache.len().saturating_sub(8);
                cache.drain(0..overflow);
            }
        }

        let cache = cache.borrow();
        let entry = cache
            .iter()
            .find(|entry| {
                entry.app_instance_id == app_instance_id
                    && entry.render_key == render_key
                    && entry.theme == theme
                    && entry.area == area
                    && entry.follow_mode == follow_mode
                    && entry.transcript_scroll == transcript_scroll
                    && entry.render_width == render_width_u16
            })
            .expect("cached transcript selection snapshot should be present after insertion");
        Some(render(&entry.snapshot))
    })
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

fn selection_rows_for_markdownish_text_block(
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    let base_style = Style::default().fg(color);
    let mut rows = Vec::new();

    for line in text.lines() {
        rows.extend(selection_rows_for_markdownish_line(
            line, color, prefix, base_style, theme, width,
        ));
    }

    if text.is_empty() {
        rows.extend(selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            Vec::new(),
            width,
            display_width(prefix),
        ));
    }

    rows
}

fn selection_rows_for_markdownish_line(
    line: &str,
    color: Color,
    prefix: &str,
    base_style: Style,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    if line.is_empty() {
        return selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            Vec::new(),
            width,
            display_width(prefix),
        );
    }

    let indent_width = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let indent = " ".repeat(indent_width);
    let trimmed = line.trim_start();
    let content_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);

    if let Some(text) = markdown_heading_text(trimmed) {
        return selection_rows_for_prefixed_wrapped_spans(
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown_spans(
                text,
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                theme.text.accent,
                theme,
            ),
            width,
            display_width(prefix),
        );
    }

    if markdown_rule(trimmed) {
        return selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            vec![Span::styled(
                "─".repeat(content_width),
                Style::default().fg(theme.text.secondary),
            )],
            width,
            display_width(prefix),
        );
    }

    if let Some(text) = trimmed.strip_prefix("> ") {
        return selection_rows_for_prefixed_wrapped_spans(
            &format!("{prefix}{indent}▍ "),
            Style::default().fg(theme.text.secondary),
            parse_inline_markdown_spans(
                text,
                Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::ITALIC),
                theme.text.secondary,
                theme,
            ),
            width,
            display_width(prefix),
        );
    }

    if let Some((list_prefix, text, list_style, text_style)) = markdown_list_prefix(trimmed, theme)
    {
        return selection_rows_for_prefixed_wrapped_spans(
            &format!("{prefix}{indent}{list_prefix}"),
            list_style,
            parse_inline_markdown_spans(text, text_style, color, theme),
            width,
            display_width(prefix),
        );
    }

    selection_rows_for_prefixed_wrapped_spans(
        prefix,
        base_style,
        parse_inline_markdown_spans(trimmed, base_style, color, theme),
        width,
        display_width(prefix),
    )
}

fn selection_rows_for_prefixed_wrapped_spans(
    prefix: &str,
    prefix_style: Style,
    content_spans: Vec<Span<'static>>,
    width: u16,
    copy_offset: usize,
) -> Vec<TranscriptSelectionRow> {
    let rendered_lines = if content_spans.is_empty() {
        vec![Line::from(Span::styled(prefix.to_string(), prefix_style))]
    } else {
        let prefix_width = display_width(prefix);
        let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
        wrap_surface_spans(content_spans, content_width)
            .into_iter()
            .map(|row| {
                let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
                spans.extend(row);
                Line::from(spans)
            })
            .collect::<Vec<_>>()
    };

    rendered_lines
        .into_iter()
        .enumerate()
        .map(|(idx, row)| TranscriptSelectionRow {
            cells: transcript_selection_line_rows(&row, usize::from(width))
                .into_iter()
                .next()
                .unwrap_or_else(|| vec![" ".to_string(); usize::from(width)]),
            continues_previous: idx > 0,
            copy_offset,
        })
        .collect()
}

fn transcript_selection_line_rows(line: &Line<'static>, width: usize) -> Vec<Vec<String>> {
    let mut row = Vec::new();
    let mut rows = Vec::new();

    for span in &line.spans {
        for ch in span.content.chars() {
            let display = ch.to_string();
            let cell_width = display_width(&display).max(1);
            if row.len() + cell_width > width {
                row.resize(width, " ".to_string());
                rows.push(std::mem::take(&mut row));
            }

            row.push(display);
            for _ in 1..cell_width {
                if row.len() == width {
                    rows.push(std::mem::take(&mut row));
                }
                row.push(String::new());
            }

            if row.len() == width {
                rows.push(std::mem::take(&mut row));
            }
        }
    }

    if rows.is_empty() && row.is_empty() {
        row.resize(width, " ".to_string());
        rows.push(row);
        return rows;
    }

    if !row.is_empty() {
        row.resize(width, " ".to_string());
        rows.push(row);
    }

    rows
}

fn selection_rows_for_rendered_line(
    line: &Line<'static>,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    transcript_selection_line_rows(line, usize::from(width.max(1)))
        .into_iter()
        .enumerate()
        .map(|(idx, cells)| TranscriptSelectionRow {
            cells,
            continues_previous: idx > 0,
            copy_offset: 0,
        })
        .collect()
}

fn blank_selection_row(width: u16) -> TranscriptSelectionRow {
    TranscriptSelectionRow {
        cells: vec![" ".to_string(); usize::from(width.max(1))],
        continues_previous: false,
        copy_offset: 0,
    }
}

fn lifecycle_selection_snapshot(
    surface: super::ui_lifecycle::LifecycleSelectionSurface,
) -> Option<TranscriptSelectionSnapshot> {
    let width = usize::from(surface.viewport.width.max(1));
    let height = usize::from(surface.viewport.height);
    if height == 0 {
        return None;
    }

    let mut rows = vec![
        TranscriptSelectionRow {
            cells: vec![" ".to_string(); width],
            continues_previous: false,
            copy_offset: 0,
        };
        height
    ];

    for text in surface.text_rows {
        let rendered_rows = aligned_selection_rows_for_line(&text.line, width, text.alignment);
        let max_height = usize::from(text.max_height).min(rendered_rows.len());
        for (offset, mut row) in rendered_rows.into_iter().take(max_height).enumerate() {
            let target = text.row.saturating_add(offset);
            if target >= rows.len() {
                break;
            }
            row.continues_previous = offset > 0;
            rows[target] = row;
        }
    }

    Some(TranscriptSelectionSnapshot {
        viewport: surface.viewport,
        scroll_top: 0,
        rows,
    })
}

fn aligned_selection_rows_for_line(
    line: &Line<'static>,
    width: usize,
    alignment: Alignment,
) -> Vec<TranscriptSelectionRow> {
    let mut rows = transcript_selection_line_rows(line, width.max(1));
    let mut copy_offsets = vec![0; rows.len()];
    if !matches!(alignment, Alignment::Left) {
        for (idx, cells) in rows.iter_mut().enumerate() {
            copy_offsets[idx] = align_selection_cells(cells, alignment);
        }
    }

    rows.into_iter()
        .enumerate()
        .map(|(idx, cells)| TranscriptSelectionRow {
            cells,
            continues_previous: idx > 0,
            copy_offset: copy_offsets[idx],
        })
        .collect()
}

fn align_selection_cells(cells: &mut Vec<String>, alignment: Alignment) -> usize {
    let width = cells.len();
    let content_end = cells
        .iter()
        .enumerate()
        .rev()
        .find(|(_, cell)| !cell.is_empty() && cell.as_str() != " ")
        .map(|(idx, _)| idx.saturating_add(1))
        .unwrap_or(0);
    if content_end == 0 || content_end >= width {
        return 0;
    }

    let leading = match alignment {
        Alignment::Center => width.saturating_sub(content_end) / 2,
        Alignment::Right => width.saturating_sub(content_end),
        Alignment::Left => 0,
    };
    if leading == 0 {
        return 0;
    }

    let mut shifted = vec![" ".to_string(); width];
    let copy_len = content_end.min(width.saturating_sub(leading));
    shifted[leading..leading + copy_len].clone_from_slice(&cells[..copy_len]);
    *cells = shifted;
    leading
}

fn render_transcript_selection(
    frame: &mut Frame,
    selection: Option<TranscriptSelection>,
    snapshot: Option<&TranscriptSelectionSnapshot>,
    area: Rect,
    theme: &Theme,
) {
    let Some(selection) = selection else {
        return;
    };
    let Some(snapshot) = snapshot else {
        return;
    };
    if snapshot.rows.is_empty() {
        return;
    }

    let (start, end) = selection.normalized();
    if start == end {
        return;
    }

    let visible_height = usize::from(area.height);
    let buffer = frame.buffer_mut();
    let max_row = snapshot.rows.len().saturating_sub(1);
    let start_row = start.row.min(max_row);
    let end_row = end.row.min(max_row);

    for local_row in 0..visible_height {
        let absolute_row = snapshot.scroll_top.saturating_add(local_row);
        if absolute_row < start_row || absolute_row > end_row {
            continue;
        }

        let row = &snapshot.rows[absolute_row];
        let row_start = if absolute_row == start_row {
            start.column.min(row.cells.len().saturating_sub(1))
        } else {
            0
        };
        let row_end = if absolute_row == end_row {
            end.column.min(row.cells.len().saturating_sub(1))
        } else {
            row.cells.len().saturating_sub(1)
        };
        if row_start > row_end {
            continue;
        }

        let y = area
            .y
            .saturating_add(u16::try_from(local_row).unwrap_or(u16::MAX));
        for column in row_start..=row_end {
            let x = area
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX));
            if x >= area.right() || y >= area.bottom() {
                continue;
            }

            let cell = &mut buffer[(x, y)];
            cell.set_fg(theme.text.inverse);
            cell.set_bg(theme.status.info);
        }
    }
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
        let layout = measure_transcript_layout(&sections, theme, width, base_surface);

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

fn build_transcript_sections(app: &AppState) -> Vec<TranscriptSection> {
    let mut sections = Vec::new();
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
            profile_label: app.active_profile(),
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

    for turn in turn_sections {
        sections.push(TranscriptSection::Turn(Box::new(turn)));
    }

    if app.active_permission_view().is_none() {
        for (_permission_id, summary) in app.transcript_pending_permissions() {
            sections.push(TranscriptSection::PendingPermission(
                TranscriptPendingPermissionSection { summary },
            ));
        }
    }

    sections
}

fn hidden_delegated_child_request_ids(app: &AppState) -> BTreeSet<&str> {
    app.activities
        .iter()
        .flat_map(|activity| activity.tool_calls.iter())
        .filter(|tool_call| {
            matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
                || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
        })
        .filter_map(task_tool_child_request_id)
        .collect()
}

fn task_tool_child_request_id(tool_call: &crate::app::ToolCallEntry) -> Option<&str> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_request_id.as_deref())
        .and_then(non_empty_trimmed)
        .or_else(|| {
            tool_json_string_ref(
                tool_call.output_json.as_ref(),
                &["child_request_id", "request_id"],
            )
        })
}

fn turn_supports_assistant_footer(turn: &TranscriptTurnSection) -> bool {
    !turn.assistant_parts.is_empty() || activity_status_supports_footer_only(turn.header.status)
}

fn activity_status_supports_footer_only(status: ActivityStatus) -> bool {
    matches!(status, ActivityStatus::Streaming)
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
        profile_label,
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
            profile_label: profile_label.to_string(),
            model_id: activity.model_id.clone(),
            duration_ms: activity.duration_ms(),
        },
        body_blocks,
        tool_calls,
        thinking,
        error,
        assistant_parts,
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
                .map(|tool_call| TranscriptAssistantPart::ToolCall(tool_call.section)),
        );
        if let Some(error) = error {
            fallback_parts.push(TranscriptAssistantPart::Error(error));
        }
        return fallback_parts;
    }

    sync_reasoning_parts_with_activity(&mut event_parts, activity, thinking_visible);

    if let Some(error) = error {
        event_parts.push(TranscriptAssistantPart::Error(error));
    }
    event_parts
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

fn turn_event_matches_activity(
    event: &harness_core::event::EventEnvelopeV1,
    request_id: &str,
) -> bool {
    match &event.payload {
        harness_core::event::EventV1::ProviderReasoningDelta(data) => {
            provider_event_matches_activity(event, &data.request_id, request_id)
        }
        harness_core::event::EventV1::ProviderStreamDelta(data) => {
            provider_event_matches_activity(event, &data.request_id, request_id)
        }
        harness_core::event::EventV1::TaskCompleted(_)
        | harness_core::event::EventV1::ToolCallRequested(_) => {
            event.correlation_id.as_deref() == Some(request_id)
        }
        _ => false,
    }
}

fn provider_event_matches_activity(
    event: &harness_core::event::EventEnvelopeV1,
    provider_request_id: &str,
    activity_request_id: &str,
) -> bool {
    provider_request_id == activity_request_id
        || event.correlation_id.as_deref() == Some(activity_request_id)
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
                        part: TranscriptAssistantPart::ToolCall(tool_call.section),
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
            part: TranscriptAssistantPart::ToolCall(tool_call.section),
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

fn activity_has_thinking_text(activity: &ActivityEntry) -> bool {
    has_trimmed_content(&activity.thinking_text)
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
            build_agent_spawn_tool_row(tool_call, task_row, &mut detail_blocks)
        }
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
        "apply_patch" => apply_patch_tool_header_metadata(tool_call).map(|metadata| {
            header_path_metadata = metadata.parent.clone();
            metadata.leaf
        }),
        _ => None,
    };

    TranscriptToolCallSection {
        tool_call_id: tool_call.tool_call_id.clone(),
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

fn question_tool_subtitle(question_answers: &[TranscriptQuestionAnswerItem]) -> Option<String> {
    let count = question_answers.len();
    if count == 0 {
        return None;
    }
    Some(if count == 1 {
        "answered 1 question".to_string()
    } else {
        format!("answered {count} questions")
    })
}

fn todo_tool_title(items: &[TranscriptTodoItem]) -> String {
    if items.is_empty() {
        return "Todos".to_string();
    }
    let done = items
        .iter()
        .filter(|item| item.status == TranscriptTodoStatus::Completed)
        .count();
    format!("{done} of {} todos completed", items.len())
}

fn todo_items_from_tool_call(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Vec<TranscriptTodoItem> {
    todo_items_from_value(tool_call.output_json.as_ref())
        .or_else(|| todo_items_from_artifacts(tool_call, session_path))
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| todo_items_from_value(Some(&value)))
        })
        .unwrap_or_default()
}

fn todo_items_from_artifacts(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<Vec<TranscriptTodoItem>> {
    let session_path = session_path?;
    tool_call.artifact_refs.iter().find_map(|artifact| {
        if !(artifact.path.ends_with(".json") || artifact.path.ends_with(".txt")) {
            return None;
        }
        let path = Path::new(&artifact.path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return None;
        }
        std::fs::read_to_string(session_path.join(path))
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|value| todo_items_from_value(Some(&value)))
    })
}

fn todo_items_from_value(value: Option<&serde_json::Value>) -> Option<Vec<TranscriptTodoItem>> {
    let todos = todo_array_from_value(value?)?;
    let items = todos
        .iter()
        .filter_map(|todo| {
            let content = todo
                .get("content")
                .or_else(|| todo.get("text"))
                .or_else(|| todo.get("title"))
                .and_then(serde_json::Value::as_str)
                .map(collapse_inline_whitespace)
                .filter(|content| !content.is_empty())?;
            let status = todo
                .get("status")
                .or_else(|| todo.get("state"))
                .and_then(serde_json::Value::as_str)
                .map(TranscriptTodoStatus::from_value)
                .or_else(|| {
                    todo.get("done")
                        .and_then(serde_json::Value::as_bool)
                        .map(|done| {
                            if done {
                                TranscriptTodoStatus::Completed
                            } else {
                                TranscriptTodoStatus::Pending
                            }
                        })
                })
                .unwrap_or(TranscriptTodoStatus::Pending);
            Some(TranscriptTodoItem { content, status })
        })
        .collect::<Vec<_>>();
    (!items.is_empty()).then_some(items)
}

fn todo_array_from_value(value: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    value
        .get("todos")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .or_else(|| {
            value
                .get("structured_output")
                .and_then(todo_array_from_value)
        })
}

impl TranscriptTodoStatus {
    fn from_value(value: &str) -> Self {
        match value {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }

    fn checkbox_glyph(self) -> &'static str {
        match self {
            Self::Completed | Self::Cancelled => "[✓]",
            Self::InProgress => "[•]",
            Self::Pending => "[ ]",
        }
    }

    fn style(self, theme: &Theme) -> Style {
        match self {
            Self::InProgress => Style::default().fg(theme.status.warning),
            Self::Completed | Self::Cancelled | Self::Pending => {
                Style::default().fg(theme.text.secondary)
            }
        }
    }

    fn content_style(self, theme: &Theme) -> Style {
        let style = self.style(theme);
        if matches!(self, Self::Completed | Self::Cancelled) {
            style.add_modifier(Modifier::CROSSED_OUT)
        } else {
            style
        }
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

fn resolved_question_answer_items(
    tool_call: &crate::app::ToolCallEntry,
) -> Vec<TranscriptQuestionAnswerItem> {
    tool_call
        .permissions
        .iter()
        .flat_map(question_answer_items_from_permission)
        .collect()
}

fn question_answer_items_from_permission(
    permission: &crate::app::PermissionEntry,
) -> Vec<TranscriptQuestionAnswerItem> {
    if permission.resolved_decision != Some(harness_core::event::PermissionDecision::Allow)
        || !question_permission_kind(&permission.kind)
    {
        return Vec::new();
    }

    let questions = question_prompt_texts(&permission.summary);
    if questions.is_empty() {
        return Vec::new();
    }

    let answers = permission
        .resolution_reason
        .as_deref()
        .and_then(|reason| serde_json::from_str::<Vec<Vec<String>>>(reason).ok())
        .unwrap_or_default();

    questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| TranscriptQuestionAnswerItem {
            question,
            answer: answers
                .get(index)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.trim())
                        .filter(|item| !item.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|answer| !answer.is_empty())
                .unwrap_or_else(|| "(no answer)".to_string()),
        })
        .collect()
}

fn question_permission_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("question")
        || kind.eq_ignore_ascii_case("ask")
        || kind.eq_ignore_ascii_case("ask_user")
}

fn question_prompt_texts(summary: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|value| {
            value
                .get("questions")
                .and_then(serde_json::Value::as_array)
                .cloned()
        })
        .map(|questions| {
            questions
                .into_iter()
                .filter_map(|question| {
                    question
                        .get("question")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
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

fn tool_call_should_remain_visible_without_tool_details(
    tool_call: &crate::app::ToolCallEntry,
) -> bool {
    if todo_write_tool_id(tool_call.effective_tool_id()) || todo_write_tool_id(&tool_call.tool_id) {
        return true;
    }

    if matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
        || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
    {
        return true;
    }

    matches!(
        tool_call.effective_tool_id(),
        "edit.hashline_apply" | "fs.write" | "edit" | "apply_patch"
    ) && tool_call_has_preview_content(tool_call)
}

fn tool_call_has_diff_preview(tool_call: &crate::app::ToolCallEntry) -> bool {
    !tool_call_diff_artifacts(tool_call).is_empty()
        || tool_call_inline_diff_block(tool_call).is_some()
}

fn tool_call_has_preview_content(tool_call: &crate::app::ToolCallEntry) -> bool {
    tool_call_has_diff_preview(tool_call)
        || tool_call_apply_patch_file_rows(tool_call).is_some_and(|rows| !rows.is_empty())
}

fn apply_patch_tool_header_metadata(
    tool_call: &crate::app::ToolCallEntry,
) -> Option<TranscriptPathMetadata> {
    let count = apply_patch_file_count(tool_call)?;
    if count == 1 {
        return tool_call_path_metadata(first_apply_patch_path(tool_call).as_deref()).or_else(
            || {
                Some(TranscriptPathMetadata {
                    leaf: "1 file".to_string(),
                    parent: None,
                })
            },
        );
    }

    Some(TranscriptPathMetadata {
        leaf: format!("{count} files"),
        parent: None,
    })
}

fn join_tool_subtitles(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    match (primary, secondary) {
        (Some(primary), Some(secondary)) => Some(format!("{primary} · {secondary}")),
        (Some(primary), None) => Some(primary),
        (None, Some(secondary)) => Some(secondary),
        (None, None) => None,
    }
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

fn collect_apply_patch_file_render_entries(
    tool_call: &crate::app::ToolCallEntry,
) -> Vec<ApplyPatchFileRenderEntry> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();

    if let Some(edits) = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("edits"))
        .and_then(serde_json::Value::as_array)
    {
        for edit in edits {
            let Some(file_path) = edit
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
            else {
                continue;
            };
            if !seen.insert(file_path.clone()) {
                continue;
            }
            let diff_rel_path = edit
                .get("diff_rel_path")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string);
            entries.push(ApplyPatchFileRenderEntry {
                file_path,
                diff_rel_path,
            });
        }
    }

    if entries.is_empty() {
        let diff_artifacts = tool_call_diff_artifacts(tool_call);
        let diff_paths = diff_artifacts
            .into_iter()
            .filter_map(|(diff_rel_path, fallback_path)| {
                fallback_path.map(|file_path| ApplyPatchFileRenderEntry {
                    file_path,
                    diff_rel_path: Some(diff_rel_path),
                })
            })
            .collect::<Vec<_>>();
        for entry in diff_paths {
            if seen.insert(entry.file_path.clone()) {
                entries.push(entry);
            }
        }
    }

    if entries.is_empty() {
        let diff_rel_paths = tool_call_diff_artifacts(tool_call)
            .into_iter()
            .map(|(diff_rel_path, _)| diff_rel_path)
            .collect::<Vec<_>>();
        if let Some(rows) = tool_call_apply_patch_file_rows(tool_call) {
            for (index, row) in rows.into_iter().enumerate() {
                let Some(file_path) = normalize_apply_patch_file_row(&row) else {
                    continue;
                };
                if !seen.insert(file_path.clone()) {
                    continue;
                }
                entries.push(ApplyPatchFileRenderEntry {
                    file_path,
                    diff_rel_path: diff_rel_paths.get(index).cloned(),
                });
            }
        }
    }

    entries
}

fn apply_patch_file_count(tool_call: &crate::app::ToolCallEntry) -> Option<usize> {
    tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("files"))
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .or_else(|| {
            let count = collect_apply_patch_file_render_entries(tool_call).len();
            (count > 0).then_some(count)
        })
}

fn first_apply_patch_path(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    collect_apply_patch_file_render_entries(tool_call)
        .into_iter()
        .next()
        .map(|entry| entry.file_path)
}

fn normalize_apply_patch_file_row(row: &str) -> Option<String> {
    let trimmed = row.trim();
    if trimmed.is_empty() {
        return None;
    }

    let remainder = trimmed
        .split_once(' ')
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
        .unwrap_or(trimmed);
    Some(remainder.to_string())
}

fn tool_call_path_metadata(path: Option<&str>) -> Option<TranscriptPathMetadata> {
    let path = path?.trim();
    if path.is_empty() {
        return None;
    }

    let path = collapse_inline_whitespace(path);
    let (parent, leaf) = path
        .rsplit_once('/')
        .map(|(parent, leaf)| (Some(parent.to_string()), leaf.to_string()))
        .unwrap_or((None, path));
    Some(TranscriptPathMetadata { leaf, parent })
}

fn tool_call_inline_diff_block(
    tool_call: &crate::app::ToolCallEntry,
) -> Option<(String, Option<String>)> {
    let args = serde_json::from_str::<serde_json::Value>(&tool_call.args_summary).ok()?;
    let object = args.as_object()?;

    match tool_call.effective_tool_id() {
        "edit" => {
            let path = object
                .get("filePath")
                .or_else(|| object.get("path"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())?
                .to_string();
            let before = object
                .get("oldString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let after = object
                .get("newString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some((basic_unified_diff(&path, before, after), Some(path)))
        }
        "fs.write" => {
            let path = object
                .get("filePath")
                .or_else(|| object.get("path"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|path| !path.is_empty())?
                .to_string();
            let after = object
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some((basic_unified_diff(&path, "", after), Some(path)))
        }
        _ => None,
    }
}

fn tool_call_apply_patch_file_rows(tool_call: &crate::app::ToolCallEntry) -> Option<Vec<String>> {
    let files = tool_call
        .output_json
        .as_ref()?
        .get("files")
        .and_then(serde_json::Value::as_array)?;
    let rows = files
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(rows)
}

fn basic_unified_diff(path: &str, before: &str, after: &str) -> String {
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let before_count = before_lines.len();
    let after_count = after_lines.len();
    let before_start = if before_count == 0 { 0 } else { 1 };
    let after_start = if after_count == 0 { 0 } else { 1 };

    let mut diff = String::new();
    diff.push_str(&format!("--- {path}\n"));
    diff.push_str(&format!("+++ {path}\n"));
    diff.push_str(&format!(
        "@@ -{before_start},{before_count} +{after_start},{after_count} @@\n"
    ));
    for line in before_lines {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in after_lines {
        diff.push_str(&format!("+{line}\n"));
    }
    diff
}

fn tool_call_diff_artifacts(
    tool_call: &crate::app::ToolCallEntry,
) -> Vec<(String, Option<String>)> {
    let mut seen = BTreeSet::new();
    let mut diffs = Vec::new();

    if let Some(edit) = tool_call.edit.as_ref() {
        if let Some(diff_rel_path) = edit.diff_rel_path.as_deref() {
            if seen.insert(diff_rel_path.to_string()) {
                diffs.push((diff_rel_path.to_string(), Some(edit.path.clone())));
            }
        }
    }

    if let Some(edits) = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("edits"))
        .and_then(serde_json::Value::as_array)
    {
        for edit in edits {
            let Some(diff_rel_path) = edit
                .get("diff_rel_path")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let diff_rel_path = diff_rel_path.trim();
            if diff_rel_path.is_empty() || !seen.insert(diff_rel_path.to_string()) {
                continue;
            }
            let fallback_path = edit
                .get("path")
                .and_then(serde_json::Value::as_str)
                .and_then(non_empty_trimmed)
                .map(str::to_string);
            diffs.push((diff_rel_path.to_string(), fallback_path));
        }
    }

    let default_fallback_path = tool_call.edit_path_display();
    for artifact in &tool_call.artifact_refs {
        if !artifact.path.ends_with(".diff") || !seen.insert(artifact.path.clone()) {
            continue;
        }
        diffs.push((artifact.path.clone(), default_fallback_path.clone()));
    }

    diffs
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

fn generic_tool_title(tool_call: &crate::app::ToolCallEntry, tool_id: &str) -> String {
    let suffix = compact_tool_trigger_subtitle(
        tool_input_label(&tool_call.args_summary, false),
        tool_input_args(&tool_call.args_summary, false, &[]),
    );
    match suffix {
        Some(suffix) => format!("{} {}", generic_tool_name(tool_id), suffix),
        None => generic_tool_name(tool_id),
    }
}

fn tool_hidden_from_transcript(tool_call: &crate::app::ToolCallEntry) -> bool {
    matches!(tool_call.effective_tool_id(), "todo.read" | "todoread")
        || matches!(tool_call.tool_id.as_str(), "todoread")
}

fn todo_write_tool_id(tool_id: &str) -> bool {
    matches!(tool_id, "todo.write" | "todowrite")
}

fn generic_tool_visual_style(
    tool_call: &crate::app::ToolCallEntry,
    generic_output_visible: bool,
) -> TranscriptToolCallVisualStyle {
    if tool_call.status == ToolCallDisplayStatus::Failed {
        return TranscriptToolCallVisualStyle::Block;
    }

    if generic_output_visible && tool_call.output_summary.is_some() {
        TranscriptToolCallVisualStyle::Block
    } else {
        TranscriptToolCallVisualStyle::Inline
    }
}

fn generic_tool_name(tool_id: &str) -> String {
    tool_id.trim().to_string()
}

fn batch_tool_title(tool_call: &crate::app::ToolCallEntry) -> String {
    let count = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("requested_call_count"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("processed_call_count"))
                .and_then(serde_json::Value::as_u64)
        });

    count
        .map(|count| {
            format!(
                "Run batch · {count} tool{}",
                if count == 1 { "" } else { "s" }
            )
        })
        .unwrap_or_else(|| "Run batch".to_string())
}

fn mcp_tool_title(tool_call: &crate::app::ToolCallEntry, display_tool_id: &str) -> String {
    let title = mcp_display_name(tool_call, display_tool_id);
    let suffix = compact_tool_trigger_subtitle(
        tool_input_label(&tool_call.args_summary, true),
        tool_input_args(&tool_call.args_summary, true, &["tool"]),
    );
    match suffix {
        Some(suffix) => format!("{title} {suffix}"),
        None => title,
    }
}

fn mcp_display_name(tool_call: &crate::app::ToolCallEntry, display_tool_id: &str) -> String {
    let server = mcp_server_name(tool_call, display_tool_id);
    if let Some(tool) = mcp_remote_tool_name(tool_call, display_tool_id) {
        return server
            .map(|server| format!("{server}_{tool}"))
            .unwrap_or(tool);
    }

    let fallback = mcp_tool_suffix(display_tool_id)
        .map(|suffix| suffix.replace('.', "_"))
        .or_else(|| mcp_tool_id_body(display_tool_id).map(str::to_string))
        .unwrap_or_else(|| display_tool_id.to_string());
    server
        .map(|server| format!("{server}_{fallback}"))
        .unwrap_or(fallback)
}

fn mcp_server_name(tool_call: &crate::app::ToolCallEntry, display_tool_id: &str) -> Option<String> {
    tool_json_nested_string(tool_call.output_json.as_ref(), &["server", "id"]).or_else(|| {
        mcp_tool_id_parts(display_tool_id)
            .map(|(server, _)| collapse_inline_whitespace(server))
            .filter(|server| !server.is_empty())
    })
}

fn mcp_tool_suffix(display_tool_id: &str) -> Option<&str> {
    mcp_tool_id_parts(display_tool_id).map(|(_, suffix)| suffix)
}

fn is_mcp_tool_id(tool_id: &str) -> bool {
    mcp_tool_id_body(tool_id).is_some()
}

fn mcp_tool_id_body(tool_id: &str) -> Option<&str> {
    tool_id.strip_prefix("mcp.")
}

fn mcp_tool_id_parts(tool_id: &str) -> Option<(&str, &str)> {
    mcp_tool_id_body(tool_id)?.split_once('.')
}

fn mcp_remote_tool_name(
    tool_call: &crate::app::ToolCallEntry,
    display_tool_id: &str,
) -> Option<String> {
    tool_json_nested_string(tool_call.output_json.as_ref(), &["payload", "tool"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["tool"]))
        .or_else(|| {
            let suffix = mcp_tool_suffix(display_tool_id)?;
            (!matches!(
                suffix,
                "tools.list"
                    | "tool.call"
                    | "resources.list"
                    | "resource.read"
                    | "prompts.list"
                    | "prompt.get"
            ))
            .then(|| suffix.rsplit('.').next().unwrap_or(suffix).to_string())
        })
        .map(|value| collapse_inline_whitespace(&value))
        .filter(|value| !value.is_empty())
}

fn build_agent_spawn_tool_row(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
) -> (
    String,
    Option<&'static str>,
    TranscriptToolCallVisualStyle,
    bool,
) {
    let title = agent_spawn_title(tool_call);
    if let Some(line) = agent_spawn_context_line(tool_call, task_row) {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: line,
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
    (
        title,
        Some("│"),
        TranscriptToolCallVisualStyle::TaskInline,
        false,
    )
}

fn agent_spawn_title(tool_call: &crate::app::ToolCallEntry) -> String {
    let description = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("description"))
        .and_then(serde_json::Value::as_str)
        .map(collapse_inline_whitespace)
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["description", "task"]));
    let profile = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("profile"))
        .and_then(serde_json::Value::as_str)
        .map(collapse_inline_whitespace)
        .or_else(|| {
            tool_summary_string(
                &tool_call.args_summary,
                &["profile_name", "profile", "subagent_type"],
            )
        });

    let prefix = format!(
        "{} Task",
        subagent_profile_label(profile.as_deref().unwrap_or("General"))
    );

    match description {
        Some(description) => format!("{prefix} — {description}"),
        None => prefix,
    }
}

fn subagent_profile_label(profile: &str) -> String {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        return "General".to_string();
    }

    let mut label = String::with_capacity(trimmed.len());
    let mut previous_was_word = false;
    for ch in trimmed.chars() {
        let is_word = ch.is_ascii_alphanumeric() || ch == '_';
        if is_word && !previous_was_word {
            label.extend(ch.to_uppercase());
        } else {
            label.push(ch);
        }
        previous_was_word = is_word;
    }
    label
}

fn agent_spawn_context_line(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
) -> Option<String> {
    let status = task_row
        .map(|row| orchestration_task_state_label(row.state).to_string())
        .or_else(|| tool_json_string(tool_call.output_json.as_ref(), &["status"]));
    let completed = matches!(tool_call.status, ToolCallDisplayStatus::Succeeded)
        || status.as_deref() == Some("completed");
    let running = matches!(tool_call.status, ToolCallDisplayStatus::Running)
        || status.as_deref() == Some("running");
    let child_tool_call_count = task_row
        .map(|row| row.child_tool_call_count as u64)
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("child_tool_call_count"))
                .and_then(serde_json::Value::as_u64)
        });
    let resumed_existing_session = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("resumed_existing_session"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let duration_ms = task_row
        .and_then(crate::app::OrchestrationTaskRow::duration_ms)
        .or_else(|| tool_call.duration_ms())
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| value.get("duration_ms"))
                .and_then(serde_json::Value::as_u64)
        });

    if completed {
        let mut parts = vec![format!(
            "{} toolcalls",
            child_tool_call_count.unwrap_or_default()
        )];
        parts.push(format_duration_ms(duration_ms.unwrap_or_default()));
        if resumed_existing_session {
            parts.push("resumed session".to_string());
        }
        return Some(format!("└ {}", parts.join(" · ")));
    }

    if running {
        if let Some(current_tool_title) = task_row
            .and_then(|row| row.current_child_tool_title.as_deref())
            .and_then(collapsed_inline_non_empty)
        {
            return Some(format!("↳ {current_tool_title}"));
        }
        if let Some(count) = child_tool_call_count.filter(|count| *count > 0) {
            return Some(format!("↳ {count} toolcalls"));
        }
    }

    let mut parts = Vec::new();
    if resumed_existing_session {
        parts.push("resumed session".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn orchestration_task_state_label(state: crate::app::OrchestrationTaskState) -> &'static str {
    match state {
        crate::app::OrchestrationTaskState::Queued => "queued",
        crate::app::OrchestrationTaskState::Running => "running",
        crate::app::OrchestrationTaskState::Stale => "stale",
        crate::app::OrchestrationTaskState::Completed => "completed",
        crate::app::OrchestrationTaskState::Cancelled => "cancelled",
        crate::app::OrchestrationTaskState::LateResult => "late result",
    }
}

fn tool_json_string(output_json: Option<&serde_json::Value>, keys: &[&str]) -> Option<String> {
    let object = output_json?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(collapsed_inline_non_empty)
    })
}

fn tool_json_string_ref<'a>(
    output_json: Option<&'a serde_json::Value>,
    keys: &[&str],
) -> Option<&'a str> {
    let object = output_json?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(non_empty_trimmed)
    })
}

fn tool_json_nested_string(
    output_json: Option<&serde_json::Value>,
    path: &[&str],
) -> Option<String> {
    let mut current = output_json?;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().and_then(collapsed_inline_non_empty)
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

fn tool_disclosure_state(
    tool_call: &crate::app::ToolCallEntry,
    tool_output_expanded: bool,
) -> Option<TranscriptToolCallDisclosureState> {
    tool_call_has_transcript_disclosure(tool_call).then_some(if tool_output_expanded {
        TranscriptToolCallDisclosureState::Expanded
    } else {
        TranscriptToolCallDisclosureState::Collapsed
    })
}

fn tool_call_has_transcript_disclosure(tool_call: &crate::app::ToolCallEntry) -> bool {
    if tool_call.effective_tool_id() == "apply_patch" {
        return false;
    }

    if matches!(
        tool_call.effective_tool_id(),
        "fs.read" | "read" | "fs.glob" | "glob" | "fs.grep" | "grep" | "fs.ls" | "list"
    ) {
        return true;
    }

    if tool_output_hidden_behind_disclosure_by_default(tool_call) {
        return true;
    }

    let shell_output = shell_tool_output(tool_call);
    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    let output_line_count = output.lines().count();
    !tool_call.artifact_refs.is_empty()
        || match tool_call.effective_tool_id() {
            "shell.run" | "bash" => shell_output
                .as_deref()
                .or(tool_call.output_summary.as_deref())
                .is_some_and(has_trimmed_content),
            "edit.hashline_apply" | "fs.write" | "edit" => tool_call_has_preview_content(tool_call),
            "agent.spawn" | "task" => true,
            _ => has_trimmed_content(output) && output_line_count > 3,
        }
}

fn tool_output_hidden_behind_disclosure_by_default(tool_call: &crate::app::ToolCallEntry) -> bool {
    tool_call.status == ToolCallDisplayStatus::Succeeded
        && is_mcp_tool_id(tool_call.effective_tool_id())
        && tool_call
            .output_summary
            .as_deref()
            .is_some_and(has_trimmed_content)
}

fn tool_path_display(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_call
        .edit_path_display()
        .map(|path| collapse_inline_whitespace(&path))
}

fn tool_in_path_suffix(tool_call: &crate::app::ToolCallEntry) -> String {
    tool_path_display(tool_call)
        .filter(|path| path != ".")
        .map(|path| format!(" in {path}"))
        .unwrap_or_default()
}

fn shell_tool_subtitle(args_summary: &str) -> Option<String> {
    tool_summary_string(args_summary, &["description"])
        .filter(|value| !value.is_empty() && value != "Shell")
}

fn shell_tool_command(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    trimmed_json_string_field(tool_call.output_json.as_ref(), &["command"])
        .or_else(|| shell_tool_command_from_value(tool_call.output_json.as_ref()))
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| {
                    trimmed_json_string_field(Some(&value), &["command"])
                        .or_else(|| shell_tool_command_from_value(Some(&value)))
                })
        })
}

fn shell_tool_command_from_value(value: Option<&serde_json::Value>) -> Option<String> {
    let cmd = trimmed_json_string_field(value, &["cmd"])?;
    let args = value
        .and_then(|value| value.get("args"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if args.is_empty() {
        Some(cmd)
    } else {
        Some(format!("{cmd} {}", args.join(" ")))
    }
}

fn shell_tool_title_description(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<String> {
    let explicit_description = shell_tool_subtitle(&tool_call.args_summary);
    let description = explicit_description
        .clone()
        .unwrap_or_else(|| "Shell".to_string());
    let Some(workdir) = shell_tool_workdir_display(tool_call, session_path) else {
        return explicit_description;
    };
    if description.contains(&workdir) {
        Some(description)
    } else {
        Some(format!("{description} in {workdir}"))
    }
}

fn shell_tool_workdir_display(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<String> {
    let workdir = trimmed_json_string_field(tool_call.output_json.as_ref(), &["workdir", "cwd"])
        .or_else(|| {
            serde_json::from_str::<serde_json::Value>(&tool_call.args_summary)
                .ok()
                .and_then(|value| trimmed_json_string_field(Some(&value), &["workdir", "cwd"]))
        })?;
    if workdir == "." {
        return None;
    }

    let base = session_path?;
    let absolute = if Path::new(&workdir).is_absolute() {
        PathBuf::from(&workdir)
    } else {
        base.join(&workdir)
    };
    if absolute == base {
        return None;
    }

    Some(home_collapsed_path_display(&absolute))
}

fn home_collapsed_path_display(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
        return path.display().to_string();
    };
    let home = PathBuf::from(home);
    if path == home {
        return "~".to_string();
    }
    path.strip_prefix(&home)
        .ok()
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|| path.display().to_string())
}

fn shell_tool_output(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    let structured = shell_tool_structured_output(tool_call.output_json.as_ref());
    if tool_call.status == ToolCallDisplayStatus::Failed {
        return structured;
    }
    structured.or_else(|| {
        tool_call
            .output_summary
            .as_deref()
            .map(strip_ansi_escapes)
            .map(|output| output.trim().to_string())
    })
}

fn shell_tool_structured_output(output_json: Option<&serde_json::Value>) -> Option<String> {
    let value = output_json?;
    let stdout = value.get("stdout").and_then(serde_json::Value::as_str);
    let stderr = value.get("stderr").and_then(serde_json::Value::as_str);
    let output = match (stdout, stderr) {
        (Some(stdout), Some(stderr)) if !stdout.is_empty() && !stderr.is_empty() => {
            format!("{stdout}\n{stderr}")
        }
        (Some(stdout), _) if !stdout.is_empty() => stdout.to_string(),
        (_, Some(stderr)) if !stderr.is_empty() => stderr.to_string(),
        (Some(stdout), _) => stdout.to_string(),
        (_, Some(stderr)) => stderr.to_string(),
        _ => return None,
    };
    let stripped = strip_ansi_escapes(&output);
    Some(stripped.trim().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiStripState {
    Text,
    Escape,
    Csi,
    Osc,
    OscEscape,
    StringControl,
    StringControlEscape,
}

fn strip_ansi_escapes(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut state = AnsiStripState::Text;
    for character in text.chars() {
        match state {
            AnsiStripState::Text => {
                if character == '\u{1b}' {
                    state = AnsiStripState::Escape;
                } else {
                    output.push(character);
                }
            }
            AnsiStripState::Escape => match character {
                '[' => state = AnsiStripState::Csi,
                ']' => state = AnsiStripState::Osc,
                'P' | '^' | '_' => state = AnsiStripState::StringControl,
                _ => state = AnsiStripState::Text,
            },
            AnsiStripState::Csi => {
                if ('@'..='~').contains(&character) {
                    state = AnsiStripState::Text;
                }
            }
            AnsiStripState::Osc => match character {
                '\u{7}' => state = AnsiStripState::Text,
                '\u{1b}' => state = AnsiStripState::OscEscape,
                _ => {}
            },
            AnsiStripState::OscEscape => {
                state = if character == '\\' {
                    AnsiStripState::Text
                } else {
                    AnsiStripState::Osc
                };
            }
            AnsiStripState::StringControl => {
                if character == '\u{1b}' {
                    state = AnsiStripState::StringControlEscape;
                }
            }
            AnsiStripState::StringControlEscape => {
                state = if character == '\\' {
                    AnsiStripState::Text
                } else {
                    AnsiStripState::StringControl
                };
            }
        }
    }
    output
}

fn tool_summary_string(args_summary: &str, keys: &[&str]) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    let object = value.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .map(collapse_inline_whitespace)
            .filter(|value| !value.is_empty())
    })
}

#[derive(Debug, Clone)]
enum OrderedToolInputValue {
    String(String),
    Number(String),
    Bool(bool),
    Null,
    Array(()),
    Object(Vec<(String, OrderedToolInputValue)>),
}

impl<'de> serde::Deserialize<'de> for OrderedToolInputValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OrderedToolInputValueVisitor;

        impl<'de> serde::de::Visitor<'de> for OrderedToolInputValueVisitor {
            type Value = OrderedToolInputValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("ordered JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Number(value.to_string()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Number(value.to_string()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Number(value.to_string()))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(OrderedToolInputValue::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::String(value))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Null)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Null)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                while seq.next_element::<OrderedToolInputValue>()?.is_some() {}
                Ok(OrderedToolInputValue::Array(()))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut fields = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, OrderedToolInputValue>()? {
                    fields.push((key, value));
                }
                Ok(OrderedToolInputValue::Object(fields))
            }
        }

        deserializer.deserialize_any(OrderedToolInputValueVisitor)
    }
}

fn compact_tool_input_part(key: &str, value: &OrderedToolInputValue) -> Option<String> {
    match value {
        OrderedToolInputValue::String(text) => {
            let rendered = collapse_inline_whitespace(text);
            (!rendered.is_empty()).then(|| format!("{key}={rendered}"))
        }
        OrderedToolInputValue::Number(number) => Some(format!("{key}={number}")),
        OrderedToolInputValue::Bool(flag) => Some(format!("{key}={flag}")),
        OrderedToolInputValue::Null => Some(format!("{key}=null")),
        OrderedToolInputValue::Array(_) | OrderedToolInputValue::Object(_) => None,
    }
}

fn tool_input_label(args_summary: &str, unwrap_arguments: bool) -> Option<String> {
    ordered_tool_input_display_value(args_summary, unwrap_arguments)
        .as_ref()
        .and_then(tool_input_label_from_value)
}

fn tool_input_args(args_summary: &str, unwrap_arguments: bool, omit_keys: &[&str]) -> Vec<String> {
    ordered_tool_input_display_value(args_summary, unwrap_arguments)
        .as_ref()
        .map(|value| tool_input_args_from_value(value, omit_keys, 3))
        .unwrap_or_default()
}

fn ordered_tool_input_display_value(
    args_summary: &str,
    unwrap_arguments: bool,
) -> Option<OrderedToolInputValue> {
    let value = serde_json::from_str::<OrderedToolInputValue>(args_summary).ok()?;
    if !unwrap_arguments {
        return Some(value);
    }

    match value {
        OrderedToolInputValue::Object(fields) => fields
            .iter()
            .find(|(key, _)| key == "arguments")
            .map(|(_, value)| value.clone())
            .or(Some(OrderedToolInputValue::Object(fields))),
        _ => Some(value),
    }
}

fn tool_input_label_from_value(value: &OrderedToolInputValue) -> Option<String> {
    let OrderedToolInputValue::Object(fields) = value else {
        return None;
    };

    [
        "description",
        "query",
        "url",
        "filePath",
        "path",
        "pattern",
        "name",
    ]
    .iter()
    .find_map(|key| {
        fields
            .iter()
            .find(|(field, _)| field == key)
            .and_then(|(_, value)| match value {
                OrderedToolInputValue::String(text) => {
                    let rendered = collapse_inline_whitespace(text);
                    (!rendered.is_empty()).then_some(rendered)
                }
                _ => None,
            })
    })
}

fn tool_input_args_from_value(
    value: &OrderedToolInputValue,
    omit_keys: &[&str],
    max_parts: usize,
) -> Vec<String> {
    let OrderedToolInputValue::Object(fields) = value else {
        return Vec::new();
    };
    let skip = [
        "description",
        "query",
        "url",
        "filePath",
        "path",
        "pattern",
        "name",
    ];

    let mut parts = Vec::new();
    for (key, value) in fields.iter() {
        if skip.contains(&key.as_str()) || omit_keys.contains(&key.as_str()) {
            continue;
        }
        if let Some(part) = compact_tool_input_part(key, value) {
            parts.push(part);
        }
        if parts.len() >= max_parts {
            break;
        }
    }
    parts
}

fn compact_tool_trigger_subtitle(label: Option<String>, args: Vec<String>) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(label) = label {
        parts.push(label);
    }
    parts.extend(args.into_iter().map(|arg| format!("[{arg}]")));
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn read_tool_input_suffix(tool_call: &crate::app::ToolCallEntry) -> String {
    let offset = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("offset"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_summary_number(&tool_call.args_summary, &["offset", "start_line"]));
    let limit = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("limit"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| tool_summary_number(&tool_call.args_summary, &["limit"]));
    let mut parts = Vec::new();
    if let Some(offset) = offset {
        parts.push(format!("offset={offset}"));
    }
    if let Some(limit) = limit {
        parts.push(format!("limit={limit}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(", "))
    }
}

fn tool_summary_number(args_summary: &str, keys: &[&str]) -> Option<u64> {
    let value = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(serde_json::Value::as_u64)
}

fn tool_match_count_suffix(tool_call: &crate::app::ToolCallEntry) -> String {
    let count = tool_call
        .output_json
        .as_ref()
        .and_then(|value| value.get("total_count"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            tool_call.output_summary.as_deref().map(|output| {
                output
                    .lines()
                    .filter(|line| has_trimmed_content(line))
                    .count() as u64
            })
        });

    count
        .map(|count| format!(" ({count} match{})", if count == 1 { "" } else { "es" }))
        .unwrap_or_default()
}

fn push_collapsible_output_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    output: &str,
    max_lines: usize,
    expanded: bool,
    tone: TranscriptToolCallDetailTone,
) {
    let formatted = format_detail_payload(output);
    let overflow = line_count_exceeds(&formatted, max_lines);
    if overflow && !expanded {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: first_lines_with_ellipsis(&formatted, max_lines),
            tone,
        });
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: "Click to expand".to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    } else {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: formatted,
            tone,
        });
        if overflow {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: "Click to collapse".to_string(),
                tone: TranscriptToolCallDetailTone::Secondary,
            });
        }
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
    let overflow = line_count_exceeds(output, max_lines);
    let rendered_output = if overflow && !expanded {
        first_lines_with_ellipsis(output, max_lines)
    } else {
        output.to_string()
    };

    detail_blocks.push(TranscriptToolCallDetailBlock::BashPanel {
        command: command.to_string(),
        output: rendered_output,
        description,
        expand_hint: overflow.then(|| {
            if expanded {
                "Click to collapse".to_string()
            } else {
                "Click to expand".to_string()
            }
        }),
        tone,
    });
}

fn line_count_exceeds(text: &str, max_lines: usize) -> bool {
    text.lines().nth(max_lines).is_some()
}

fn first_lines_with_ellipsis(text: &str, max_lines: usize) -> String {
    let mut preview = String::new();
    for (index, line) in text.lines().take(max_lines).enumerate() {
        if index > 0 {
            preview.push('\n');
        }
        preview.push_str(line);
    }
    preview.push_str("\n…");
    preview
}

fn push_failed_tool_error_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    let Some(error) = tool_error_text(tool_call) else {
        return;
    };
    if detail_blocks_surface_error(detail_blocks, &error) {
        return;
    }
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: error,
        tone: TranscriptToolCallDetailTone::Error,
    });
}

fn detail_blocks_surface_error(blocks: &[TranscriptToolCallDetailBlock], error: &str) -> bool {
    let error = error.trim();
    blocks.iter().any(|block| match block {
        TranscriptToolCallDetailBlock::Message { text, tone } => {
            *tone == TranscriptToolCallDetailTone::Error && text.trim() == error
        }
        TranscriptToolCallDetailBlock::BashPanel { output, .. } => output.trim() == error,
        TranscriptToolCallDetailBlock::FileSection(section) => {
            detail_blocks_surface_error(&section.detail_blocks, error)
        }
        TranscriptToolCallDetailBlock::TodoList { .. }
        | TranscriptToolCallDetailBlock::StructuredDiff { .. } => false,
    })
}

fn search_result_count_suffix(
    tool_call: &crate::app::ToolCallEntry,
    display_tool_id: &str,
) -> String {
    let count = tool_call
        .output_json
        .as_ref()
        .and_then(|value| {
            if display_tool_id == "search.web" {
                value.get("numResults")
            } else {
                value
                    .get("results")
                    .or_else(|| value.get("result_count"))
                    .or_else(|| value.get("numResults"))
            }
        })
        .and_then(serde_json::Value::as_u64);
    count
        .map(|count| format!(" ({count} result{})", if count == 1 { "" } else { "s" }))
        .unwrap_or_default()
}

fn tool_call_denied(tool_call: &crate::app::ToolCallEntry) -> bool {
    tool_call.permissions.iter().any(|permission| {
        permission.resolved_decision == Some(harness_core::event::PermissionDecision::Deny)
    })
}

const DEFAULT_TOOL_ERROR_BODY: &str = "No error details available.";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolErrorDisplay {
    subtitle: String,
    body: String,
}

fn tool_error_subtitle(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_error_display(tool_call).map(|display| display.subtitle)
}

fn tool_error_display(tool_call: &crate::app::ToolCallEntry) -> Option<ToolErrorDisplay> {
    if tool_call.status != ToolCallDisplayStatus::Failed {
        return None;
    }

    let formatted = raw_tool_error_text(tool_call)
        .map(|text| format_detail_payload(&text))
        .unwrap_or_default();
    let stripped_error = strip_prefix_ignore_ascii_case(formatted.trim(), "Error:")
        .map(str::trim)
        .unwrap_or_else(|| formatted.trim());
    let cleaned = strip_tool_error_prefix(tool_call, stripped_error);
    let default_subtitle = default_tool_error_subtitle(tool_call);

    if cleaned.is_empty() {
        return Some(ToolErrorDisplay {
            subtitle: default_subtitle.to_string(),
            body: DEFAULT_TOOL_ERROR_BODY.to_string(),
        });
    }

    if let Some((subtitle, body)) = split_tool_error_subtitle_and_body(&cleaned) {
        let subtitle = if tool_call_denied(tool_call) {
            default_subtitle.to_string()
        } else {
            subtitle
        };
        return Some(ToolErrorDisplay { subtitle, body });
    }

    Some(ToolErrorDisplay {
        subtitle: default_subtitle.to_string(),
        body: cleaned,
    })
}

fn default_tool_error_subtitle(tool_call: &crate::app::ToolCallEntry) -> &'static str {
    if tool_call_denied(tool_call) {
        "Denied"
    } else {
        "Failed"
    }
}

fn raw_tool_error_text(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_call
        .permissions
        .iter()
        .find_map(|permission| {
            (permission.resolved_decision == Some(harness_core::event::PermissionDecision::Deny))
                .then_some(permission.resolution_reason.as_deref())
                .flatten()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            tool_call
                .output_summary
                .as_deref()
                .map(str::trim)
                .filter(|summary| !summary.is_empty())
                .map(str::to_string)
        })
        .or_else(|| tool_json_nested_string(tool_call.output_json.as_ref(), &["error", "message"]))
        .or_else(|| {
            tool_json_string(
                tool_call.output_json.as_ref(),
                &[
                    "error",
                    "message",
                    "detail",
                    "reason",
                    "result_summary",
                    "summary",
                ],
            )
        })
        .or_else(|| {
            tool_call
                .output_json
                .as_ref()
                .and_then(|value| serde_json::to_string_pretty(value).ok())
        })
}

fn strip_tool_error_prefix(tool_call: &crate::app::ToolCallEntry, text: &str) -> String {
    let trimmed = text.trim();
    for prefix in tool_error_prefix_candidates(tool_call) {
        for separator in [" ", ": ", " - ", " — "] {
            let prefixed = format!("{prefix}{separator}");
            if let Some(stripped) = strip_prefix_ignore_ascii_case(trimmed, &prefixed) {
                return stripped.trim().to_string();
            }
        }
    }
    trimmed.to_string()
}

fn tool_error_prefix_candidates(tool_call: &crate::app::ToolCallEntry) -> Vec<String> {
    let mut candidates = BTreeSet::new();
    for candidate in [
        Some(tool_call.tool_id.as_str()),
        Some(tool_call.effective_tool_id()),
        Some(tool_call.invoked_tool_id()),
        tool_call.resolved_canonical_tool_id(),
        tool_call.resolved_alias_source_tool_id(),
    ]
    .into_iter()
    .flatten()
    {
        let normalized = collapse_inline_whitespace(candidate);
        if !normalized.is_empty() {
            candidates.insert(normalized);
        }
        for alias in tool_error_aliases(candidate) {
            candidates.insert(alias.to_string());
        }
    }
    candidates.into_iter().collect()
}

fn tool_error_aliases(tool_id: &str) -> &'static [&'static str] {
    match tool_id {
        "fs.read" | "read" => &["read"],
        "fs.ls" | "list" => &["list"],
        "fs.glob" | "glob" => &["glob"],
        "fs.grep" | "grep" => &["grep"],
        "agent.spawn" | "task" => &["task"],
        "web.fetch" | "webfetch" => &["webfetch"],
        "search.web" | "websearch" => &["websearch"],
        "search.code" | "codesearch" => &["codesearch"],
        "shell.run" | "bash" => &["bash"],
        "apply_patch" => &["apply_patch"],
        "user.question" | "question" => &["question"],
        _ => &[],
    }
}

fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

fn split_tool_error_subtitle_and_body(text: &str) -> Option<(String, String)> {
    let (subtitle, body) = text.split_once(": ")?;
    let subtitle = subtitle.trim();
    let body = body.trim();
    if !is_tool_error_label(subtitle) || body.is_empty() {
        return None;
    }

    Some((subtitle.to_string(), body.to_string()))
}

fn is_tool_error_label(subtitle: &str) -> bool {
    let lowered = subtitle.trim().to_ascii_lowercase();
    if lowered.is_empty()
        || lowered.contains('\n')
        || lowered.chars().count() > 48
        || !lowered
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        || lowered.chars().any(|character| {
            matches!(
                character,
                '/' | '\\' | '{' | '}' | '[' | ']' | '(' | ')' | '"' | '\'' | '='
            )
        })
    {
        return false;
    }

    [
        "failed",
        "error",
        "denied",
        "limited",
        "timeout",
        "dismissed",
        "not found",
        "invalid",
        "forbidden",
        "unauthorized",
        "unavailable",
        "missing",
        "blocked",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn tool_error_text(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_error_display(tool_call).map(|display| display.body)
}

fn collapse_inline_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapsed_inline_non_empty(text: &str) -> Option<String> {
    let collapsed = collapse_inline_whitespace(text);
    (!collapsed.is_empty()).then_some(collapsed)
}

fn measure_transcript_layout(
    sections: &[TranscriptSection],
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> MeasuredTranscriptLayout {
    let mut top_row = 0;
    let mut measured_sections = Vec::with_capacity(sections.len());

    for section in sections {
        let surfaces = build_transcript_render_surfaces(section, theme, width, base_surface);
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
            });
            previous_surface_kind = Some(surface.kind);
        }
        let leading_gap_height =
            usize::from(!measured_sections.is_empty()) * TRANSCRIPT_SECTION_GAP_HEIGHT;
        let kind = match section {
            TranscriptSection::Turn(_) => MeasuredTranscriptSectionKind::Turn,
            TranscriptSection::PendingPermission(_) => {
                MeasuredTranscriptSectionKind::PendingPermission
            }
        };

        let measured_section = MeasuredTranscriptSection {
            kind,
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

fn transcript_surface_leading_gap(
    previous: Option<TranscriptRenderSurfaceKind>,
    current: TranscriptRenderSurfaceKind,
) -> usize {
    match previous {
        Some(previous)
            if previous == TranscriptRenderSurfaceKind::AssistantTool
                && current == TranscriptRenderSurfaceKind::AssistantTool =>
        {
            0
        }
        Some(previous)
            if previous == TranscriptRenderSurfaceKind::AssistantBody
                && current == TranscriptRenderSurfaceKind::AssistantTool =>
        {
            0
        }
        Some(previous)
            if previous == TranscriptRenderSurfaceKind::AssistantReasoning
                && current == TranscriptRenderSurfaceKind::AssistantBody =>
        {
            0
        }
        Some(previous)
            if previous == TranscriptRenderSurfaceKind::AssistantTool
                && current == TranscriptRenderSurfaceKind::AssistantReasoning =>
        {
            0
        }
        Some(_) => 1,
        None => 0,
    }
}

fn transcript_layout_lines(layout: &MeasuredTranscriptLayout, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = layout.rendered_lines();

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Waiting for first turn…",
            Style::default().fg(theme.text.secondary),
        )));
    }

    lines
}

fn render_transcript_layout_surfaces(
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
            render_transcript_surface(frame, surface, surface_rect, local_scroll, theme);
        }
    }
}

fn render_transcript_scrollbar(
    frame: &mut Frame,
    viewport: TranscriptViewportLayout,
    scroll_top: usize,
    max_scroll: usize,
    theme: &Theme,
    base_surface: Color,
    drag_active: bool,
) {
    if let Some(chrome) = viewport.scrollbar_chrome {
        frame.render_widget(
            Block::default().style(Style::default().bg(base_surface)),
            chrome,
        );
    }

    let Some(scrollbar) = transcript_scrollbar_geometry(viewport, scroll_top, max_scroll) else {
        return;
    };

    let buffer = frame.buffer_mut();
    for y in scrollbar.track.y..scrollbar.track.bottom() {
        for x in scrollbar.track.x..scrollbar.track.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(" ");
            cell.set_bg(theme.scrollbar.track);
        }
    }

    let thumb_color = if drag_active {
        theme.scrollbar.thumb_active
    } else {
        theme.scrollbar.thumb
    };
    for y in scrollbar.thumb.y..scrollbar.thumb.bottom() {
        for x in scrollbar.thumb.x..scrollbar.thumb.right() {
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(TRANSCRIPT_SCROLLBAR_THUMB_GLYPH);
            cell.set_fg(thumb_color);
            cell.set_bg(theme.scrollbar.track);
        }
    }
}

fn transcript_scrollbar_geometry(
    viewport: TranscriptViewportLayout,
    scroll_top: usize,
    max_scroll: usize,
) -> Option<TranscriptScrollbarHit> {
    let lane = viewport.scrollbar_lane?;
    if lane.width == 0 || lane.height == 0 || viewport.content.height == 0 {
        return None;
    }

    let track = transcript_scrollbar_track_rect(lane);
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

fn transcript_scrollbar_track_rect(lane: Rect) -> Rect {
    lane
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
    let scroll_top = current_transcript_scroll_top(app, max_scroll);
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

pub(crate) fn transcript_mouse_target(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<TranscriptMouseTarget> {
    let theme = *app.theme();
    let context = transcript_pane_context(app, area, &theme);
    if app.startup_shell_visible() || live_empty_state_visible(app) {
        return None;
    }

    let full_width = context.inner_area.width;
    let show_scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        full_width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout, context.inner_area),
    );
    let render_width = if show_scrollbar {
        full_width
            .saturating_sub(TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH + TRANSCRIPT_SCROLLBAR_TRACK_WIDTH)
    } else {
        full_width
    };
    let viewport = transcript_viewport_layout(context.inner_area, show_scrollbar).content;

    with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        render_width,
        context.base_surface,
        |layout| {
            transcript_mouse_target_at(
                layout,
                viewport,
                transcript_scroll_offset(app, layout, viewport),
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
pub(crate) fn reset_transcript_selection_cache_metrics_for_test() {
    TRANSCRIPT_SELECTION_CACHE.with(|cache| cache.borrow_mut().clear());
    TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn transcript_selection_cache_build_count_for_test() -> usize {
    TRANSCRIPT_SELECTION_CACHE_BUILD_COUNT.with(std::cell::Cell::get)
}

fn render_transcript_surface(
    frame: &mut Frame,
    surface: &MeasuredTranscriptSurface,
    area: Rect,
    local_scroll: usize,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(surface.surface)),
        area,
    );

    let rail_width = if surface.show_outer_rail {
        area.width.min(TRANSCRIPT_SURFACE_RAIL_WIDTH)
    } else {
        0
    };
    if rail_width > 0 {
        let rail_rect = Rect::new(area.x, area.y, rail_width, area.height);
        frame.render_widget(
            Paragraph::new(transcript_surface_rail_lines(
                usize::from(area.height),
                surface.rail_color,
                surface.surface,
            ))
            .style(Style::default().bg(surface.surface)),
            rail_rect,
        );
    }

    if area.width <= rail_width {
        return;
    }

    let content_rect = Rect::new(
        area.x.saturating_add(rail_width),
        area.y,
        area.width.saturating_sub(rail_width),
        area.height,
    );
    let visible_lines =
        visible_surface_lines(surface, local_scroll, usize::from(content_rect.height));
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(panel_style(surface.surface, theme.text.primary));
    frame.render_widget(paragraph, content_rect);
}

fn transcript_mouse_target_at(
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
            if let Some(target) = surface
                .interaction_rows
                .as_ref()
                .and_then(|targets| targets.get(local_row))
                .cloned()
                .flatten()
            {
                return Some(target);
            }
        }
    }

    None
}

fn visible_surface_lines(
    surface: &MeasuredTranscriptSurface,
    local_scroll: usize,
    visible_height: usize,
) -> Vec<Line<'static>> {
    if visible_height == 0 {
        return Vec::new();
    }

    surface
        .lines
        .iter()
        .skip(local_scroll)
        .take(visible_height)
        .cloned()
        .collect()
}

fn build_transcript_render_surfaces(
    section: &TranscriptSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    match section {
        TranscriptSection::Turn(turn) => {
            build_turn_render_surfaces(turn, theme, width, base_surface)
        }
        TranscriptSection::PendingPermission(section) => {
            vec![build_pending_permission_render_surface(
                section,
                theme,
                width,
                base_surface,
            )]
        }
    }
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
        Vec::new(),
        Style::default().fg(theme.text.primary),
        content_width,
        surface,
    )];
    append_user_surface_rich_text_block(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        content_width,
        surface,
        theme,
    );
    if user_msg.queued {
        let agent_accent = theme.agent_accent(&turn.header.profile_label);
        lines.push(user_surface_line(
            vec![Span::styled(
                " QUEUED ".to_string(),
                Style::default()
                    .fg(selected_foreground_for_badge(agent_accent, theme))
                    .bg(agent_accent)
                    .add_modifier(Modifier::BOLD),
            )],
            Style::default().fg(theme.text.secondary),
            content_width,
            surface,
        ));
    }
    lines.push(user_surface_line(
        Vec::new(),
        Style::default().fg(theme.text.primary),
        content_width,
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
    }
}

fn append_user_surface_rich_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: ratatui::style::Color,
    width: u16,
    surface: Color,
    theme: &Theme,
) {
    let _ = theme;
    append_user_surface_text_block(lines, text, color, width, surface);
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
                    TranscriptAssistantPart::ToolCall(tool_call) => Some(tool_call),
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

    Some(
        turn.assistant_parts
            .iter()
            .rposition(|part| matches!(part, TranscriptAssistantPart::Body(_)))
            .unwrap_or_else(|| turn.assistant_parts.len() - 1),
    )
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

fn context_group_tool_id(tool_id: &str) -> bool {
    matches!(
        tool_id,
        "fs.read" | "read" | "fs.glob" | "glob" | "fs.grep" | "grep" | "fs.ls" | "list"
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
    let (kind, show_outer_rail, rail_color, surface, interaction_rows, selection_rows) = match part
    {
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
                transcript_flat_surface(base_surface),
                None,
                None,
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
                assistant_primary_rail_color(turn, theme),
                transcript_flat_surface(base_surface),
                None,
                selection_rows,
            )
        }
        TranscriptAssistantPart::ToolCall(tool_call) => {
            let render = append_tool_call_section_lines(tool_call, theme, width, base_surface);
            lines = render.lines;
            (
                TranscriptRenderSurfaceKind::AssistantTool,
                false,
                transcript_nested_rail_color(theme),
                transcript_nested_surface(theme, base_surface),
                Some(render.interaction_rows),
                None,
            )
        }
        TranscriptAssistantPart::Error(error) => {
            append_assistant_error_box(&mut lines, &error.text, theme, width, base_surface);
            (
                TranscriptRenderSurfaceKind::AssistantError,
                false,
                theme.status.error,
                transcript_nested_surface(theme, base_surface),
                None,
                None,
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
    }
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
        rail_color: assistant_primary_rail_color(turn, theme),
        surface: transcript_flat_surface(base_surface),
        lines: vec![build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
        )],
        interaction_rows: None,
        selection_rows: None,
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
    let surface = transcript_flat_surface(base_surface);
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
        interaction_rows[0] = Some(TranscriptMouseTarget::ToolGroup {
            tool_call_ids: tool_calls
                .iter()
                .map(|tool_call| tool_call.tool_call_id.clone())
                .collect(),
        });
    }

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::AssistantTool,
        show_outer_rail: false,
        rail_color: transcript_nested_rail_color(theme),
        surface,
        lines,
        interaction_rows: Some(interaction_rows),
        selection_rows: None,
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
    let fg = inline_tool_color(tool_call, theme);
    let style = tool_call_header_style(tool_call, fg);

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
        render,
        tool_header_target(tool_call),
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        transcript_flat_surface(base_surface),
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
    let fg = task_inline_tool_color(tool_call, theme);
    let style = tool_call_header_style(tool_call, fg);
    let mut spans = Vec::new();
    if let Some(icon) = tool_call.header.icon {
        spans.push(Span::styled(format!("{icon} "), style));
    }
    spans.push(Span::styled(tool_call.header.title.clone(), style));

    append_surface_row_with_target(
        render,
        tool_header_target(tool_call),
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        transcript_flat_surface(base_surface),
        spans,
        transcript_surface_content_width(width, false),
    );

    for detail_block in &tool_call.detail_blocks {
        let start = render.lines.len();
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
                    append_surface_row(
                        &mut render.lines,
                        TRANSCRIPT_OPCODE_EDIT_INDENT,
                        transcript_flat_surface(base_surface),
                        spans,
                        transcript_surface_content_width(width, false),
                    );
                }
                append_noninteractive_rows(render, start);
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
                        tool_call_id: tool_call.tool_call_id.clone(),
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

    let surface = transcript_flat_surface(base_surface);
    let card_shell = (tool_call.header.status == ToolCallDisplayStatus::Failed).then_some(
        TranscriptToolCardShell {
            indent: TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            rail_color: block_tool_rail_color(tool_call, theme),
            surface,
        },
    );
    let title_style = tool_call_header_style(tool_call, block_tool_color(tool_call, theme));
    let header_target = tool_header_target(tool_call);

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
            render,
            header_target.clone(),
            shell.indent,
            shell.rail_color,
            shell.surface,
            title_spans,
            transcript_surface_content_width(width, false),
        );
    } else {
        append_surface_row_with_target(
            render,
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
                render,
                header_target,
                shell.indent,
                shell.rail_color,
                shell.surface,
                path_spans,
                transcript_surface_content_width(width, false),
            );
        } else {
            append_surface_row_with_target(
                render,
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
                let added = render.lines.len().saturating_sub(start);
                render
                    .interaction_rows
                    .extend(std::iter::repeat_n(tool_header_target(tool_call), added));
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
                append_noninteractive_rows(render, start);
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
                        tool_call_id: tool_call.tool_call_id.clone(),
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
                append_noninteractive_rows(render, start);
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
                append_noninteractive_rows(render, start);
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
                append_noninteractive_rows(render, start);
            }
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                fallback_path,
                force_stacked,
                show_file_header,
            } => {
                append_tool_call_diff_block(
                    &mut render.lines,
                    diff_content,
                    fallback_path.as_deref(),
                    *force_stacked,
                    *show_file_header,
                    theme,
                    width,
                    base_surface,
                    card_shell,
                );
                append_noninteractive_rows(render, start);
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
        .unwrap_or_else(|| transcript_flat_surface(base_surface));
    if let Some(shell) = card_shell {
        append_nested_surface_row_with_target(
            render,
            header_target,
            shell.indent,
            shell.rail_color,
            shell.surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    } else {
        append_surface_row_with_target(
            render,
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

fn tool_header_target(tool_call: &TranscriptToolCallSection) -> Option<TranscriptMouseTarget> {
    tool_call
        .header
        .disclosure_state
        .map(|_| TranscriptMouseTarget::Tool {
            tool_call_id: tool_call.tool_call_id.clone(),
        })
}

fn append_surface_row_with_target(
    render: &mut ToolSectionRender,
    target: Option<TranscriptMouseTarget>,
    indent: &str,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let start = render.lines.len();
    append_surface_row(&mut render.lines, indent, surface, content_spans, width);
    let added = render.lines.len().saturating_sub(start);
    render
        .interaction_rows
        .extend(std::iter::repeat_n(target, added));
}

fn append_nested_surface_row_with_target(
    render: &mut ToolSectionRender,
    target: Option<TranscriptMouseTarget>,
    indent: &str,
    rail_color: Color,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let start = render.lines.len();
    append_nested_surface_row(
        &mut render.lines,
        indent,
        rail_color,
        surface,
        content_spans,
        width,
    );
    let added = render.lines.len().saturating_sub(start);
    render
        .interaction_rows
        .extend(std::iter::repeat_n(target, added));
}

fn append_noninteractive_rows(render: &mut ToolSectionRender, start: usize) {
    let added = render.lines.len().saturating_sub(start);
    render
        .interaction_rows
        .extend(std::iter::repeat_n(None, added));
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
        .unwrap_or_else(|| transcript_flat_surface(base_surface));

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
    let surface = transcript_nested_surface(theme, base_surface);
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

fn append_harness_bash_panel(
    lines: &mut Vec<Line<'static>>,
    panel: HarnessBashPanel<'_>,
    theme: &Theme,
    width: u16,
) {
    let available_width = transcript_surface_content_width(width, false);
    let prefix_width = surface_prefix_width(TRANSCRIPT_OPCODE_EDIT_INDENT);
    let panel_width = usize::from(available_width)
        .saturating_sub(prefix_width)
        .max(HARNESS_SPLIT_RAIL_WIDTH + HARNESS_BLOCK_TOOL_PADDING_LEFT + 1);
    for _ in 0..HARNESS_BLOCK_TOOL_MARGIN_TOP {
        lines.push(Line::default());
    }
    let card_lines = harness_bash_card_lines(
        panel.command,
        panel.output,
        panel.description,
        panel.expand_hint,
        panel.tone,
        theme,
        panel_width,
    );
    append_prebuilt_surface_lines(
        lines,
        TRANSCRIPT_OPCODE_EDIT_INDENT,
        theme.surface.panel,
        card_lines,
        available_width,
    );
}

struct HarnessBashPanel<'a> {
    command: &'a str,
    output: &'a str,
    description: Option<&'a str>,
    expand_hint: Option<&'a str>,
    tone: TranscriptToolCallDetailTone,
}

fn harness_bash_card_lines(
    command: &str,
    output: &str,
    description: Option<&str>,
    expand_hint: Option<&str>,
    tone: TranscriptToolCallDetailTone,
    theme: &Theme,
    panel_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for _ in 0..HARNESS_BLOCK_TOOL_PADDING_TOP {
        lines.push(harness_bash_padding_line(theme));
    }

    let title = harness_bash_title(description);
    append_harness_bash_rows(
        &mut lines,
        &title,
        Style::default().fg(theme.text.secondary),
        theme,
        panel_width,
        HARNESS_BLOCK_TOOL_PADDING_LEFT + HARNESS_BLOCK_TOOL_TITLE_PADDING_LEFT,
    );

    for _ in 0..HARNESS_BLOCK_TOOL_GAP {
        lines.push(harness_bash_padding_line(theme));
    }

    let command_style = Style::default().fg(theme.text.primary);
    append_harness_bash_rows(
        &mut lines,
        &format!("$ {command}"),
        command_style,
        theme,
        panel_width,
        HARNESS_BLOCK_TOOL_PADDING_LEFT,
    );

    let output = output.trim();
    if !output.is_empty() {
        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(theme));
        }
        append_harness_bash_rows(
            &mut lines,
            output,
            harness_bash_output_style(tone, theme),
            theme,
            panel_width,
            HARNESS_BLOCK_TOOL_PADDING_LEFT,
        );
    }

    if let Some(expand_hint) = expand_hint.filter(|hint| has_trimmed_content(hint)) {
        for _ in 0..HARNESS_BLOCK_TOOL_GAP {
            lines.push(harness_bash_padding_line(theme));
        }
        append_harness_bash_rows(
            &mut lines,
            expand_hint.trim(),
            Style::default().fg(theme.text.secondary),
            theme,
            panel_width,
            HARNESS_BLOCK_TOOL_PADDING_LEFT,
        );
    }

    for _ in 0..HARNESS_BLOCK_TOOL_PADDING_BOTTOM {
        lines.push(harness_bash_padding_line(theme));
    }
    lines
}

fn harness_bash_title(description: Option<&str>) -> String {
    let description = description
        .map(collapse_inline_whitespace)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Shell".to_string());
    if description.starts_with("# ") {
        description
    } else {
        format!("# {description}")
    }
}

fn harness_bash_output_style(_tone: TranscriptToolCallDetailTone, theme: &Theme) -> Style {
    Style::default().fg(theme.text.primary)
}

fn append_harness_bash_rows(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    style: Style,
    theme: &Theme,
    panel_width: usize,
    padding_left: usize,
) {
    let content_width = panel_width
        .saturating_sub(HARNESS_SPLIT_RAIL_WIDTH)
        .saturating_sub(padding_left)
        .max(1);
    let rows = if text.is_empty() {
        vec![String::new()]
    } else {
        text.split('\n')
            .flat_map(|row| wrap_plain_terminal_row(row, content_width))
            .collect::<Vec<_>>()
    };

    for row in rows {
        lines.push(harness_bash_content_line(
            &row,
            style,
            theme,
            padding_left,
            content_width,
        ));
    }
}

fn harness_bash_content_line(
    text: &str,
    style: Style,
    theme: &Theme,
    padding_left: usize,
    content_width: usize,
) -> Line<'static> {
    let content = sanitize_harness_bash_text(text);
    let remaining = content_width.saturating_sub(display_width(&content));
    harness_bash_line(
        vec![
            harness_split_rail_span(theme),
            Span::styled(" ".repeat(padding_left), Style::default()),
            Span::styled(content, style),
            Span::styled(" ".repeat(remaining), Style::default()),
        ],
        theme.surface.panel,
    )
}

fn harness_bash_padding_line(theme: &Theme) -> Line<'static> {
    harness_bash_line(vec![harness_split_rail_span(theme)], theme.surface.panel)
}

fn harness_split_rail_span(theme: &Theme) -> Span<'static> {
    Span::styled(
        HARNESS_SPLIT_RAIL_GLYPH.to_string(),
        Style::default().fg(theme.surface.shell),
    )
}

fn harness_bash_line(spans: Vec<Span<'static>>, surface: Color) -> Line<'static> {
    Line::from(
        spans
            .into_iter()
            .map(|span| surface_span(span.content.into_owned(), span.style, surface))
            .collect::<Vec<_>>(),
    )
}

fn wrap_plain_terminal_row(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let mut chunk = take_width_prefix(remaining, width);
        if chunk.is_empty() {
            chunk = remaining
                .char_indices()
                .nth(1)
                .map(|(index, _)| &remaining[..index])
                .unwrap_or(remaining);
        }
        rows.push(chunk.to_string());
        remaining = &remaining[chunk.len()..];
    }
    rows
}

fn sanitize_harness_bash_text(text: &str) -> String {
    replace_control_chars_except_tabs(text)
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
        .unwrap_or_else(|| transcript_flat_surface(base_surface));
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

fn ordered_todo_items(items: &[TranscriptTodoItem]) -> Vec<&TranscriptTodoItem> {
    let active_index = items
        .iter()
        .position(|item| item.status == TranscriptTodoStatus::InProgress)
        .or_else(|| {
            items
                .iter()
                .position(|item| item.status == TranscriptTodoStatus::Pending)
        })
        .or_else(|| {
            items
                .iter()
                .rposition(|item| item.status == TranscriptTodoStatus::Completed)
        })
        .unwrap_or(0);
    std::iter::once(&items[active_index])
        .chain(
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| (index != active_index).then_some(item)),
        )
        .collect()
}

fn inline_tool_color(tool_call: &TranscriptToolCallSection, theme: &Theme) -> Color {
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission => theme.status.warning,
        ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::Succeeded
        | ToolCallDisplayStatus::Failed => theme.text.secondary,
    }
}

fn task_inline_tool_color(tool_call: &TranscriptToolCallSection, theme: &Theme) -> Color {
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission => theme.status.warning,
        ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::Succeeded
        | ToolCallDisplayStatus::Failed => theme.text.secondary,
    }
}

fn block_tool_color(tool_call: &TranscriptToolCallSection, theme: &Theme) -> Color {
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission => theme.status.warning,
        ToolCallDisplayStatus::Failed => theme.status.error,
        ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::Succeeded => theme.text.primary,
    }
}

fn block_tool_rail_color(tool_call: &TranscriptToolCallSection, theme: &Theme) -> Color {
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission => theme.status.warning,
        ToolCallDisplayStatus::Failed => theme.status.error,
        ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::Succeeded => theme.text.accent,
    }
}

fn tool_call_header_style(tool_call: &TranscriptToolCallSection, color: Color) -> Style {
    let mut style = Style::default().fg(color);
    if tool_call.header.struck_out {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

fn tool_header_disclosure_glyph(
    disclosure_state: Option<TranscriptToolCallDisclosureState>,
) -> Option<&'static str> {
    match disclosure_state {
        Some(TranscriptToolCallDisclosureState::Collapsed) => Some("▸"),
        Some(TranscriptToolCallDisclosureState::Expanded) => Some("▾"),
        None => None,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tool diff rendering keeps transcript shell styling explicit at the call site"
)]
fn append_tool_call_diff_block(
    lines: &mut Vec<Line<'static>>,
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
    if let Some(diff_lines) = render_structured_diff_lines_with_options(
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
        if let Some(shell) = card_shell {
            append_prebuilt_nested_surface_lines(
                lines,
                shell.indent,
                shell.rail_color,
                shell.surface,
                diff_lines,
                nested_width,
            );
        } else {
            append_prebuilt_surface_lines(
                lines,
                TRANSCRIPT_OPCODE_EDIT_INDENT,
                transcript_nested_surface(theme, base_surface),
                diff_lines,
                nested_width,
            );
        }
    }
}

fn append_prebuilt_nested_surface_lines(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    rail_color: Color,
    surface: Color,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    let prefix = nested_surface_prefix(indent, rail_color, surface);
    let prefix_width = nested_surface_prefix_width(indent);
    for line in prebuilt {
        lines.push(nested_surface_line(
            prefix.clone(),
            prefix_width,
            line.spans,
            width,
            surface,
        ));
    }
}

fn append_prebuilt_surface_lines(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    surface: Color,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    let prefix = surface_prefix(indent);
    let prefix_width = surface_prefix_width(indent);
    for line in prebuilt {
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            line.spans,
            width,
            surface,
        ));
    }
}

fn append_surface_row(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let prefix = surface_prefix(indent);
    let prefix_width = surface_prefix_width(indent);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wrapped_rows = wrap_surface_spans(content_spans, content_width);

    if wrapped_rows.is_empty() {
        lines.push(surface_line(
            prefix,
            prefix_width,
            Vec::new(),
            width,
            surface,
        ));
        return;
    }

    for row in wrapped_rows {
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            row,
            width,
            surface,
        ));
    }
}

fn surface_prefix(indent: &str) -> Vec<Span<'static>> {
    if indent.is_empty() {
        Vec::new()
    } else {
        vec![Span::raw(indent.to_string())]
    }
}

fn surface_prefix_width(indent: &str) -> usize {
    display_width(indent)
}

fn surface_line(
    mut prefix: Vec<Span<'static>>,
    prefix_width: usize,
    content_spans: Vec<Span<'static>>,
    width: u16,
    surface: Color,
) -> Line<'static> {
    let mut visible_width = prefix_width;
    for span in content_spans {
        visible_width += span.width();
        prefix.push(surface_span(span.content.into_owned(), span.style, surface));
    }
    let remaining = usize::from(width).saturating_sub(visible_width);
    if remaining > 0 {
        prefix.push(surface_span(
            " ".repeat(remaining),
            Style::default(),
            surface,
        ));
    }
    Line::from(prefix)
}

fn build_pending_permission_render_surface(
    section: &TranscriptPendingPermissionSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> TranscriptRenderSurface {
    let surface = transcript_emphasized_surface(theme, base_surface);
    let mut lines = vec![Line::from(vec![
        Span::raw(TRANSCRIPT_SURFACE_BODY_PREFIX.to_string()),
        Span::styled(
            format!("{} ", theme.live_shell.glyphs.pending_permission),
            Style::default().fg(theme.status.warning),
        ),
        Span::styled(
            "permission checkpoint",
            Style::default()
                .fg(theme.status.warning)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (", muted_meta_style(theme)),
        Span::styled("fail-closed", Style::default().fg(theme.status.error)),
        Span::styled(" · ", muted_meta_style(theme)),
        Span::styled("review required", Style::default().fg(theme.status.warning)),
        Span::styled(")", muted_meta_style(theme)),
    ])];
    append_rich_text_block(
        &mut lines,
        &section.summary,
        theme.text.primary,
        TRANSCRIPT_SURFACE_BODY_PREFIX,
        theme,
        transcript_surface_content_width(width, true),
    );

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::PendingPermission,
        show_outer_rail: true,
        rail_color: theme.status.warning,
        surface,
        lines,
        interaction_rows: None,
        selection_rows: None,
    }
}

fn render_transcript_surface_lines(surfaces: &[TranscriptRenderSurface]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for surface in surfaces {
        if surface.show_outer_rail {
            lines.extend(surface.lines.iter().cloned().map(|line| {
                prepend_transcript_surface_rail(line, surface.rail_color, surface.surface)
            }));
        } else {
            lines.extend(surface.lines.iter().cloned());
        }
    }
    lines
}

fn append_user_surface_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: ratatui::style::Color,
    width: u16,
    surface: Color,
) {
    let base_style = Style::default().fg(color);
    for line in text.lines() {
        append_user_surface_wrapped_line(
            lines,
            if line.is_empty() {
                Vec::new()
            } else {
                vec![Span::styled(line.to_string(), base_style)]
            },
            base_style,
            width,
            surface,
        );
    }

    if text.is_empty() {
        append_user_surface_wrapped_line(lines, Vec::new(), base_style, width, surface);
    }
}

fn append_user_surface_wrapped_line(
    lines: &mut Vec<Line<'static>>,
    content_spans: Vec<Span<'static>>,
    prefix_style: Style,
    width: u16,
    surface: Color,
) {
    let prefix_width = display_width(TRANSCRIPT_USER_BODY_PREFIX);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    if content_spans.is_empty() {
        lines.push(user_surface_line(Vec::new(), prefix_style, width, surface));
        return;
    }

    for row in wrap_surface_spans(content_spans, content_width) {
        lines.push(user_surface_line(row, prefix_style, width, surface));
    }
}

fn user_surface_line(
    content_spans: Vec<Span<'static>>,
    prefix_style: Style,
    _width: u16,
    surface: Color,
) -> Line<'static> {
    let mut spans = vec![surface_span(
        TRANSCRIPT_USER_BODY_PREFIX,
        prefix_style,
        surface,
    )];
    for span in content_spans {
        spans.push(surface_span(span.content.into_owned(), span.style, surface));
    }
    Line::from(spans)
}

fn append_markdownish_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: ratatui::style::Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    let base_style = Style::default().fg(color);
    for line in text.lines() {
        append_markdownish_line(lines, line, color, prefix, base_style, theme, width);
    }

    if text.is_empty() {
        append_prefixed_wrapped_spans_line(lines, prefix, base_style, Vec::new(), width);
    }
}

fn append_markdownish_line(
    lines: &mut Vec<Line<'static>>,
    line: &str,
    color: Color,
    prefix: &str,
    base_style: Style,
    theme: &Theme,
    width: u16,
) {
    if line.is_empty() {
        append_prefixed_wrapped_spans_line(lines, prefix, base_style, Vec::new(), width);
        return;
    }

    let indent_width = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let indent = " ".repeat(indent_width);
    let trimmed = line.trim_start();
    let content_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);

    if let Some(text) = markdown_heading_text(trimmed) {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown_spans(
                text,
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                theme.text.accent,
                theme,
            ),
            width,
        );
        return;
    }

    if markdown_rule(trimmed) {
        append_prefixed_wrapped_spans_line(
            lines,
            prefix,
            base_style,
            vec![Span::styled(
                "─".repeat(content_width),
                Style::default().fg(theme.text.secondary),
            )],
            width,
        );
        return;
    }

    if let Some(text) = trimmed.strip_prefix("> ") {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}▍ "),
            Style::default().fg(theme.text.secondary),
            parse_inline_markdown_spans(
                text,
                Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::ITALIC),
                theme.text.secondary,
                theme,
            ),
            width,
        );
        return;
    }

    if let Some((list_prefix, text, list_style, text_style)) = markdown_list_prefix(trimmed, theme)
    {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}{list_prefix}"),
            list_style,
            parse_inline_markdown_spans(text, text_style, color, theme),
            width,
        );
        return;
    }

    append_prefixed_wrapped_spans_line(
        lines,
        prefix,
        base_style,
        parse_inline_markdown_spans(trimmed, base_style, color, theme),
        width,
    );
}

fn append_prefixed_wrapped_spans_line(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    prefix_style: Style,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    if content_spans.is_empty() {
        lines.push(Line::from(Span::styled(prefix.to_string(), prefix_style)));
        return;
    }

    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    for row in wrap_surface_spans(content_spans, content_width) {
        let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
        spans.extend(row);
        lines.push(Line::from(spans));
    }
}

fn parse_inline_markdown_spans(
    text: &str,
    base_style: Style,
    base_color: Color,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix('[') {
            if let Some(label_end) = rest.find("](") {
                let after_label = &rest[label_end + 2..];
                if let Some(url_end) = after_label.find(')') {
                    spans.push(Span::styled(
                        rest[..label_end].to_string(),
                        base_style
                            .fg(theme.text.accent)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    remaining = &after_label[url_end + 1..];
                    continue;
                }
            }
        }

        if let Some(url_end) = raw_url_length(remaining) {
            spans.push(Span::styled(
                remaining[..url_end].to_string(),
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            remaining = &remaining[url_end..];
            continue;
        }

        if let Some(rest) = remaining.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style.fg(base_color).add_modifier(Modifier::BOLD),
                ));
                remaining = &rest[end + 2..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix("~~") {
            if let Some(end) = rest.find("~~") {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .fg(theme.text.secondary)
                        .add_modifier(Modifier::CROSSED_OUT),
                ));
                remaining = &rest[end + 2..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style.fg(theme.status.success),
                ));
                remaining = &rest[end + 1..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .fg(theme.status.warning)
                        .add_modifier(Modifier::ITALIC),
                ));
                remaining = &rest[end + 1..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('_') {
            if let Some(end) = rest.find('_') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .fg(theme.status.warning)
                        .add_modifier(Modifier::ITALIC),
                ));
                remaining = &rest[end + 1..];
                continue;
            }
        }

        let next_marker = ["[", "http://", "https://", "**", "~~", "`", "*", "_"]
            .into_iter()
            .filter_map(|marker| remaining.find(marker))
            .min()
            .unwrap_or(remaining.len());
        if next_marker == 0 {
            let ch = remaining
                .chars()
                .next()
                .expect("remaining text should not be empty");
            spans.push(Span::styled(ch.to_string(), base_style.fg(base_color)));
            remaining = &remaining[ch.len_utf8()..];
            continue;
        }
        let plain = &remaining[..next_marker];
        spans.push(Span::styled(plain.to_string(), base_style.fg(base_color)));
        remaining = &remaining[next_marker..];
    }

    spans
}

fn markdown_heading_text(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    line[hashes..].strip_prefix(' ').map(str::trim)
}

fn markdown_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
    matches!(stripped.as_str(), "---" | "***" | "___")
}

fn markdown_list_prefix<'a>(
    line: &'a str,
    theme: &Theme,
) -> Option<(String, &'a str, Style, Style)> {
    for marker in ["- [x] ", "* [x] ", "+ [x] "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "☑ ".to_string(),
                text,
                Style::default()
                    .fg(theme.status.success)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    for marker in ["- [ ] ", "* [ ] ", "+ [ ] "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "☐ ".to_string(),
                text,
                Style::default().fg(theme.text.secondary),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "• ".to_string(),
                text,
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    let digits = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 {
        let suffix = &line[digits..];
        if let Some(text) = suffix.strip_prefix(". ") {
            return Some((
                format!("{}{}", &line[..digits], ". "),
                text,
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    None
}

fn raw_url_length(text: &str) -> Option<usize> {
    let prefix = if text.starts_with("https://") {
        "https://"
    } else if text.starts_with("http://") {
        "http://"
    } else {
        return None;
    };

    let tail = &text[prefix.len()..];
    let extra = tail.find(char::is_whitespace).unwrap_or(tail.len());
    Some(prefix.len() + extra)
}

fn append_rich_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: ratatui::style::Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    if !text.contains("```") {
        append_markdownish_text_block(lines, text, color, prefix, theme, width);
        return;
    }

    let Some(blocks) = parse_fenced_text_blocks(text) else {
        append_markdownish_text_block(lines, text, color, prefix, theme, width);
        return;
    };

    for block in blocks {
        match block {
            ParsedTextBlock::Plain(plain) => {
                append_markdownish_text_block(lines, &plain, color, prefix, theme, width)
            }
            ParsedTextBlock::Code {
                language,
                body,
                raw,
            } => {
                if let Some(language) = language.as_deref() {
                    if matches!(language, "diff" | "patch") {
                        if let Some(diff_lines) =
                            render_structured_diff_lines(&body, None, prefix, width, false, theme)
                        {
                            lines.extend(diff_lines);
                            continue;
                        }
                    }
                }

                let highlighted = render_highlighted_code_block(
                    language.as_deref(),
                    &body,
                    &raw,
                    prefix,
                    color,
                    theme,
                );
                if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty()) {
                    lines.push(Line::default());
                }
                append_prebuilt_plain_lines(lines, prefix, highlighted, width);
                lines.push(Line::default());
            }
        }
    }
}

fn append_prebuilt_plain_lines(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    for line in prebuilt {
        append_prefixed_wrapped_spans_line(lines, prefix, Style::default(), line.spans, width);
    }
}

fn parse_fenced_text_blocks(text: &str) -> Option<Vec<ParsedTextBlock>> {
    let mut blocks = Vec::new();
    let mut plain_lines = Vec::new();
    let mut code_lines = Vec::new();
    let mut raw_lines = Vec::new();
    let mut language = None;
    let mut in_code = false;

    for line in text.lines().map(normalize_fenced_line) {
        if !in_code {
            if let Some(block_language) = opening_fence_language(line) {
                if !plain_lines.is_empty() {
                    blocks.push(ParsedTextBlock::Plain(plain_lines.join("\n")));
                    plain_lines.clear();
                }
                in_code = true;
                language = block_language.map(str::to_string);
                raw_lines.push(line.to_string());
                continue;
            }
            plain_lines.push(line.to_string());
            continue;
        }

        raw_lines.push(line.to_string());
        if is_closing_fence(line) {
            blocks.push(ParsedTextBlock::Code {
                language: language.take(),
                body: code_lines.join("\n"),
                raw: raw_lines.join("\n"),
            });
            code_lines.clear();
            raw_lines.clear();
            in_code = false;
        } else {
            code_lines.push(line.to_string());
        }
    }

    if in_code {
        return None;
    }

    if !plain_lines.is_empty() {
        blocks.push(ParsedTextBlock::Plain(plain_lines.join("\n")));
    }

    Some(blocks)
}

fn render_highlighted_code_block(
    language: Option<&str>,
    body: &str,
    _raw: &str,
    _prefix: &str,
    color: ratatui::style::Color,
    _theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let highlighted = language.and_then(|language| {
        let syntax_assets = syntax_highlight_assets();
        let syntax = syntax_assets.syntax_set.find_syntax_by_token(language)?;
        let mut highlighter = HighlightLines::new(syntax, &syntax_assets.theme);
        let mut lines = Vec::new();
        for source_line in body.lines() {
            let Ok(regions) = highlighter.highlight_line(source_line, &syntax_assets.syntax_set)
            else {
                return None;
            };
            if regions.is_empty() {
                lines.push(Line::from(Span::styled(
                    source_line.to_string(),
                    Style::default().fg(color),
                )));
            } else {
                lines.push(Line::from(
                    regions
                        .into_iter()
                        .map(|(style, content)| {
                            Span::styled(content.to_string(), syntect_style_to_ratatui(style))
                        })
                        .collect::<Vec<_>>(),
                ));
            }
        }
        Some(lines)
    });

    if let Some(highlighted) = highlighted {
        lines.extend(highlighted);
    } else {
        for source_line in body.lines() {
            lines.push(Line::from(Span::styled(
                source_line.to_string(),
                Style::default().fg(color),
            )));
        }
    }

    if body.is_empty() {
        lines.push(Line::from(Span::styled("", Style::default().fg(color))));
    }

    lines
}

fn syntax_highlight_assets() -> &'static SyntaxHighlightAssets {
    static SYNTAX_ASSETS: OnceLock<SyntaxHighlightAssets> = OnceLock::new();

    SYNTAX_ASSETS.get_or_init(|| {
        let syntax_set = SyntaxSet::load_defaults_nonewlines();
        let theme_set = syntect::highlighting::ThemeSet::load_defaults();
        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .or_else(|| theme_set.themes.values().next().cloned())
            .unwrap_or_default();
        SyntaxHighlightAssets { syntax_set, theme }
    })
}

fn syntect_style_to_ratatui(style: syntect::highlighting::Style) -> Style {
    let mut rendered = Style::default().fg(syntect_color_to_ratatui(style.foreground));

    if style.font_style.contains(SyntectFontStyle::BOLD) {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(SyntectFontStyle::ITALIC) {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(SyntectFontStyle::UNDERLINE) {
        rendered = rendered.add_modifier(Modifier::UNDERLINED);
    }

    rendered
}

fn syntect_color_to_ratatui(color: syntect::highlighting::Color) -> ratatui::style::Color {
    ratatui::style::Color::Rgb(color.r, color.g, color.b)
}

fn opening_fence_language(line: &str) -> Option<Option<&str>> {
    let trimmed = line.trim_start();
    let suffix = trimmed.strip_prefix("```")?;
    let language = suffix
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty());
    Some(language)
}

fn is_closing_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

fn normalize_fenced_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
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

fn current_transcript_scroll_top(app: &AppState, max_scroll: usize) -> usize {
    if max_scroll == 0 {
        return 0;
    }

    if app.follow_mode {
        return max_scroll;
    }

    max_scroll
        .saturating_sub(app.transcript_scroll)
        .min(max_scroll)
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn transcript_scroll_offset(
    app: &AppState,
    layout: &MeasuredTranscriptLayout,
    inner_area: Rect,
) -> usize {
    let viewport_height = usize::from(inner_area.height);
    if viewport_height == 0 {
        return 0;
    }

    let max_scroll = layout.total_height.saturating_sub(viewport_height);
    if max_scroll == 0 {
        return 0;
    }

    if app.follow_mode {
        return max_scroll;
    }

    current_transcript_scroll_top(app, max_scroll)
}

fn transcript_viewport_layout(area: Rect, show_scrollbar: bool) -> TranscriptViewportLayout {
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

fn transcript_scrollbar_needed(layout: &MeasuredTranscriptLayout, area: Rect) -> bool {
    area.width > TRANSCRIPT_SCROLLBAR_GUTTER_WIDTH + TRANSCRIPT_SCROLLBAR_TRACK_WIDTH
        && layout.total_height > usize::from(area.height)
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

fn transcript_surface_focused(app: &AppState) -> bool {
    !app.replay_mode
        && app.active_tab == Tab::Run
        && app.focus == Focus::Details
        && !app.details_drawer_open()
}

fn transcript_surface_content_width(width: u16, show_outer_rail: bool) -> u16 {
    if show_outer_rail {
        width.saturating_sub(TRANSCRIPT_SURFACE_RAIL_WIDTH).max(1)
    } else {
        width.max(1)
    }
}

fn transcript_surface_render_width(width: u16, kind: TranscriptRenderSurfaceKind) -> u16 {
    match kind {
        TranscriptRenderSurfaceKind::User => width
            .saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH)
            .max(1),
        _ => width.max(1),
    }
}

fn transcript_emphasized_surface(theme: &Theme, base_surface: Color) -> Color {
    if base_surface == theme.surface.panel {
        elevated_card_surface(theme)
    } else {
        theme.surface.panel
    }
}

fn transcript_flat_surface(base_surface: Color) -> Color {
    base_surface
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
        Style::default().fg(assistant_primary_label_color(turn, theme)),
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

fn transcript_streaming_spinner_frame(animation_phase: usize) -> &'static str {
    TRANSCRIPT_BRAILLE_SPINNER_FRAMES[animation_phase % TRANSCRIPT_BRAILLE_SPINNER_FRAMES.len()]
}

fn titlecase_label(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

fn assistant_footer_label(value: &str) -> String {
    if !has_trimmed_content(value)
        || value.eq_ignore_ascii_case("unknown")
        || value.eq_ignore_ascii_case("default")
    {
        return "Assistant".to_string();
    }
    titlecase_label(value)
}

fn assistant_primary_label_color(turn: &TranscriptTurnSection, theme: &Theme) -> Color {
    match turn.header.status {
        ActivityStatus::Queued => theme.text.secondary,
        ActivityStatus::Streaming => theme.text.primary,
        ActivityStatus::Done => theme.text.primary,
        ActivityStatus::Error => theme.status.error,
    }
}

fn selected_foreground_for_badge(background: Color, theme: &Theme) -> Color {
    match background {
        Color::Rgb(red, green, blue) => {
            let luminance =
                0.299 * f64::from(red) + 0.587 * f64::from(green) + 0.114 * f64::from(blue);
            if luminance > 127.5 {
                theme.text.inverse
            } else {
                theme.text.primary
            }
        }
        _ => theme.text.inverse,
    }
}

fn assistant_primary_rail_color(turn: &TranscriptTurnSection, theme: &Theme) -> Color {
    match turn.header.status {
        ActivityStatus::Queued | ActivityStatus::Streaming | ActivityStatus::Done => {
            theme.agent_accent(&turn.header.profile_label)
        }
        ActivityStatus::Error => theme.status.error,
    }
}

fn transcript_nested_rail_color(theme: &Theme) -> Color {
    theme.text.secondary
}

fn transcript_nested_surface(_theme: &Theme, base_surface: Color) -> Color {
    transcript_flat_surface(base_surface)
}

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 86_400_000 {
        format!(
            "{}d {}h",
            duration_ms / 86_400_000,
            (duration_ms % 86_400_000) / 3_600_000
        )
    } else if duration_ms >= 3_600_000 {
        format!(
            "{}h {}m",
            duration_ms / 3_600_000,
            (duration_ms % 3_600_000) / 60_000
        )
    } else if duration_ms >= 60_000 {
        format!(
            "{}m {}s",
            duration_ms / 60_000,
            (duration_ms % 60_000) / 1_000
        )
    } else if duration_ms >= 1_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

fn transcript_surface_rail_lines(
    height: usize,
    rail_color: Color,
    surface: Color,
) -> Text<'static> {
    Text::from(
        (0..height)
            .map(|_| {
                Line::from(Span::styled(
                    TRANSCRIPT_RAIL_GLYPH,
                    Style::default().fg(rail_color).bg(surface),
                ))
            })
            .collect::<Vec<_>>(),
    )
}

fn prepend_transcript_surface_rail(
    line: Line<'static>,
    rail_color: Color,
    surface: Color,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        TRANSCRIPT_RAIL_GLYPH,
        Style::default().fg(rail_color).bg(surface),
    )];
    spans.extend(line.spans);
    Line::from(spans)
}

fn surface_span(text: impl Into<String>, style: Style, surface: Color) -> Span<'static> {
    Span::styled(text.into(), Style::default().bg(surface).patch(style))
}

fn append_nested_surface_row(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    rail_color: Color,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let prefix = nested_surface_prefix(indent, rail_color, surface);
    let prefix_width = nested_surface_prefix_width(indent);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wrapped_rows = wrap_surface_spans(content_spans, content_width);

    if wrapped_rows.is_empty() {
        lines.push(nested_surface_line(
            prefix,
            prefix_width,
            Vec::new(),
            width,
            surface,
        ));
        return;
    }

    for row in wrapped_rows {
        lines.push(nested_surface_line(
            prefix.clone(),
            prefix_width,
            row,
            width,
            surface,
        ));
    }
}

fn nested_surface_prefix(indent: &str, rail_color: Color, surface: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if !indent.is_empty() {
        spans.push(Span::raw(indent.to_string()));
    }
    spans.push(Span::styled(
        TRANSCRIPT_RAIL_GLYPH,
        Style::default().fg(rail_color).bg(surface),
    ));
    spans.push(surface_span(" ", Style::default(), surface));
    spans
}

fn nested_surface_prefix_width(indent: &str) -> usize {
    display_width(indent) + display_width(TRANSCRIPT_RAIL_GLYPH) + 1
}

fn nested_surface_line(
    mut prefix: Vec<Span<'static>>,
    prefix_width: usize,
    content_spans: Vec<Span<'static>>,
    width: u16,
    surface: Color,
) -> Line<'static> {
    let mut visible_width = prefix_width;
    for span in content_spans {
        visible_width += span.width();
        prefix.push(surface_span(span.content.into_owned(), span.style, surface));
    }
    let remaining = usize::from(width).saturating_sub(visible_width);
    if remaining > 0 {
        prefix.push(surface_span(
            " ".repeat(remaining),
            Style::default(),
            surface,
        ));
    }
    Line::from(prefix)
}

fn wrap_surface_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    if spans.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for token in spans.into_iter().flat_map(surface_wrap_tokens) {
        let token_text = token.content.to_string();
        let token_width = token.width();
        let token_is_whitespace = token_text.chars().all(char::is_whitespace);

        if token_is_whitespace && current.is_empty() {
            continue;
        }

        if current_width + token_width <= width {
            current_width += token_width;
            current.push(token);
            continue;
        }

        if token_is_whitespace {
            if !current.is_empty() {
                rows.push(current);
                current = Vec::new();
                current_width = 0;
            }
            continue;
        }

        if !current.is_empty() {
            rows.push(current);
            current = Vec::new();
            current_width = 0;
        }

        let mut remainder = token_text.as_str();
        while !remainder.is_empty() {
            let chunk = take_width_prefix(remainder, width);
            current_width = display_width(chunk);
            current.push(Span::styled(chunk.to_string(), token.style));
            remainder = &remainder[chunk.len()..];
            if !remainder.is_empty() {
                rows.push(current);
                current = Vec::new();
                current_width = 0;
            }
        }
    }

    if !current.is_empty() {
        rows.push(current);
    }

    rows
}

fn surface_wrap_tokens(span: Span<'static>) -> Vec<Span<'static>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_whitespace = None;

    for ch in span.content.chars() {
        let is_whitespace = ch.is_whitespace();
        if current_is_whitespace == Some(is_whitespace) || current.is_empty() {
            current.push(ch);
            current_is_whitespace = Some(is_whitespace);
            continue;
        }

        tokens.push(Span::styled(current.clone(), span.style));
        current.clear();
        current.push(ch);
        current_is_whitespace = Some(is_whitespace);
    }

    if !current.is_empty() {
        tokens.push(Span::styled(current, span.style));
    }

    tokens
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_section_model_preserves_activity_order() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity("request-a", ActivityStatus::Done, "first reply"),
        transcript_section_model_test_activity(
            "request-b",
            ActivityStatus::Streaming,
            "second reply",
        ),
    ]);
    app.selected_activity_index = 1;

    let sections = build_transcript_sections(&app);

    let turn_ids = sections
        .iter()
        .map(|section| match section {
            TranscriptSection::Turn(turn) => Some(turn.request_id.as_str()),
            TranscriptSection::PendingPermission(_) => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(turn_ids, vec![Some("request-a"), Some("request-b")]);
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-tools",
        ActivityStatus::Error,
        "assistant body",
    );
    entry.thinking_text = "tool planning".to_string();
    entry.error_message = Some("tool call failed".to_string());
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-1".to_string(),
        tool_id: "shell.run".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"cmd":"false"}"#.to_string(),
        args_digest: "digest".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Failed,
        output_summary: Some("command failed".to_string()),
        output_digest: Some("out-digest".to_string()),
        output_json: None,
        truncated_output: Some("command failed".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let sections = build_transcript_sections(&app);

    assert_eq!(sections.len(), 1);
    let TranscriptSection::Turn(turn) = &sections[0] else {
        panic!("expected turn section");
    };

    assert_eq!(
        turn.thinking.as_ref().map(|block| block.text.as_str()),
        Some("tool planning")
    );
    assert_eq!(
        turn.body_blocks,
        vec![TranscriptBodyBlock::RichText("assistant body".to_string())]
    );
    assert_eq!(
        turn.error.as_ref().map(|error| error.text.as_str()),
        Some("tool call failed")
    );
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(
        turn.tool_calls[0],
        TranscriptToolCallSection {
            tool_call_id: "call-1".to_string(),
            header: TranscriptToolCallHeader {
                tool_id: "shell.run".to_string(),
                title: "false".to_string(),
                subtitle: None,
                path_metadata: None,
                icon: Some("$"),
                status: ToolCallDisplayStatus::Failed,
                visual_style: TranscriptToolCallVisualStyle::Inline,
                struck_out: false,
                disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
            },
            detail_blocks: vec![TranscriptToolCallDetailBlock::Message {
                text: "command failed".to_string(),
                tone: TranscriptToolCallDetailTone::Error,
            }],
            expanded: false,
        }
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_reasoning_precedes_answer_and_tool_rows() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-answer-first",
        ActivityStatus::Done,
        "assistant answer",
    );
    entry.thinking_text = "working through the plan".to_string();
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-1".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
        args_digest: "digest".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("24 lines read".to_string()),
        output_digest: Some("out-digest".to_string()),
        output_json: None,
        truncated_output: Some("24 lines read".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 80)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    let reasoning_row = lines
        .iter()
        .position(|line| line.contains("working through the plan"))
        .expect("reasoning row");
    let answer_row = lines
        .iter()
        .position(|line| line.contains("assistant answer"))
        .expect("assistant answer row");
    let tool_row = lines
        .iter()
        .enumerate()
        .skip(answer_row + 1)
        .find_map(|(index, line)| line.contains("Read src/ui.rs").then_some(index))
        .expect("tool row");

    assert!(reasoning_row < answer_row);
    assert!(answer_row < tool_row);
    assert!(lines
        .iter()
        .any(|line| line.contains("working through the plan")));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_tool_rows_follow_chronological_turn_order() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_turn_order_{seq:04}"),
            seq,
            run_id: "run_turn_order".to_string(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T14:36:00Z".to_string()),
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
        "req_ordered_turn",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_ordered_turn".to_string(),
                text: "Check transcript parity".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_ordered_turn".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Check transcript parity".to_string(),
                request_digest: "digest-turn-order".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_ordered_turn".to_string(),
                delta: "I’ll inspect the MCP result first.\n".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_docs".to_string(),
                tool_id: "mcp.docs-rs.search".to_string(),
                args_summary: r#"{"query":"ratatui"}"#.to_string(),
                args_digest: "digest-docs-search".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_docs".to_string(),
        }),
    ));
    app.ingest_event(event(
        6,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_docs".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("ratatui 0.29.0".to_string()),
                output_digest: Some("digest-docs-result".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        7,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_ordered_turn".to_string(),
                delta: "It returns the crate metadata inline afterward.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_ordered_turn".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-turn-order-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 96)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    let opening_row = lines
        .iter()
        .position(|line| line.contains("I’ll inspect the MCP result first."))
        .expect("opening answer row");
    let tool_row = lines
        .iter()
        .position(|line| line.contains("docs-rs_search") && line.contains("ratatui"))
        .expect("tool row");
    let closing_row = lines
        .iter()
        .position(|line| line.contains("It returns the crate metadata inline afterward."))
        .expect("closing answer row");

    assert!(
        opening_row < tool_row,
        "tool row should stay after the initial assistant prose"
    );
    assert!(
        tool_row < closing_row,
        "tool row should stay before the dependent assistant follow-up"
    );
}

#[test]
fn reasoning_after_tool_renders_in_new_block_below_tool() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_reasoning_after_tool_{seq:04}"),
            seq,
            run_id: "run_reasoning_after_tool".to_string(),
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
        "req_reasoning_after_tool",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                text: "Inspect tokio docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect tokio docs".to_string(),
                request_digest: "digest-reasoning-after-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                delta: "Inspecting Tokio docs first.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_tokio_docs".to_string(),
                tool_id: "mcp.docs-rs.search_in_crate".to_string(),
                args_summary: r#"{"query":"spawn"}"#.to_string(),
                args_digest: "digest-tokio-docs-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_tokio_docs".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("spawn docs".to_string()),
                output_digest: Some("digest-tokio-docs-output".to_string()),
                output_json: Some(serde_json::json!({
                    "server": { "id": "docs-rs" },
                    "payload": { "tool": "docs_rs_search_in_crate" }
                })),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        6,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                delta: "Now I can answer with the exact API.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        7,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                delta: "Use tokio::spawn for spawned tasks.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-reasoning-after-tool-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    let sections = build_transcript_sections(&app);
    assert_eq!(sections.len(), 1);
    let TranscriptSection::Turn(turn) = &sections[0] else {
        panic!("expected turn section");
    };

    assert!(matches!(
        turn.assistant_parts.as_slice(),
        [
            TranscriptAssistantPart::Reasoning(_),
            TranscriptAssistantPart::ToolCall(_),
            TranscriptAssistantPart::Reasoning(_),
            TranscriptAssistantPart::Body(_)
        ]
    ));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let first_reasoning_row = lines
        .iter()
        .position(|line| line.contains("Inspecting Tokio docs first."))
        .expect("first reasoning row");
    let tool_row = lines
        .iter()
        .position(|line| line.contains("docs-rs_docs_rs_search_in_crate") && line.contains("spawn"))
        .expect("tool row");
    let second_reasoning_row = lines
        .iter()
        .position(|line| line.contains("Now I can answer with the exact API."))
        .expect("second reasoning row");
    let answer_row = lines
        .iter()
        .position(|line| line.contains("Use tokio::spawn for spawned tasks."))
        .expect("answer row");

    assert!(first_reasoning_row < tool_row);
    assert!(tool_row < second_reasoning_row);
    assert!(second_reasoning_row < answer_row);
    assert!(
        lines[second_reasoning_row.saturating_sub(1)]
            .trim()
            .is_empty()
            || lines[second_reasoning_row.saturating_sub(1)].trim() == "┃"
    );
}

#[test]
fn task_completion_summary_does_not_duplicate_streamed_assistant_text() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_duplicate_body_{seq:04}"),
            seq,
            run_id: "run_duplicate_body".to_string(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T14:40:00Z".to_string()),
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
        "req_duplicate_body",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_duplicate_body".to_string(),
                text: "Say hello".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_duplicate_body".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Say hello".to_string(),
                request_digest: "digest-duplicate-body".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_duplicate_body".to_string(),
                delta: "Hello!".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_duplicate_body".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-duplicate-body-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_duplicate_body",
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_duplicate_body".to_string(),
            result_summary: "Hello!".to_string(),
            result_digest: "digest-task-duplicate-body".to_string(),
            metadata: None,
        }),
    ));

    let sections = build_transcript_sections(&app);
    assert_eq!(sections.len(), 1);
    let TranscriptSection::Turn(turn) = &sections[0] else {
        panic!("expected turn section");
    };

    let body_parts = turn
        .assistant_parts
        .iter()
        .filter_map(|part| match part {
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text)) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(body_parts, vec!["Hello!"]);
}

#[test]
fn tool_task_completion_summary_does_not_render_as_assistant_body() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_tool_task_body_{seq:04}"),
            seq,
            run_id: "run_tool_task_body".to_string(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T14:45:00Z".to_string()),
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
        "req_tool_task_body",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_tool_task_body".to_string(),
                text: "Inspect tokio docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_tool_task_body",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_task_body".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect tokio docs".to_string(),
                request_digest: "digest-tool-task-body".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_tool_task_body",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_docs_tokio".to_string(),
                tool_id: "mcp.docs-rs.search_in_crate".to_string(),
                args_summary: r#"{"crate_name":"tokio","query":"spawn"}"#.to_string(),
                args_digest: "digest-docs-tokio-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_tool_task_body",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_docs_tokio".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("fn spawn\nstruct JoinHandle".to_string()),
                output_digest: Some("digest-docs-tokio-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_tool_task_body",
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_docs_tokio".to_string(),
            result_summary: "fn spawn\nstruct JoinHandle".to_string(),
            result_digest: "digest-task-docs-tokio".to_string(),
            metadata: Some(harness_core::event::TaskCompletionMetadata {
                lineage: Some(harness_core::event::TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_docs_tokio".to_string()),
                    ..harness_core::event::TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 96)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("docs-rs_search_in_crate"));
    assert!(rendered.contains("tokio"));
    assert!(rendered.contains("spawn"));
    assert!(
        !rendered.contains("fn spawn\nstruct JoinHandle")
            && !rendered.contains("struct JoinHandle"),
        "tool task completion summary must stay out of assistant body\n{rendered}"
    );
}

#[test]
fn task_row_hides_raw_task_result_payload_until_expanded() {
    let tool_call = crate::app::ToolCallEntry {
        tool_call_id: "tc_noisy_task".to_string(),
        tool_id: "task".to_string(),
        canonical_tool_id: Some("task".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"description":"review streaming states","subagent_type":"explore"}"#
            .to_string(),
        args_digest: "digest-noisy-task-args".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some(
            "task_id: agent_000002 (for resuming to continue this task if needed)\
             \nrequest_id: req_000004\
             \n<task_result>\n.sisyphus/evidence/task-7-streaming-states-review.json\n</task_result>"
                .to_string(),
        ),
        output_digest: Some("digest-noisy-task-output".to_string()),
        output_json: None,
        truncated_output: None,
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: Some(4_000),
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    };

    let mut detail_blocks = Vec::new();
    let (title, icon, visual_style, _) =
        build_agent_spawn_tool_row(&tool_call, None, &mut detail_blocks);
    assert_eq!(title, "Explore Task — review streaming states");
    assert_eq!(icon, Some("│"));
    assert_eq!(visual_style, TranscriptToolCallVisualStyle::TaskInline);
    let detail_text = task_detail_blocks_text(&detail_blocks);
    assert!(detail_text.contains("└ 0 toolcalls · 4.0s"));
    assert!(!detail_text.contains("task_id:"));
    assert!(!detail_text.contains("request_id:"));
    assert!(!detail_text.contains("<task_result>"));
    assert!(!detail_text.contains(".sisyphus/evidence"));
}

#[test]
fn task_row_profile_label_matches_harness_titlecase() {
    assert_eq!(subagent_profile_label(""), "General");
    assert_eq!(subagent_profile_label("general"), "General");
    assert_eq!(subagent_profile_label("foo-bar"), "Foo-Bar");
    assert_eq!(subagent_profile_label("foo_bar"), "Foo_bar");
    assert_eq!(subagent_profile_label("gPT worker"), "GPT Worker");
}

#[cfg(test)]
fn task_detail_blocks_text(blocks: &[TranscriptToolCallDetailBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            TranscriptToolCallDetailBlock::Message { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_edit_tool_matches_inline_diff_shape() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("harness-inline.diff"),
        "--- crates/harness-tui/src/ui.rs\n+++ crates/harness-tui/src/ui.rs\n@@ -44,8 +44,7 @@\n use ui_secondary::{\n-    render_diff_tab, render_events_tab, render_help_tab,\n+    render_events_tab, render_help_tab, render_live_details_overlay,\n     render_operator_sidebar,\n };\n",
    )
    .expect("write inline diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry =
        transcript_section_model_test_activity("request-edit-inline", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-1".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied".to_string()),
        output_digest: Some("digest-edit-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-inline-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Applied,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: Some("digest-new-file".to_string()),
            diff_rel_path: Some("artifacts/harness-inline.diff".to_string()),
            diff_digest: Some("digest-diff".to_string()),
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 160)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Patch · ui.rs"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(rendered.contains("render_diff_tab"));
    assert!(rendered.contains("render_live_details_overlay"));
    assert!(rendered.contains("44"));
    assert!(
        !rendered.contains("@@ -44,8 +44,7 @@"),
        "inline transcript diffs should suppress raw hunk headers to match harness chat diffs\n{rendered}"
    );
    assert!(
        rendered.lines().any(|line| {
            line.contains("render_diff_tab") && line.contains("render_live_details_overlay")
        }),
        "tool inline diff should render side-by-side in wide transcript layouts\n{rendered}"
    );
    assert!(rendered.lines().any(|line| {
        line.contains("render_diff_tab") && line.contains('-') && line.contains('+')
    }));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_native_edit_renders_inline_diff_from_artifact() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("native-edit.diff"),
        "--- docs/rust.md\n+++ docs/rust.md\n@@ -1,3 +1,2 @@\n # Rust\n-## Ownership\n Safe and fast\n",
    )
    .expect("write native edit diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry =
        transcript_section_model_test_activity("request-native-edit", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-native-edit-1".to_string(),
        tool_id: "edit".to_string(),
        canonical_tool_id: Some("edit".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary:
            "{\"filePath\":\"docs/rust.md\",\"oldString\":\"## Ownership\\n\",\"newString\":\"\"}"
                .to_string(),
        args_digest: "digest-native-edit".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied successfully.".to_string()),
        output_digest: Some("digest-native-edit-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied successfully.".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![crate::app::ToolArtifactEntry {
            path: "artifacts/native-edit.diff".to_string(),
            digest: Some("digest-native-edit-diff".to_string()),
        }],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 140)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("Edit · rust.md"));
    assert!(rendered.contains("docs"));
    assert!(rendered.contains("Ownership"));
    assert!(rendered
        .lines()
        .any(|line| line.contains("Ownership") && line.contains('-')));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_apply_patch_multifile_uses_output_edit_paths() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("apply-a.diff"),
        "@@ -1,1 +1,1 @@\n-old a\n+new a\n",
    )
    .expect("write apply patch diff fixture a");
    std::fs::write(
        artifacts_dir.join("apply-b.diff"),
        "@@ -1,1 +1,1 @@\n-old b\n+new b\n",
    )
    .expect("write apply patch diff fixture b");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-apply-patch-inline",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-apply-patch-1".to_string(),
        tool_id: "apply_patch".to_string(),
        canonical_tool_id: Some("apply_patch".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
        args_digest: "digest-apply-patch".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Success. Updated the following files".to_string()),
        output_digest: Some("digest-apply-patch-output".to_string()),
        output_json: Some(serde_json::json!({
            "files": ["M notes/a.md", "M notes/b.md"],
            "edits": [
                {
                    "edit_id": "apply-patch-1",
                    "path": "notes/a.md",
                    "summary": "apply patch update notes/a.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-a.diff",
                    "diff_digest": "digest-apply-a"
                },
                {
                    "edit_id": "apply-patch-2",
                    "path": "notes/b.md",
                    "summary": "apply patch update notes/b.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-b.diff",
                    "diff_digest": "digest-apply-b"
                }
            ]
        })),
        truncated_output: Some("Success. Updated the following files".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-a.diff".to_string(),
                digest: Some("digest-apply-a".to_string()),
            },
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-b.diff".to_string(),
                digest: Some("digest-apply-b".to_string()),
            },
        ],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.set_patch_file_output_expanded_for_test("call-apply-patch-1", "notes/a.md", true);
    app.set_patch_file_output_expanded_for_test("call-apply-patch-1", "notes/b.md", true);
    assert!(app.patch_file_output_expanded("call-apply-patch-1", "notes/a.md"));
    assert!(app.patch_file_output_expanded("call-apply-patch-1", "notes/b.md"));
    let sections = build_transcript_sections(&app);
    let TranscriptSection::Turn(turn) = &sections[0] else {
        panic!("expected turn section");
    };
    let Some(TranscriptToolCallDetailBlock::FileSection(file_section)) =
        turn.tool_calls[0].detail_blocks.first()
    else {
        panic!("expected apply_patch file section");
    };
    assert_eq!(
        file_section.disclosure_state,
        TranscriptToolCallDisclosureState::Expanded
    );
    assert!(!file_section.detail_blocks.is_empty());

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 140)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let direct_rendered = transcript_test_line_texts(
        append_tool_call_section_lines(
            &turn.tool_calls[0],
            &Theme::default(),
            140,
            Theme::default().surface.shell,
        )
        .lines,
    )
    .join("\n");

    assert!(rendered.contains("Patch · 2 files"));
    assert!(!rendered.contains("Patch · 2 files  ▸"));
    assert!(rendered.contains("a.md · notes"));
    assert!(rendered.contains("b.md · notes"));
    assert!(
        rendered.contains("new a"),
        "rendered output missing first diff\nfull:\n{rendered}\n\ndirect:\n{direct_rendered}"
    );
    assert!(
        rendered.contains("new b"),
        "rendered output missing second diff\nfull:\n{rendered}\n\ndirect:\n{direct_rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("apply-rename.diff"),
        "--- src/session_turn.rs\n+++ src/session_diff.rs\n@@ -1,1 +1,1 @@\n-pub fn render_session_turn_diff() {}\n+pub fn render_session_diff_view() {}\n",
    )
    .expect("write rename diff fixture");
    std::fs::write(
        artifacts_dir.join("apply-wrap.diff"),
        "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n",
    )
    .expect("write wrapped diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-apply-patch-rename-wrap",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-apply-patch-rename-wrap".to_string(),
        tool_id: "apply_patch".to_string(),
        canonical_tool_id: Some("apply_patch".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
        args_digest: "digest-apply-patch-rename-wrap".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Success. Updated the following files".to_string()),
        output_digest: Some("digest-apply-patch-rename-wrap-output".to_string()),
        output_json: Some(serde_json::json!({
            "files": ["M src/session_diff.rs", "M docs/transcript.md"],
            "edits": [
                {
                    "edit_id": "apply-patch-rename",
                    "path": "src/session_diff.rs",
                    "summary": "apply patch move src/session_turn.rs -> src/session_diff.rs",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-rename.diff",
                    "diff_digest": "digest-apply-rename"
                },
                {
                    "edit_id": "apply-patch-wrap",
                    "path": "docs/transcript.md",
                    "summary": "apply patch update docs/transcript.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-wrap.diff",
                    "diff_digest": "digest-apply-wrap"
                }
            ]
        })),
        truncated_output: Some("Success. Updated the following files".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-rename.diff".to_string(),
                digest: Some("digest-apply-rename".to_string()),
            },
            crate::app::ToolArtifactEntry {
                path: "artifacts/apply-wrap.diff".to_string(),
                digest: Some("digest-apply-wrap".to_string()),
            },
        ],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.set_patch_file_output_expanded_for_test(
        "call-apply-patch-rename-wrap",
        "src/session_diff.rs",
        true,
    );
    app.set_patch_file_output_expanded_for_test(
        "call-apply-patch-rename-wrap",
        "docs/transcript.md",
        true,
    );
    assert!(app.patch_file_output_expanded("call-apply-patch-rename-wrap", "src/session_diff.rs"));
    assert!(app.patch_file_output_expanded("call-apply-patch-rename-wrap", "docs/transcript.md"));
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        84,
    ));
    let rendered = lines.join("\n");

    let rename_header = lines
        .iter()
        .find(|line| line.contains("session_diff.rs") && line.contains("src"))
        .expect("rename header");
    assert!(
        rename_header.contains("session_diff.rs"),
        "rename header: {rename_header}"
    );
    assert!(rendered.contains("Patch · 2 files"));
    assert!(!rendered.contains("Patch · 2 files  ▸"));
    assert!(
        lines.iter().any(|line| {
            line.contains("session turn diff view keeps the tool row spacing perfectly aligne")
        }),
        "long diff prefix missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("n every transcript lane for operators reviewing compact windows")
        }),
        "removed line continuation missing\n{rendered}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("across the transcript surface for operators reviewing compact wind")
        }),
        "added line wrapped continuation prefix missing\n{rendered}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("s and narrow shells")),
        "added line wrapped continuation tail missing\n{rendered}"
    );
    assert!(
        !rendered.contains('…'),
        "wrapped transcript diff should not fall back to truncation\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_inline_diff_stays_compact_between_tool_rows() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("apply-compact.diff"),
        "--- docs/spacing.md\n+++ docs/spacing.md\n@@ -1,1 +1,1 @@\n-tool rows drift apart after inline diffs in compact transcript layouts\n+tool rows stay packed tightly after inline diffs in compact transcript layouts\n",
    )
    .expect("write compact diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-inline-diff-compact-tools",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-read-before-diff".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: Some("read".to_string()),
        resolved_tool_identity: None,
        args_summary: r#"{"path":"docs/spacing.md"}"#.to_string(),
        args_digest: "digest-read-before-diff".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("12 lines read".to_string()),
        output_digest: Some("digest-read-before-diff-output".to_string()),
        output_json: None,
        truncated_output: Some("12 lines read".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    });
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-apply-patch-compact".to_string(),
        tool_id: "apply_patch".to_string(),
        canonical_tool_id: Some("apply_patch".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
        args_digest: "digest-apply-patch-compact".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Success. Updated the following files".to_string()),
        output_digest: Some("digest-apply-patch-compact-output".to_string()),
        output_json: Some(serde_json::json!({
            "files": ["M docs/spacing.md"],
            "edits": [
                {
                    "edit_id": "apply-patch-compact",
                    "path": "docs/spacing.md",
                    "summary": "apply patch update docs/spacing.md",
                    "deleted": false,
                    "diff_rel_path": "artifacts/apply-compact.diff",
                    "diff_digest": "digest-apply-compact"
                }
            ]
        })),
        truncated_output: Some("Success. Updated the following files".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: vec![crate::app::ToolArtifactEntry {
            path: "artifacts/apply-compact.diff".to_string(),
            digest: Some("digest-apply-compact".to_string()),
        }],
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 3,
        last_seq: 4,
        first_mono_ms: 3,
        last_mono_ms: 4,
        first_timestamp: None,
        last_timestamp: None,
    });
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-read-after-diff".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: Some("read".to_string()),
        resolved_tool_identity: None,
        args_summary: r#"{"path":"docs/spacing.md"}"#.to_string(),
        args_digest: "digest-read-after-diff".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("12 lines read".to_string()),
        output_digest: Some("digest-read-after-diff-output".to_string()),
        output_json: None,
        truncated_output: Some("12 lines read".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 5,
        last_seq: 6,
        first_mono_ms: 5,
        last_mono_ms: 6,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.set_patch_file_output_expanded_for_test(
        "call-apply-patch-compact",
        "docs/spacing.md",
        true,
    );
    assert!(app.patch_file_output_expanded("call-apply-patch-compact", "docs/spacing.md"));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");
    let read_before = lines
        .iter()
        .position(|line| line.contains("Read docs/spacing.md"))
        .expect("read-before row");
    let patch_header = lines
        .iter()
        .position(|line| line.contains("Patch · spacing.md"))
        .expect("patch header row");
    let diff_tail = lines
        .iter()
        .rposition(|line| line.contains("compact transcript layouts"))
        .expect("diff tail row");
    let read_after = lines
        .iter()
        .rposition(|line| line.contains("Read docs/spacing.md"))
        .expect("read-after row");

    assert!(read_before < patch_header && patch_header < diff_tail && diff_tail < read_after);
    assert!(
        lines[read_before + 1..patch_header]
            .iter()
            .filter(|line| line.trim().is_empty() || line.trim() == "┃")
            .count()
            <= 1,
        "tool-to-diff spacing should stay compact\n{rendered}"
    );
    assert!(
        lines[diff_tail + 1..read_after]
            .iter()
            .filter(|line| line.trim().is_empty() || line.trim() == "┃")
            .count()
            <= 1,
        "diff-to-tool spacing should stay compact\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_applied_edit_missing_diff_surfaces_fallback() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = transcript_section_model_test_activity(
        "request-edit-missing-diff",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-missing-diff".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"docs/rust.md"}"#.to_string(),
        args_digest: "digest-edit-missing-diff".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied".to_string()),
        output_digest: Some("digest-edit-missing-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-missing-diff-1".to_string(),
            path: "docs/rust.md".to_string(),
            status: crate::app::EditDisplayStatus::Applied,
            summary: Some("Remove the ownership section".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: Some("digest-new-file".to_string()),
            diff_rel_path: Some("artifacts/missing-edit.diff".to_string()),
            diff_digest: Some("digest-diff".to_string()),
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 140)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Patch · rust.md"));
    assert!(rendered.contains("docs"));
    assert!(rendered.contains("Diff preview unavailable"));
    assert!(rendered.contains("artifacts/missing-edit.diff"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_proposed_edit_renders_header() {
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-edit-proposed", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-proposed".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-proposed".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Running,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-proposed-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Proposed,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: None,
            diff_rel_path: None,
            diff_digest: None,
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 120)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Edit · ui.rs"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(!rendered.contains("tool edit.hashline_apply"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_harness_tool_progress_indicators() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-harness-tool-progress",
        ActivityStatus::Done,
        "",
    );

    let mut pending_read = transcript_section_model_test_tool_call("call-read-pending", "fs.read");
    pending_read.status = ToolCallDisplayStatus::Queued;

    let mut running_read = transcript_section_model_test_tool_call("call-read-running", "fs.read");
    running_read.args_summary = r#"{"path":"src/lib.rs","offset":3,"limit":8}"#.to_string();
    running_read.status = ToolCallDisplayStatus::Running;

    let mut pending_edit = transcript_section_model_test_tool_call("call-edit-pending", "edit");
    pending_edit.status = ToolCallDisplayStatus::Queued;

    let mut pending_patch =
        transcript_section_model_test_tool_call("call-patch-pending", "apply_patch");
    pending_patch.status = ToolCallDisplayStatus::Queued;

    entry.tool_calls = vec![pending_read, running_read, pending_edit, pending_patch];
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let initial_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));
    let initial_rendered = initial_lines.join("\n");
    assert!(
        initial_rendered.contains("~ Reading file..."),
        "missing pending read indicator\n{initial_rendered}"
    );
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("⠋ Read src/lib.rs [offset=3, limit=8]")));
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("~ Preparing edit...")));
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("~ Preparing patch...")));

    app.advance_transcript_animation_phase();

    let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));
    assert!(updated_lines
        .iter()
        .any(|line| line.contains("⠙ Read src/lib.rs [offset=3, limit=8]")));

    let mut mixed_context_app = AppState::default();
    let mut mixed_context_entry = transcript_section_model_test_activity(
        "request-mixed-context-progress",
        ActivityStatus::Done,
        "",
    );
    let mut completed_read = transcript_section_model_test_tool_call("call-read-done", "fs.read");
    completed_read.args_summary = r#"{"path":"src/lib.rs"}"#.to_string();
    completed_read.status = ToolCallDisplayStatus::Succeeded;
    completed_read.output_summary = Some("12 lines read from src/lib.rs".to_string());

    let mut running_glob = transcript_section_model_test_tool_call("call-glob-running", "fs.glob");
    running_glob.args_summary = r#"{"pattern":"*.rs","path":"src"}"#.to_string();
    running_glob.status = ToolCallDisplayStatus::Running;

    mixed_context_entry.tool_calls = vec![completed_read, running_glob];
    mixed_context_app.activities = std::collections::VecDeque::from(vec![mixed_context_entry]);

    let mixed_context_rendered = transcript_test_line_texts(build_transcript_lines_for_width(
        &mixed_context_app,
        &Theme::default(),
        120,
    ))
    .join("\n");
    assert!(
        !mixed_context_rendered.contains("Gathering context"),
        "active context tools should stay as per-tool indicators\n{mixed_context_rendered}"
    );
    assert!(mixed_context_rendered.contains("→ Read src/lib.rs"));
    assert!(mixed_context_rendered.contains("✱ Glob \"*.rs\" in src"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_rejected_edit_surfaces_reason_inline() {
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-edit-rejected", ActivityStatus::Error, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-rejected".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-rejected".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Failed,
        output_summary: Some("ANCHOR_MISMATCH".to_string()),
        output_digest: None,
        output_json: None,
        truncated_output: Some("ANCHOR_MISMATCH".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-rejected-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Rejected,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch".to_string()),
            new_file_digest: None,
            diff_rel_path: None,
            diff_digest: None,
            rejection_reason: Some("ANCHOR_MISMATCH at line 45".to_string()),
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 120)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("← Edit · ui.rs"));
    assert!(rendered.contains("crates/harness-tui/src"));
    assert!(rendered.contains("ANCHOR_MISMATCH at line 45"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_follow_mode_uses_measured_surface_heights() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity(
            "request-long",
            ActivityStatus::Done,
        "this reply is intentionally long enough to wrap across several measured surface rows and keep wrapping even after the harness footer gap is accounted for in measured layout",
        ),
        transcript_section_model_test_activity(
            "request-short",
            ActivityStatus::Done,
            "short reply",
        ),
    ]);
    app.selected_activity_index = 1;
    app.follow_mode = true;

    let width = 28;
    let viewport_height = 6;
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), width);

    assert_eq!(layout.sections.len(), 2);
    assert!(
        layout.sections[0].content_height >= layout.sections[0].lines.len(),
        "measured transcript height should never undercount the rendered line inventory"
    );
    assert!(
        layout.sections[0].content_height > layout.sections[1].content_height,
        "the longer wrapped section should still measure taller than the short reply"
    );

    let measured_total_height = layout
        .sections
        .iter()
        .map(MeasuredTranscriptSection::total_height)
        .sum::<usize>();
    assert_eq!(layout.total_height, measured_total_height);

    let scroll = transcript_scroll_offset(&app, &layout, Rect::new(0, 0, width, viewport_height));
    let expected = layout
        .total_height
        .saturating_sub(usize::from(viewport_height));

    assert_eq!(scroll, expected);
    assert!(
        scroll > 1,
        "follow mode should scroll by measured surface height, not just section count"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_scroll_offset_preserves_large_overflow() {
    let mut app = AppState::default();
    app.follow_mode = false;
    app.transcript_scroll = 17;

    let layout = MeasuredTranscriptLayout {
        sections: Vec::new(),
        total_height: usize::from(u16::MAX) + 512,
    };

    let scroll = transcript_scroll_offset(&app, &layout, Rect::new(0, 0, 80, 12));
    let expected = layout
        .total_height
        .saturating_sub(12)
        .saturating_sub(app.transcript_scroll);

    assert_eq!(scroll, expected);
    assert!(
        scroll > usize::from(u16::MAX),
        "large transcript offsets should not truncate to u16"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_visible_surface_lines_support_large_offsets() {
    let surface = MeasuredTranscriptSurface {
        top_offset: 0,
        height: usize::from(u16::MAX) + 1024,
        width: 24,
        show_outer_rail: false,
        rail_color: Color::Reset,
        surface: Color::Reset,
        lines: (0..(usize::from(u16::MAX) + 1024))
            .map(|index| Line::from(format!("line {index}")))
            .collect(),
        interaction_rows: None,
        selection_rows: None,
    };

    let visible = visible_surface_lines(&surface, usize::from(u16::MAX) + 7, 3)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        visible,
        vec![
            format!("line {}", usize::from(u16::MAX) + 7),
            format!("line {}", usize::from(u16::MAX) + 8),
            format!("line {}", usize::from(u16::MAX) + 9),
        ]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_pending_permission_stays_after_last_activity() {
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-a",
            ActivityStatus::Done,
            "assistant reply",
        )]);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_pending_permission_order".to_string(),
        seq: 1,
        run_id: "run_pending_permission_order".to_string(),
        mono_ms: 0,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Supervisor,
            None,
        ),
        correlation_id: Some("tool_call_pending_permission_order".to_string()),
        causation_id: None,
        stream_key: Some("tool_call_pending_permission_order".to_string()),
        payload: harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: "perm_pending_permission_order".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tool_call_pending_permission_order".to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm-order".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    });

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);

    assert_eq!(layout.sections.len(), 1);
    assert_eq!(layout.sections[0].kind, MeasuredTranscriptSectionKind::Turn);
}

#[cfg(test)]
pub(crate) fn exact_test_native_tool_transcript_rows_show_disclosure_timestamps_and_task_metadata()
{
    let theme = Theme::default();

    let mut native_read = transcript_section_model_test_tool_call("tc-native-read", "fs.read");
    native_read.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("fs.read".to_string()),
        effective_tool_id: Some("fs.read".to_string()),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: None,
    });
    native_read.args_summary = r#"{"path":"src/ui.rs","offset":12,"limit":24}"#.to_string();
    native_read.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    native_read.status = ToolCallDisplayStatus::Succeeded;
    native_read.output_summary = Some("24 lines read from src/ui.rs".to_string());
    native_read.truncated_output = native_read.output_summary.clone();
    native_read.last_mono_ms = 1_250;
    native_read.last_timestamp = Some("2026-03-22T14:35:44Z".to_string());

    let mut alias_read = transcript_section_model_test_tool_call("tc-alias-read", "read");
    alias_read.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("read".to_string()),
        effective_tool_id: Some("fs.read".to_string()),
        canonical_tool_id: Some("fs.read".to_string()),
        alias_source_tool_id: Some("read".to_string()),
    });
    alias_read.args_summary = native_read.args_summary.clone();
    alias_read.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    alias_read.status = ToolCallDisplayStatus::Succeeded;
    alias_read.output_summary = native_read.output_summary.clone();
    alias_read.truncated_output = native_read.truncated_output.clone();
    alias_read.last_mono_ms = native_read.last_mono_ms;
    alias_read.last_timestamp = native_read.last_timestamp.clone();

    let native_read_section = build_transcript_tool_call_section(
        &native_read,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    let alias_read_section = build_transcript_tool_call_section(
        &alias_read,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        native_read_section.header.title,
        alias_read_section.header.title
    );
    assert_eq!(
        native_read_section.header.icon,
        alias_read_section.header.icon
    );
    assert_eq!(
        native_read_section.header.visual_style,
        alias_read_section.header.visual_style
    );
    assert_eq!(native_read_section.header.subtitle, None);
    assert_eq!(alias_read_section.header.subtitle, None);
    assert_eq!(
        alias_read_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    assert!(alias_read_section
        .detail_blocks
        .iter()
        .any(|block| matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, .. }
                if text.contains("Compat alias · read → fs.read")
        )));

    let mut task_call = transcript_section_model_test_tool_call("tc-task", "task");
    task_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("task".to_string()),
        effective_tool_id: Some("task".to_string()),
        canonical_tool_id: Some("task".to_string()),
        alias_source_tool_id: None,
    });
    task_call.args_summary =
        r#"{"description":"audit transcript parity","subagent_type":"researcher"}"#.to_string();
    task_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    task_call.status = ToolCallDisplayStatus::Succeeded;
    task_call.output_json = Some(serde_json::json!({
        "description": "audit transcript parity",
        "profile": "researcher",
        "mode": "foreground",
        "status": "completed",
        "duration_ms": 1600,
        "result_summary": "Found the inline transcript path.",
        "child_tool_call_count": 3,
        "child_session_id": "agent_worker",
        "child_request_id": "req_child",
    }));
    task_call.lineage = Some(crate::app::TaskLineageEntry {
        parent_tool_call_id: Some("tc-task".to_string()),
        parent_task_id: None,
        parent_request_id: Some("req_parent".to_string()),
        child_session_id: Some("agent_worker".to_string()),
        child_request_id: Some("req_child".to_string()),
    });
    task_call.timing_elapsed_ms = Some(1600);
    task_call.last_mono_ms = 1_600;
    task_call.last_timestamp = Some("2026-03-22T14:36:01Z".to_string());

    let task_section = build_transcript_tool_call_section(
        &task_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(task_section.header.disclosure_state, None);
    assert_eq!(task_section.header.icon, Some("│"));
    assert_eq!(
        task_section.header.visual_style,
        TranscriptToolCallVisualStyle::TaskInline
    );
    let mut task_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&task_section, &theme, 120, theme.surface.panel);
        task_lines.extend(render.lines);
    }
    let task_text = transcript_test_line_texts(task_lines).join("\n");
    assert!(task_text.contains("│ Researcher Task — audit transcript parity"));
    assert!(task_text.contains("Researcher Task — audit transcript parity"));
    assert!(task_text.contains("3 toolcalls · 1.6s"));
    assert!(!task_text.contains("Found the inline transcript path."));
    assert!(!task_text.contains("Compat alias · task → agent.spawn"));
    assert!(!task_text.contains("Task audit transcript parity"));

    let expanded_task_section = build_transcript_tool_call_section(
        &task_call,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_task_render =
        append_tool_call_section_lines(&expanded_task_section, &theme, 120, theme.surface.panel);
    let expanded_task_text = transcript_test_line_texts(expanded_task_render.lines).join("\n");
    assert!(!expanded_task_text.contains("Found the inline transcript path."));

    let mut fetch_call = transcript_section_model_test_tool_call("tc-fetch", "webfetch");
    fetch_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("webfetch".to_string()),
        effective_tool_id: Some("web.fetch".to_string()),
        canonical_tool_id: Some("web.fetch".to_string()),
        alias_source_tool_id: Some("webfetch".to_string()),
    });
    fetch_call.args_summary =
        r#"{"url":"https://example.test/report.pdf","format":"markdown"}"#.to_string();
    fetch_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    fetch_call.status = ToolCallDisplayStatus::Succeeded;
    fetch_call.output_summary =
        Some("report ready\npage count: 2\nformat: pdf\nstored inline artifact".to_string());
    fetch_call.truncated_output = fetch_call.output_summary.clone();
    fetch_call.artifact_refs = vec![crate::app::ToolArtifactEntry {
        path: "artifacts/toolcalls/tc-fetch/web.fetch.pdf".to_string(),
        digest: Some("digest-fetch-artifact".to_string()),
    }];
    fetch_call.timing_elapsed_ms = Some(2400);
    fetch_call.last_mono_ms = 2_400;
    fetch_call.last_timestamp = Some("2026-03-22T14:37:12Z".to_string());

    let fetch_section = build_transcript_tool_call_section(
        &fetch_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(fetch_section.header.disclosure_state, None);
    let mut fetch_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&fetch_section, &theme, 120, theme.surface.panel);
        fetch_lines.extend(render.lines);
    }
    let fetch_text = transcript_test_line_texts(fetch_lines).join("\n");
    assert!(fetch_text.contains("% WebFetch https://example.test/report.pdf"));
    assert!(!fetch_text.contains("report ready"));
    assert!(!fetch_text.contains("stored inline artifact"));
    assert!(!fetch_text.contains("Click to expand"));
    assert!(!fetch_text.contains("Attachment ·"));
    assert!(!fetch_text.contains("web.fetch.pdf"));
    assert!(fetch_text.contains("Compat alias · webfetch → web.fetch"));

    let mut generic_call = transcript_section_model_test_tool_call("tc-generic", "vendor.magic");
    generic_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("vendor.magic".to_string()),
        effective_tool_id: Some("vendor.magic".to_string()),
        canonical_tool_id: None,
        alias_source_tool_id: None,
    });
    generic_call.args_summary =
        r#"{"path":"notes.md","query":"child parity","limit":3}"#.to_string();
    generic_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    generic_call.status = ToolCallDisplayStatus::Succeeded;
    generic_call.timing_elapsed_ms = Some(800);
    let generic_section = build_transcript_tool_call_section(
        &generic_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        generic_section.header.title,
        "vendor.magic child parity [limit=3]"
    );
    assert_eq!(generic_section.header.subtitle, None);
}

#[cfg(test)]
pub(crate) fn exact_test_mcp_tool_transcript_rows_use_effective_identity_without_generic_fallback()
{
    let theme = Theme::default();

    let mut direct_call =
        transcript_section_model_test_tool_call("tc-mcp-direct", "mcp.fixture.echo");
    direct_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("mcp.fixture.echo".to_string()),
        effective_tool_id: Some("mcp.fixture.echo".to_string()),
        canonical_tool_id: None,
        alias_source_tool_id: None,
    });
    direct_call.args_summary = r#"{"text":"hello from direct"}"#.to_string();
    direct_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    direct_call.status = ToolCallDisplayStatus::Succeeded;
    direct_call.output_summary = Some("direct output summary".to_string());
    direct_call.output_json = Some(serde_json::json!({
        "server": {
            "id": "fixture",
            "transport": "stdio",
        },
        "protocolVersion": "2025-06-18",
        "serverInfo": {
            "name": "fixture",
            "version": "1.0.0",
        },
        "payload": {
            "tool": "echo",
            "arguments": { "text": "hello from direct" },
            "result": {
                "content": [{ "type": "text", "text": "direct output summary" }],
                "isError": false,
            },
        },
    }));
    direct_call.timing_elapsed_ms = Some(900);

    let mut wrapper_call =
        transcript_section_model_test_tool_call("tc-mcp-wrapper", "mcp.fixture.tool.call");
    wrapper_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("mcp.fixture.tool.call".to_string()),
        effective_tool_id: Some("mcp.fixture.echo".to_string()),
        canonical_tool_id: None,
        alias_source_tool_id: None,
    });
    wrapper_call.args_summary =
        r#"{"tool":"echo","arguments":{"text":"hello from wrapper"}}"#.to_string();
    wrapper_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    wrapper_call.status = ToolCallDisplayStatus::Succeeded;
    wrapper_call.output_summary = Some("wrapper output summary".to_string());
    wrapper_call.output_json = Some(serde_json::json!({
        "server": {
            "id": "fixture",
            "transport": "stdio",
        },
        "protocolVersion": "2025-06-18",
        "serverInfo": {
            "name": "fixture",
            "version": "1.0.0",
        },
        "payload": {
            "tool": "echo",
            "arguments": { "text": "hello from wrapper" },
            "result": {
                "content": [{ "type": "text", "text": "wrapper output summary" }],
                "isError": false,
            },
        },
    }));
    wrapper_call.timing_elapsed_ms = Some(900);

    let direct_section = build_transcript_tool_call_section(
        &direct_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    let wrapper_section = build_transcript_tool_call_section(
        &wrapper_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );

    assert_eq!(
        direct_section.header.title,
        "fixture_echo [text=hello from direct]"
    );
    assert_eq!(
        wrapper_section.header.title,
        "fixture_echo [text=hello from wrapper]"
    );
    assert_eq!(direct_section.header.icon, Some("⚙"));
    assert_eq!(wrapper_section.header.icon, Some("⚙"));
    assert_eq!(
        direct_section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(
        wrapper_section.header.visual_style,
        direct_section.header.visual_style
    );
    assert_eq!(direct_section.header.disclosure_state, None);
    assert_eq!(wrapper_section.header.disclosure_state, None);

    let mut direct_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&direct_section, &theme, 120, theme.surface.panel);
        direct_lines.extend(render.lines);
    }
    let mut wrapper_lines = Vec::new();
    {
        let render =
            append_tool_call_section_lines(&wrapper_section, &theme, 120, theme.surface.panel);
        wrapper_lines.extend(render.lines);
    }

    let direct_text = transcript_test_line_texts(direct_lines).join("\n");
    let wrapper_text = transcript_test_line_texts(wrapper_lines).join("\n");
    assert!(direct_text.contains("fixture_echo"));
    assert!(direct_text.contains("[text=hello from direct]"));
    assert!(wrapper_text.contains("fixture_echo"));
    assert!(wrapper_text.contains("[text=hello from wrapper]"));
    assert!(!direct_text.contains("direct output summary"));
    assert!(!wrapper_text.contains("wrapper output summary"));
    assert!(!wrapper_text.contains("mcp.fixture.tool.call"));
    assert!(!wrapper_text.contains("Compat alias"));

    let expanded_wrapper = build_transcript_tool_call_section(
        &wrapper_call,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render =
                append_tool_call_section_lines(&expanded_wrapper, &theme, 120, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(expanded_text.contains("wrapper output summary"));
}

#[cfg(test)]
pub(crate) fn exact_test_generic_tool_successful_output_prefers_inline_background_rows() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-vendor-magic", "vendor.magic");
    tool_call.args_summary = r#"{"path":"notes.md","query":"child parity","limit":3}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool_call.output_summary = Some("line 1\nline 2\nline 3\nline 4".to_string());
    tool_call.timing_elapsed_ms = Some(250);

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(section.header.disclosure_state, None);

    let rendered = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    });

    assert!(rendered[0].contains("vendor.magic"));
    assert!(rendered[0].contains("child parity"));
    assert!(rendered[0].contains("[limit=3]"));
    assert!(rendered.iter().all(|line| !line.contains("line 1")));
    assert!(rendered
        .iter()
        .all(|line| !line.trim_start().starts_with('┃')));

    let visible = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        true,
        false,
        false,
        None,
    );
    assert_eq!(
        visible.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(
        visible.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );

    let visible_rendered = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&visible, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    });
    let visible_text = visible_rendered.join("\n");
    assert!(visible_text.contains("line 1"));
    assert!(visible_text.contains("line 3"));
    assert!(!visible_text.contains("line 4"));
    assert!(visible_text.contains("Click to expand"));
    assert!(visible_text.contains('…'));

    let expanded = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&expanded, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(expanded_text.contains("line 4"));
}

#[cfg(test)]
pub(crate) fn exact_test_lsp_tool_successful_output_stays_hidden_until_generic_output_enabled() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-lsp", "code.lsp");
    tool_call.args_summary =
        r#"{"operation":"goto_definition","filePath":"src/main.rs","line":12,"character":4}"#
            .to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool_call.output_summary = Some("result 1\nresult 2\nresult 3\nresult 4".to_string());

    let hidden = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        hidden.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(hidden.header.disclosure_state, None);

    let hidden_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&hidden, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(hidden_text.contains("lsp"));
    assert!(hidden_text.contains("src/main.rs"));
    assert!(hidden_text.contains("[operation=goto_definition]"));
    assert!(!hidden_text.contains("result 1"));

    let visible = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        true,
        false,
        false,
        None,
    );
    assert_eq!(
        visible.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );

    let visible_text = transcript_test_line_texts({
        let mut lines = Vec::new();
        {
            let render = append_tool_call_section_lines(&visible, &theme, 96, theme.surface.panel);
            lines.extend(render.lines);
        }
        lines
    })
    .join("\n");
    assert!(visible_text.contains("result 1"));
    assert!(visible_text.contains("Click to expand"));
    assert!(visible_text.contains('…'));
    assert!(!visible_text.contains("result 4"));
}

#[cfg(test)]
pub(crate) fn exact_test_todo_write_rows_render_open_checklist() {
    let app = AppState::new_live(None, false, None);
    let theme = Theme::default();

    let mut write_call = transcript_section_model_test_tool_call("tc-todo-write", "todo.write");
    write_call.status = ToolCallDisplayStatus::Succeeded;
    write_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    write_call.output_summary = Some("todo list updated".to_string());
    write_call.output_json = Some(serde_json::json!({
        "todos": [
            {"content": "Plan work", "status": "completed", "priority": "high"},
            {"content": "Implement UI", "status": "in_progress", "priority": "high"},
            {"content": "Verify tests", "status": "pending", "priority": "medium"}
        ]
    }));

    let mut read_call = transcript_section_model_test_tool_call("tc-todo-read", "todo.read");
    read_call.status = ToolCallDisplayStatus::Succeeded;
    read_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    read_call.output_summary = Some("[]".to_string());

    let mut compat_read_call = transcript_section_model_test_tool_call("tc-todoread", "todoread");
    compat_read_call.status = ToolCallDisplayStatus::Succeeded;
    compat_read_call.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    compat_read_call.output_summary = Some("[]".to_string());

    let write_section =
        build_tool_call_section(&write_call, &app, false, false, false, false, false, None)
            .expect("todo write should render in transcript");
    assert_eq!(
        write_section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(write_section.header.title, "1 of 3 todos completed");
    assert_eq!(write_section.header.disclosure_state, None);

    let rendered = transcript_test_line_texts({
        let render =
            append_tool_call_section_lines(&write_section, &theme, 96, theme.surface.panel);
        render.lines
    })
    .join("\n");
    assert!(rendered.contains("1 of 3 todos completed"));
    assert!(rendered.contains("[•] Implement UI"));
    assert!(rendered.contains("[✓] Plan work"));
    assert!(rendered.contains("[ ] Verify tests"));
    let active = rendered.find("[•] Implement UI").expect("active todo row");
    let completed = rendered.find("[✓] Plan work").expect("completed todo row");
    assert!(
        active < completed,
        "active todo should be pinned first\n{rendered}"
    );

    let mut cancelled_then_pending =
        transcript_section_model_test_tool_call("tc-todo-cancelled-pending", "todowrite");
    cancelled_then_pending.status = ToolCallDisplayStatus::Succeeded;
    cancelled_then_pending.lifecycle_state =
        Some(harness_core::event::ToolCallLifecycleState::Completed);
    cancelled_then_pending.output_json = Some(serde_json::json!([
        {"content": "Skip stale path", "status": "cancelled", "priority": "low"},
        {"content": "Pick next path", "status": "pending", "priority": "high"}
    ]));
    let cancelled_then_pending_section = build_tool_call_section(
        &cancelled_then_pending,
        &app,
        false,
        false,
        false,
        false,
        false,
        None,
    )
    .expect("compat todo write should render bare-array output");
    let cancelled_then_pending_rendered = transcript_test_line_texts({
        let render = append_tool_call_section_lines(
            &cancelled_then_pending_section,
            &theme,
            96,
            theme.surface.panel,
        );
        render.lines
    })
    .join("\n");
    let pending = cancelled_then_pending_rendered
        .find("[ ] Pick next path")
        .expect("pending todo row");
    let cancelled = cancelled_then_pending_rendered
        .find("[✓] Skip stale path")
        .expect("cancelled todo row");
    assert!(
        pending < cancelled,
        "pending todo should outrank cancelled fallback\n{cancelled_then_pending_rendered}"
    );

    let mut structured_output =
        transcript_section_model_test_tool_call("tc-todo-structured-output", "todo.write");
    structured_output.status = ToolCallDisplayStatus::Succeeded;
    structured_output.lifecycle_state =
        Some(harness_core::event::ToolCallLifecycleState::Completed);
    structured_output.output_json = Some(serde_json::json!({
        "structured_output": {
            "todos": [
                {"content": "Render nested todos", "status": "pending", "priority": "medium"}
            ]
        }
    }));
    let structured_output_section = build_tool_call_section(
        &structured_output,
        &app,
        false,
        false,
        false,
        false,
        false,
        None,
    )
    .expect("todo write should render structured_output todos");
    let structured_output_rendered = transcript_test_line_texts({
        let render = append_tool_call_section_lines(
            &structured_output_section,
            &theme,
            96,
            theme.surface.panel,
        );
        render.lines
    })
    .join("\n");
    assert!(structured_output_rendered.contains("[ ] Render nested todos"));

    assert!(
        build_tool_call_section(&read_call, &app, true, false, false, false, false, None).is_none(),
        "todo reads should stay hidden because they do not update visible state"
    );
    assert!(build_tool_call_section(
        &compat_read_call,
        &app,
        true,
        false,
        false,
        false,
        false,
        None,
    )
    .is_none());
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_task_rows_show_child_status_duration_and_counts() {
    fn event(
        seq: u64,
        ts: &str,
        actor: harness_core::event::EventActor,
        correlation_id: Option<&str>,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_task_rows_{seq:04}"),
            seq,
            run_id: "run_task_rows".to_string(),
            mono_ms: seq * 100,
            ts: Some(ts.to_string()),
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        "2026-03-22T14:36:00Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::User,
            Some("interactive-user".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent".to_string(),
                text: "Audit transcript parity".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "2026-03-22T14:36:01Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_parent".to_string(),
                provider_id: "default".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Audit transcript parity".to_string(),
                request_digest: "digest-parent".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "2026-03-22T14:36:02Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task".to_string(),
                tool_id: "task".to_string(),
                args_summary:
                    r#"{"description":"audit transcript parity","subagent_type":"researcher"}"#
                        .to_string(),
                args_digest: "digest-task-call".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("task".to_string()),
                    alias_source_tool_id: None,
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "2026-03-22T14:36:03Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_task".to_string(),
        }),
    ));
    app.ingest_event(event(
        5,
        "2026-03-22T14:36:04Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_child".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:researcher".to_string()),
        }),
    ));
    app.ingest_event(event(
        6,
        "2026-03-22T14:36:05Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_child_read".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
                args_digest: "digest-child-read".to_string(),
                metadata: None,
            },
        ),
    ));

    let running_tool = &app.activities[0].tool_calls[0];
    let running_row = app.transcript_task_row_for_tool_call(running_tool);
    let running_section = build_transcript_tool_call_section(
        running_tool,
        &AppState::default(),
        running_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let mut running_lines = Vec::new();
    {
        let render = append_tool_call_section_lines(
            &running_section,
            &Theme::default(),
            120,
            Theme::default().surface.panel,
        );
        running_lines.extend(render.lines);
    }
    let running_text = transcript_test_line_texts(running_lines).join("\n");
    assert!(running_text.contains("Researcher Task — audit transcript parity"));
    assert!(
        running_text.contains("↳ Read src/ui.rs"),
        "running task row should keep child-session context inline\n{running_text}"
    );
    assert!(!running_text.contains("↳ 1 toolcalls"));
    assert!(!running_text.contains("1 toolcalls · 100ms"));

    app.ingest_event(event(
        7,
        "2026-03-22T14:36:06Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_child_grep".to_string(),
                tool_id: "fs.grep".to_string(),
                args_summary: r#"{"pattern":"task row"}"#.to_string(),
                args_digest: "digest-child-grep".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "2026-03-22T14:36:07Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_child".to_string(),
                delta: "CHILD SUBAGENT DETAILS SHOULD STAY OUT OF PARENT".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        9,
        "2026-03-22T14:36:07Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_child".to_string(),
            result_summary: "Found the inline transcript path.".to_string(),
            result_digest: "digest-task-child".to_string(),
            metadata: Some(harness_core::event::TaskCompletionMetadata {
                lineage: Some(harness_core::event::TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_task".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_worker".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..harness_core::event::TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: Some(harness_core::event::ExecutionTimingMetadata {
                    started_mono_ms: Some(400),
                    finished_mono_ms: Some(2_000),
                    elapsed_ms: Some(1_600),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    app.ingest_event(event(
        10,
        "2026-03-22T14:36:08Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_task".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("child session finished".to_string()),
                output_digest: Some("digest-task-finished".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let completed_tool = &app.activities[0].tool_calls[0];
    let completed_row = app.transcript_task_row_for_tool_call(completed_tool);
    let completed_section = build_transcript_tool_call_section(
        completed_tool,
        &AppState::default(),
        completed_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let mut completed_lines = Vec::new();
    {
        let render = append_tool_call_section_lines(
            &completed_section,
            &Theme::default(),
            120,
            Theme::default().surface.panel,
        );
        completed_lines.extend(render.lines);
    }
    let completed_text = transcript_test_line_texts(completed_lines).join("\n");
    assert!(completed_text.contains("Researcher Task — audit transcript parity"));
    assert!(
        completed_text.contains("2 toolcalls · 1.6s"),
        "completed task row should keep child-session context inline\n{completed_text}"
    );
    assert!(!completed_text.contains("Found the inline transcript path."));
    assert!(!completed_text.contains("child session finished"));

    let expanded_completed_section = build_transcript_tool_call_section(
        completed_tool,
        &AppState::default(),
        completed_row.as_ref(),
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_completed_render = append_tool_call_section_lines(
        &expanded_completed_section,
        &Theme::default(),
        120,
        Theme::default().surface.panel,
    );
    let expanded_completed_text =
        transcript_test_line_texts(expanded_completed_render.lines).join("\n");
    assert!(!expanded_completed_text.contains("Found the inline transcript path."));
    assert!(!expanded_completed_text.contains("child session finished"));

    let parent_transcript_text = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ))
    .join("\n");
    assert!(parent_transcript_text.contains("Researcher Task — audit transcript parity"));
    assert!(parent_transcript_text.contains("2 toolcalls · 1.6s"));
    assert!(!parent_transcript_text.contains("ctrl+] view subagents"));
    assert!(
        !parent_transcript_text.contains("CHILD SUBAGENT DETAILS SHOULD STAY OUT OF PARENT"),
        "parent transcript should keep delegated child turns behind the task row\n{parent_transcript_text}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_block_tool_cards_skip_empty_subtitle_rows() {
    let theme = Theme::default();

    let mut shell_call = transcript_section_model_test_tool_call("tc-shell-card", "shell.run");
    shell_call.args_summary =
        r#"{"cmd":"cargo test -p harness-tui","cwd":"/tmp/demo"}"#.to_string();
    shell_call.status = ToolCallDisplayStatus::Failed;
    shell_call.output_summary = Some("exit code: 1\nstderr: snapshot mismatch".to_string());
    shell_call.truncated_output = shell_call.output_summary.clone();
    shell_call.first_mono_ms = 10;
    shell_call.last_mono_ms = 0;

    let section = build_transcript_tool_call_section(
        &shell_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(section.header.subtitle, None);
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );

    let mut lines = Vec::new();
    {
        let render = append_tool_call_section_lines(&section, &theme, 120, theme.surface.panel);
        lines.extend(render.lines);
    }
    let text_lines = transcript_test_line_texts(lines);

    let command_row = text_lines
        .iter()
        .position(|line| line.contains("cargo test -p harness-tui"))
        .expect("shell inline command");
    let exit_row = text_lines
        .iter()
        .position(|line| line.contains("exit code: 1"))
        .expect("shell inline error exit");
    let stderr_row = text_lines
        .iter()
        .position(|line| line.contains("stderr: snapshot mismatch"))
        .expect("shell inline error stderr");

    assert!(command_row < exit_row && exit_row < stderr_row);
    assert!(
        !text_lines.iter().any(|line| line.contains("# Shell")),
        "failed shell summaries without structured output should stay inline like harness tool rows\n{text_lines:#?}"
    );
    assert!(
        !text_lines.iter().any(|line| line.contains("● ● ●")),
        "block tool rows should not render the removed fake terminal header icon\n{text_lines:#?}"
    );
    assert!(
        !text_lines
            .iter()
            .any(|line| line.contains('╭') || line.contains('╰') || line.contains('│')),
        "block tool rows should not render a rounded window frame\n{text_lines:#?}"
    );
}

#[cfg(test)]
fn failed_tool_cards_parse_legacy_error_copy() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-webfetch-failure", "web.fetch");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some(
        "Error: webfetch Request failed: 404 Not Found while fetching https://example.com"
            .to_string(),
    );

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Request failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && text == "404 Not Found while fetching https://example.com"
        )
    }));
}

#[cfg(test)]
fn failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators() {
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-webfetch-lowercase", "web.fetch");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some("error: webfetch: Request failed: 404 Not Found".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Request failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error && text == "404 Not Found"
        )
    }));
}

#[cfg(test)]
fn denied_tool_cards_use_denied_subtitle() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-denied-failure", "shell.run");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.permissions = vec![crate::app::PermissionEntry {
        permission_id: "perm-denied".to_string(),
        kind: "shell".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        summary: "Shell execution denied".to_string(),
        request_digest: "digest-denied".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Deny),
        resolution_reason: Some("Policy denied shell execution".to_string()),
        first_seq: 1,
        last_seq: 2,
    }];

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Denied".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text: output, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && output.contains("Policy denied shell execution")
        )
    }));
}

#[cfg(test)]
fn denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-denied-colon", "shell.run");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.permissions = vec![crate::app::PermissionEntry {
        permission_id: "perm-denied-colon".to_string(),
        kind: "shell".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        summary: "Shell execution denied".to_string(),
        request_digest: "digest-denied-colon".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Deny),
        resolution_reason: Some("Permission denied: shell execution blocked".to_string()),
        first_seq: 1,
        last_seq: 2,
    }];

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Denied".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text: output, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && output.contains("shell execution blocked")
        )
    }));
}

#[cfg(test)]
fn generic_failed_tool_messages_do_not_split_arbitrary_prefixes() {
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-generic-url-failure", "vendor.remote");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary =
        Some("Error: GET https://example.com: connection refused".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && text == "GET https://example.com: connection refused"
        )
    }));
}

#[cfg(test)]
fn failed_tool_cards_fallback_when_error_details_are_missing() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-empty-failure", "tool.batch");
    tool_call.status = ToolCallDisplayStatus::Failed;

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && text == "No error details available."
        )
    }));
}

#[cfg(test)]
#[test]
fn question_tool_cards_render_answered_question_details() {
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-question-answered", "user.question");
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.permissions = vec![crate::app::PermissionEntry {
        permission_id: "perm-question-answered".to_string(),
        kind: "question".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        summary: serde_json::json!({
            "questions": [
                {
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                },
                {
                    "question": "Pick another",
                    "header": "Mode",
                    "options": [{"label": "B", "description": "Option B"}],
                }
            ]
        })
        .to_string(),
        request_digest: "digest-question-answered".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Allow),
        resolution_reason: Some("[[\"A\"],[]]".to_string()),
        first_seq: 1,
        last_seq: 2,
    }];

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.title, "Questions");
    assert_eq!(
        section.header.subtitle,
        Some("answered 2 questions".to_string())
    );
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Primary && text == "Pick one"
        )
    }));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Secondary && text == "↳ A"
        )
    }));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Secondary
                    && text == "↳ (no answer)"
        )
    }));
}

#[test]
fn consecutive_tool_rows_do_not_insert_terminal_blank_rows() {
    let mut app = AppState::default();
    let mut activity =
        transcript_section_model_test_activity("request-tool-stacking", ActivityStatus::Done, "");

    let mut cancel_tool =
        transcript_section_model_test_tool_call("tc-cancel-stacking", "background.cancel");
    cancel_tool.status = ToolCallDisplayStatus::Succeeded;
    cancel_tool.args_summary = r#"{"taskId":"bg_123"}"#.to_string();
    cancel_tool.first_mono_ms = 10;
    cancel_tool.last_mono_ms = 20;

    let mut lsp_tool = transcript_section_model_test_tool_call("tc-lsp-stacking", "code.lsp");
    lsp_tool.status = ToolCallDisplayStatus::Succeeded;
    lsp_tool.args_summary = r#"{"operation":"goto_definition","symbol":"AppState"}"#.to_string();
    lsp_tool.first_mono_ms = 30;
    lsp_tool.last_mono_ms = 45;

    activity.tool_calls = vec![cancel_tool, lsp_tool];
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let surfaces = &layout.sections[0].surfaces;

    let cancel_surface = surfaces
        .iter()
        .find(|surface| {
            surface.lines.iter().any(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("background.cancel"))
            })
        })
        .expect("background.cancel surface");
    let lsp_surface = surfaces
        .iter()
        .find(|surface| {
            surface.lines.iter().any(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("lsp"))
            })
        })
        .expect("lsp surface");

    assert_eq!(
        lsp_surface.top_offset,
        cancel_surface.top_offset + cancel_surface.height,
        "consecutive tool surfaces should stack without an extra blank terminal row"
    );
}

#[test]
fn block_tool_cards_render_subtitle_inline_with_title() {
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tool-agent-spawn".to_string(),
        header: TranscriptToolCallHeader {
            tool_id: "agent.spawn".to_string(),
            title: "Spawn researcher · audit transcript parity".to_string(),
            subtitle: Some("14:36 · 1.6s".to_string()),
            path_metadata: None,
            icon: Some("↗"),
            status: ToolCallDisplayStatus::Succeeded,
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: vec![TranscriptToolCallDetailBlock::Message {
            text: "┃ agent_worker · req_child · completed · 2 child tool calls".to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        }],
        expanded: false,
    };

    let mut lines = Vec::new();
    {
        let render = append_tool_call_section_lines(&section, &theme, 120, theme.surface.panel);
        lines.extend(render.lines);
    }
    let text_lines = transcript_test_line_texts(lines);

    let title_row = text_lines
        .iter()
        .position(|line| line.contains("Spawn researcher · audit transcript parity · 14:36 · 1.6s"))
        .expect("block tool title row");

    assert!(
        text_lines[title_row].contains("Spawn researcher · audit transcript parity · 14:36 · 1.6s"),
        "block tool card title row should keep subtitle metadata inline\n{text_lines:#?}"
    );
    assert!(
        !text_lines
            .iter()
            .enumerate()
            .any(|(index, line)| index != title_row && line.contains("14:36 · 1.6s")),
        "block tool cards should not dedicate a separate subtitle row\n{text_lines:#?}"
    );
}

#[test]
fn shell_tool_cards_use_harness_bash_styling_values() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-harness", "shell.run");
    tool_call.args_summary = r#"{"command":"echo hi","description":"list files"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some(
        (1..=11)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel {
            command: "echo hi".to_string(),
            output:
                "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n…"
                    .to_string(),
            description: Some("list files".to_string()),
            expand_hint: Some("Click to expand".to_string()),
            tone: TranscriptToolCallDetailTone::Primary,
        }
    );

    let text_lines = transcript_test_line_texts({
        let mut lines = Vec::new();
        let render = append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel);
        lines.extend(render.lines);
        lines
    });
    let rendered = text_lines.join("\n");

    assert!(rendered.contains('┃'));
    assert!(!rendered.contains("● ● ●"));
    assert!(rendered.contains("# list files"));
    assert!(!rendered.contains('╭'));
    assert!(!rendered.contains('├'));
    assert!(rendered.contains("$ echo hi"));
    assert!(!rendered.contains("stdout>"));
    assert!(rendered.contains("line 10"));
    assert!(!rendered.contains("line 11"));
    assert!(rendered.contains("Click to expand"));
    assert!(!rendered.contains('╰'));
}

#[test]
fn shell_tool_cards_render_workdir_in_harness_title() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-workdir", "bash");
    tool_call.args_summary =
        r#"{"command":"pwd","description":"show cwd","workdir":"crates/harness-tui"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("/workspace/crates/harness-tui".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        Some(Path::new("/workspace")),
    );

    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { description, .. }
            if description.as_deref() == Some("show cwd in /workspace/crates/harness-tui")
    ));
    let rendered = transcript_test_line_texts({
        let render = append_tool_call_section_lines(
            &section,
            &Theme::default(),
            96,
            Theme::default().surface.panel,
        );
        render.lines
    })
    .join("\n");
    assert!(rendered.contains("# show cwd in /workspace/crates/harness-tui"));
}

#[test]
fn shell_tool_cards_render_cmd_with_args_and_structured_output() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-cmd-args", "shell.run");
    tool_call.args_summary =
        r#"{"cmd":"bash","args":["-lc","printf shell-run"],"cwd":"."}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = None;
    tool_call.output_json = Some(serde_json::json!({
        "stdout": "shell-run\n",
        "stderr": "",
        "status": 0,
        "success": true,
        "truncated": false
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        Some(Path::new("/workspace")),
    );

    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { command, output, .. }
            if command == "bash -lc printf shell-run" && output == "shell-run"
    ));
}

#[test]
fn failed_structured_shell_output_does_not_duplicate_matching_error_summary() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-structured-fail", "bash");
    tool_call.args_summary = r#"{"command":"false"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some("boom".to_string());
    tool_call.output_json = Some(serde_json::json!({
        "stdout": "",
        "stderr": "boom",
        "status": 1,
        "success": false,
        "truncated": false
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.detail_blocks.len(), 1);
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, tone, .. }
            if output == "boom" && *tone == TranscriptToolCallDetailTone::Primary
    ));
}

#[test]
fn shell_tool_cards_strip_ansi_from_output() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-ansi", "bash");
    tool_call.args_summary = r#"{"command":"printf color"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("\u{1b}[31mred\u{1b}[0m\nplain".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, .. }
            if output == "red\nplain"
    ));
}

#[test]
fn shell_tool_aliases_use_canonical_bash_card_path() {
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-shell-alias", "shell.run.wrapper");
    tool_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("shell.run.wrapper".to_string()),
        effective_tool_id: Some("bash".to_string()),
        canonical_tool_id: Some("bash".to_string()),
        alias_source_tool_id: Some("shell.run".to_string()),
    });
    tool_call.args_summary = r#"{"command":"echo alias"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("alias".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(section.header.tool_id, "bash");
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(matches!(
        section.detail_blocks.first(),
        Some(TranscriptToolCallDetailBlock::BashPanel { .. })
    ));
}

#[test]
fn completed_empty_shell_output_keeps_block_card() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-empty", "bash");
    tool_call.args_summary = r#"{"command":"true"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_json = Some(serde_json::json!({
        "stdout": "",
        "stderr": "",
        "status": 0,
        "success": true,
        "truncated": false
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { command, output, expand_hint, .. }
            if command == "true" && output.is_empty() && expand_hint.is_none()
    ));
}

#[test]
fn running_shell_without_output_metadata_uses_inline_fallback_until_output_event() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-running-empty", "bash");
    tool_call.args_summary = r#"{"command":"sleep 1"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Running;

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    // The harness transcript model has no running-output presence bit while a
    // shell call is running without output. Until a result event carries
    // `output_summary` or structured stdout/stderr, the closest deterministic
    // equivalent is the harness inline shell row.
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(section.header.icon, Some("$"));
    assert!(section.detail_blocks.is_empty());
}

#[test]
fn shell_tool_cards_toggle_overflow_expand_and_collapse_hints() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-overflow", "bash");
    tool_call.args_summary = r#"{"command":"seq 12"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some(
        (1..=12)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let collapsed = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    let expanded = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        true,
        false,
        None,
    );

    assert!(matches!(
        &collapsed.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, expand_hint, .. }
            if output.ends_with("line 10\n…")
                && !output.contains("line 11")
                && expand_hint.as_deref() == Some("Click to expand")
    ));
    assert!(matches!(
        &expanded.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, expand_hint, .. }
            if output.contains("line 12")
                && expand_hint.as_deref() == Some("Click to collapse")
    ));
}

#[cfg(test)]
pub(crate) fn exact_test_inline_tool_rows_wrap_long_subtitles_cleanly() {
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tool-inline-read".to_string(),
        header: TranscriptToolCallHeader {
            tool_id: "fs.read".to_string(),
            title: "Read src/ui.rs [offset=12, limit=24]".to_string(),
            subtitle: Some(
                "14:35 · 1.2s · foreground · agent_worker · req_child · completed · 3 child tool calls"
                    .to_string(),
            ),
            path_metadata: None,
            icon: None,
            status: ToolCallDisplayStatus::Succeeded,
            visual_style: TranscriptToolCallVisualStyle::Inline,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: Vec::new(),
        expanded: false,
    };

    let mut lines = Vec::new();
    {
        let render = append_tool_call_section_lines(&section, &theme, 56, theme.surface.panel);
        lines.extend(render.lines);
    }
    let text_lines = transcript_test_line_texts(lines);

    assert!(
        text_lines.len() >= 2,
        "long inline subtitles should wrap in narrow widths"
    );
    assert!(text_lines[0].contains("Read src/ui.rs [offset=12, limit=24]"));
    assert!(text_lines.iter().any(|line| line.contains("14:35 · 1.2s")));
    assert!(text_lines
        .iter()
        .any(|line| line.contains("foreground · agent_worker")));
    assert!(
        text_lines.iter().any(|line| line.contains("completed · 3")),
        "wrapped inline subtitle should preserve completion count metadata\n{text_lines:#?}"
    );
    assert!(
        text_lines
            .iter()
            .any(|line| line.contains("child tool calls")),
        "wrapped inline subtitle should preserve child-call wording after wrap\n{text_lines:#?}"
    );
    assert!(text_lines.iter().all(|line| line.starts_with("   ")));
}

#[cfg(test)]
fn transcript_section_model_test_activity(
    request_id: &str,
    status: ActivityStatus,
    transcript_text: &str,
) -> ActivityEntry {
    ActivityEntry {
        request_id: request_id.to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: transcript_text.to_string(),
        usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }
}

#[cfg(test)]
fn transcript_section_model_test_tool_call(
    tool_call_id: &str,
    tool_id: &str,
) -> crate::app::ToolCallEntry {
    crate::app::ToolCallEntry {
        tool_call_id: tool_call_id.to_string(),
        tool_id: tool_id.to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: "{}".to_string(),
        args_digest: "digest".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Queued,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 0,
        last_mono_ms: 0,
        first_timestamp: None,
        last_timestamp: None,
    }
}

#[cfg(test)]
fn transcript_test_line_texts(lines: Vec<Line<'static>>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::UserMessageSubmittedEvent;

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
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
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
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
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
    fn transcript_scrollbar_layout_matches_shell_lane_width() {
        let viewport = transcript_viewport_layout(Rect::new(4, 2, 20, 12), true);

        assert_eq!(viewport.content, Rect::new(4, 2, 19, 12));
        assert_eq!(viewport.scrollbar_chrome, Some(Rect::new(23, 2, 1, 12)));
        assert_eq!(viewport.scrollbar_lane, Some(Rect::new(23, 2, 1, 12)));
    }

    #[test]
    fn transcript_scrollbar_track_uses_full_height_without_vertical_padding() {
        let track = transcript_scrollbar_track_rect(Rect::new(23, 2, 1, 12));

        assert_eq!(track, Rect::new(23, 2, 1, 12));
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
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            usage: None,
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
