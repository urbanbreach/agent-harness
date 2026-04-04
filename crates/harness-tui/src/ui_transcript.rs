use std::path::Path;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Theme as SyntectTheme};
use syntect::parsing::SyntaxSet;

use super::*;

use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;

use super::ui_secondary::{format_detail_payload, render_structured_diff_lines};

struct LabeledTextBlockStyle<'a> {
    indent: &'a str,
    label: &'a str,
    rail_color: Color,
    surface: Color,
    label_style: Style,
    body_style: Style,
}

struct SyntaxHighlightAssets {
    syntax_set: SyntaxSet,
    theme: SyntectTheme,
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
    show_outer_rail: bool,
    rail_color: Color,
    surface: Color,
    lines: Vec<Line<'static>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptSection {
    Turn(Box<TranscriptTurnSection>),
    PendingPermission(TranscriptPendingPermissionSection),
}

struct BuildTurnSectionArgs<'a> {
    activity: &'a ActivityEntry,
    is_selected: bool,
    thinking_visible: bool,
    timestamps_visible: bool,
    show_tool_details: bool,
    show_generic_tool_output: bool,
    stacked_diffs: bool,
    profile_label: &'a str,
    session_path: Option<&'a Path>,
    app: &'a AppState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTurnSection {
    request_id: String,
    user_message: Option<TranscriptUserMessageSection>,
    header: TranscriptTurnHeader,
    body_blocks: Vec<TranscriptBodyBlock>,
    tool_calls: Vec<TranscriptToolCallSection>,
    thinking: Option<TranscriptLabeledTextSection>,
    error: Option<TranscriptErrorSection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptUserMessageSection {
    text: String,
    timestamp: Option<String>,
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
    WaitingResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptLabeledTextSection {
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptToolCallSection {
    header: TranscriptToolCallHeader,
    detail_blocks: Vec<TranscriptToolCallDetailBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptToolCallHeader {
    tool_id: String,
    title: String,
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
    StructuredDiff {
        diff_content: String,
        fallback_path: Option<String>,
        force_stacked: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptToolCallVisualStyle {
    Inline,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscriptToolCallDetailTone {
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
const TRANSCRIPT_SURFACE_BODY_PREFIX: &str = " ";
const TRANSCRIPT_USER_BODY_PREFIX: &str = "  ";
const TRANSCRIPT_ASSISTANT_BODY_PREFIX: &str = "   ";
const TRANSCRIPT_NESTED_INDENT: &str = "  ";
const TRANSCRIPT_OPCODE_EDIT_INDENT: &str = "    ";
const TRANSCRIPT_RAIL_GLYPH: &str = "┃";

pub(super) fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if !app.replay_mode {
        let vertical_gutter = if app.startup_shell_visible() || live_empty_state_visible(app) {
            0
        } else {
            theme.live_shell.rhythm.transcript_gutter_y
        };
        let inner_area = inset_rect(
            area,
            theme.live_shell.rhythm.transcript_gutter_x,
            vertical_gutter,
        );

        if app.startup_shell_visible() {
            render_startup_lifecycle_surface(frame, app, inner_area, theme);
            return;
        }

        if live_empty_state_visible(app) {
            render_live_empty_state(frame, app, inner_area, theme);
            return;
        }

        render_measured_transcript_pane(frame, app, inner_area, theme, theme.surface.shell);
        return;
    }

    if app.active_tab == Tab::Run {
        let inner_area = inset_rect(
            area,
            theme.live_shell.rhythm.transcript_gutter_x,
            theme.live_shell.rhythm.transcript_gutter_y,
        );

        render_measured_transcript_pane(frame, app, inner_area, theme, theme.surface.panel);
        return;
    }

    let is_focused = transcript_surface_focused(app);

    let title = if app.replay_mode {
        format!(
            "Transcript{}{}",
            if is_focused { " (focused)" } else { "" },
            if app.follow_mode { " (following)" } else { "" }
        )
    } else {
        format!(
            "Conversation{}{}",
            if is_focused { " (focused)" } else { "" },
            if app.follow_mode { " (following)" } else { "" }
        )
    };

    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);

    let inner_area = inset_rect(
        block.inner(area),
        theme.live_shell.rhythm.transcript_gutter_x,
        theme.live_shell.rhythm.transcript_gutter_y,
    );

    frame.render_widget(block, area);

    if live_empty_state_visible(app) {
        render_live_empty_state(frame, app, inner_area, theme);
        return;
    }

    render_measured_transcript_pane(frame, app, inner_area, theme, surface);
}

fn render_measured_transcript_pane(
    frame: &mut Frame,
    app: &AppState,
    inner_area: Rect,
    theme: &Theme,
    empty_surface: Color,
) {
    let layout = build_measured_transcript_layout_for_width_on_surface(
        app,
        theme,
        inner_area.width,
        empty_surface,
    );
    if layout.sections.is_empty() {
        frame.render_widget(
            Paragraph::new(Text::from(vec![Line::from(Span::styled(
                "Waiting for first turn…",
                Style::default().fg(theme.text.secondary),
            ))]))
            .style(panel_style(empty_surface, theme.text.primary))
            .wrap(Wrap { trim: false }),
            inner_area,
        );
        return;
    }

    let transcript_scroll = usize::from(transcript_scroll_offset(app, &layout, inner_area));
    render_transcript_layout_surfaces(frame, &layout, inner_area, transcript_scroll, theme);
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
    let layout = build_measured_transcript_layout_for_width(app, theme, width);
    transcript_layout_lines(&layout, theme)
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
    let sections = build_transcript_sections(app);
    measure_transcript_layout(&sections, theme, width, base_surface)
}

fn build_transcript_sections(app: &AppState) -> Vec<TranscriptSection> {
    let mut sections = Vec::new();

    for (index, activity) in app.activities.iter().enumerate() {
        sections.push(TranscriptSection::Turn(Box::new(build_turn_section(
            BuildTurnSectionArgs {
                activity,
                is_selected: index == app.selected_activity_index,
                thinking_visible: app.transcript_thinking_visible(),
                timestamps_visible: app.transcript_timestamps_visible(),
                show_tool_details: app.tool_details_visible(),
                show_generic_tool_output: app.generic_tool_output_visible(),
                stacked_diffs: app.stacked_transcript_diffs(),
                profile_label: app.active_profile(),
                session_path: app.session_path.as_deref(),
                app,
            },
        ))));
    }

    for (_permission_id, summary) in app.transcript_pending_permissions() {
        sections.push(TranscriptSection::PendingPermission(
            TranscriptPendingPermissionSection { summary },
        ));
    }

    sections
}

fn build_turn_section(args: BuildTurnSectionArgs<'_>) -> TranscriptTurnSection {
    let BuildTurnSectionArgs {
        activity,
        is_selected,
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
                timestamp: timestamps_visible
                    .then_some(activity.user_timestamp.as_deref())
                    .flatten()
                    .map(format_transcript_timestamp),
            });

    let thinking = (thinking_visible && !activity.thinking_text.trim().is_empty()).then(|| {
        TranscriptLabeledTextSection {
            text: activity.thinking_text.clone(),
        }
    });

    let mut body_blocks = Vec::new();
    if !activity.transcript_text.is_empty() {
        body_blocks.push(TranscriptBodyBlock::RichText(
            activity.transcript_text.clone(),
        ));
    } else if activity.status == ActivityStatus::Streaming {
        body_blocks.push(TranscriptBodyBlock::WaitingResponse);
    }

    TranscriptTurnSection {
        request_id: activity.request_id.clone(),
        user_message,
        header: TranscriptTurnHeader {
            status: activity.status,
            is_selected,
            profile_label: profile_label.to_string(),
            model_id: activity.model_id.clone(),
            duration_ms: activity.duration_ms(),
        },
        body_blocks,
        tool_calls: activity
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
            })
            .collect(),
        thinking,
        error: activity
            .error_message
            .as_ref()
            .map(|text| TranscriptErrorSection { text: text.clone() }),
    }
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
    if tool_call.status == ToolCallDisplayStatus::Succeeded && !show_tool_details {
        return None;
    }

    let task_row = app.transcript_task_row_for_tool_call(tool_call);

    Some(build_opencode_tool_call_section(
        tool_call,
        task_row.as_ref(),
        timestamps_visible,
        show_generic_tool_output,
        tool_output_expanded,
        stacked_diffs,
        session_path,
    ))
}

fn build_opencode_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    timestamps_visible: bool,
    show_generic_tool_output: bool,
    tool_output_expanded: bool,
    stacked_diffs: bool,
    session_path: Option<&Path>,
) -> TranscriptToolCallSection {
    let struck_out = tool_call_denied(tool_call);
    let mut detail_blocks = Vec::new();
    let expanded = show_generic_tool_output || tool_output_expanded;
    let display_tool_id = tool_call.canonical_tool_id();

    let (title, icon, visual_style) = match display_tool_id {
        "fs.read" => (
            format!(
                "Read {}{}",
                tool_path_display(tool_call).unwrap_or_else(|| "file".to_string()),
                read_tool_input_suffix(tool_call),
            ),
            Some("→"),
            TranscriptToolCallVisualStyle::Inline,
        ),
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
        ),
        "fs.ls" => (
            format!(
                "List {}",
                tool_summary_string(&tool_call.args_summary, &["path"])
                    .unwrap_or_else(|| ".".to_string()),
            ),
            Some("→"),
            TranscriptToolCallVisualStyle::Inline,
        ),
        "shell.run" => {
            let cmd = tool_summary_string(&tool_call.args_summary, &["cmd", "command"])
                .unwrap_or_else(|| "Shell".to_string());
            if tool_call.output_summary.is_some() {
                detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                    text: format!("$ {cmd}"),
                    tone: TranscriptToolCallDetailTone::Secondary,
                });
                if let Some(output) = tool_call.output_summary.as_deref() {
                    push_collapsible_output_block(
                        &mut detail_blocks,
                        output,
                        10,
                        expanded,
                        if tool_call.status == ToolCallDisplayStatus::Failed {
                            TranscriptToolCallDetailTone::Error
                        } else {
                            TranscriptToolCallDetailTone::Secondary
                        },
                    );
                }
                (
                    shell_block_title(&tool_call.args_summary),
                    None,
                    TranscriptToolCallVisualStyle::Block,
                )
            } else {
                (cmd, Some("$"), TranscriptToolCallVisualStyle::Inline)
            }
        }
        "edit.hashline_apply" => {
            let path = tool_call.edit_path_display();
            let title = match tool_call.edit.as_ref().map(|edit| edit.status) {
                Some(crate::app::EditDisplayStatus::Applied) => path
                    .as_ref()
                    .map(|path| format!("← Patched {path}"))
                    .unwrap_or_else(|| "← Patched file".to_string()),
                Some(crate::app::EditDisplayStatus::Rejected)
                | Some(crate::app::EditDisplayStatus::Proposed) => path
                    .as_ref()
                    .map(|path| format!("← Edit {path}"))
                    .unwrap_or_else(|| "← Preparing edit...".to_string()),
                None => path
                    .as_ref()
                    .map(|path| format!("← Edit {path}"))
                    .unwrap_or_else(|| "← Preparing edit...".to_string()),
            };

            if let Some(edit) = &tool_call.edit {
                if edit.status == crate::app::EditDisplayStatus::Applied {
                    if let Some(diff_rel_path) = edit.diff_rel_path.as_deref() {
                        if let Some(session_path) = session_path {
                            if let Ok(diff_content) =
                                std::fs::read_to_string(session_path.join(diff_rel_path))
                            {
                                detail_blocks.push(TranscriptToolCallDetailBlock::StructuredDiff {
                                    diff_content,
                                    fallback_path: Some(edit.path.clone()),
                                    force_stacked: stacked_diffs,
                                });
                            }
                        }
                    }
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

            (title, None, TranscriptToolCallVisualStyle::Block)
        }
        "edit.hashline_scan" => (
            format!(
                "Scan {}",
                tool_path_display(tool_call).unwrap_or_else(|| "file".to_string())
            ),
            Some("→"),
            TranscriptToolCallVisualStyle::Inline,
        ),
        "agent.spawn" => build_agent_spawn_tool_row(tool_call, task_row, &mut detail_blocks),
        "fs.write" => (
            format!(
                "Write {}",
                tool_summary_string(&tool_call.args_summary, &["filePath", "path"])
                    .unwrap_or_else(|| "file".to_string())
            ),
            Some("←"),
            TranscriptToolCallVisualStyle::Inline,
        ),
        "apply_patch" => (
            "Apply patch".to_string(),
            Some("←"),
            TranscriptToolCallVisualStyle::Block,
        ),
        "web.fetch" => (
            format!(
                "Fetch {}",
                tool_summary_string(&tool_call.args_summary, &["url"])
                    .unwrap_or_else(|| "url".to_string())
            ),
            Some("⇣"),
            TranscriptToolCallVisualStyle::Block,
        ),
        "search.web" | "search.code" => (
            format!(
                "{} \"{}\"",
                if display_tool_id == "search.web" {
                    "Search"
                } else {
                    "Code search"
                },
                tool_summary_string(&tool_call.args_summary, &["query"])
                    .unwrap_or_else(|| "query".to_string())
            ),
            Some("✱"),
            TranscriptToolCallVisualStyle::Block,
        ),
        "todo.write" | "todo.read" => (
            if display_tool_id == "todo.write" {
                "Update todos".to_string()
            } else {
                "Read todos".to_string()
            },
            Some("☑"),
            TranscriptToolCallVisualStyle::Inline,
        ),
        "skill.load" => (
            format!(
                "Load skill {}",
                tool_summary_string(&tool_call.args_summary, &["name"])
                    .unwrap_or_else(|| "skill".to_string())
            ),
            Some("✦"),
            TranscriptToolCallVisualStyle::Inline,
        ),
        "user.question" => (
            "Ask question".to_string(),
            Some("?"),
            TranscriptToolCallVisualStyle::Block,
        ),
        "tool.batch" => (
            batch_tool_title(tool_call),
            Some("≋"),
            TranscriptToolCallVisualStyle::Block,
        ),
        "code.lsp" => (
            lsp_tool_title(tool_call),
            Some("⌘"),
            TranscriptToolCallVisualStyle::Block,
        ),
        _ => {
            let title = generic_tool_title(tool_call, display_tool_id);
            if tool_call.output_summary.is_some() {
                push_collapsible_output_block(
                    &mut detail_blocks,
                    tool_call.output_summary.as_deref().unwrap_or_default(),
                    3,
                    expanded,
                    if tool_call.status == ToolCallDisplayStatus::Failed {
                        TranscriptToolCallDetailTone::Error
                    } else {
                        TranscriptToolCallDetailTone::Secondary
                    },
                );
                (title, Some("⚙"), TranscriptToolCallVisualStyle::Block)
            } else {
                (title, Some("⚙"), TranscriptToolCallVisualStyle::Inline)
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
        && tool_prefers_generic_output_block(display_tool_id)
        && tool_call.output_summary.is_some()
    {
        push_collapsible_output_block(
            &mut detail_blocks,
            tool_call.output_summary.as_deref().unwrap_or_default(),
            3,
            expanded,
            if tool_call.status == ToolCallDisplayStatus::Failed {
                TranscriptToolCallDetailTone::Error
            } else {
                TranscriptToolCallDetailTone::Secondary
            },
        );
    }

    push_tool_identity_block(&mut detail_blocks, tool_call);
    push_tool_attachment_blocks(&mut detail_blocks, tool_call, expanded);

    if detail_blocks.is_empty() {
        if let Some(output) = tool_error_text(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                text: output,
                tone: TranscriptToolCallDetailTone::Error,
            });
        } else if display_tool_id == "agent.spawn" {
            if let Some(output) = tool_call.output_summary.as_deref() {
                detail_blocks.push(TranscriptToolCallDetailBlock::Message {
                    text: format!("└ {}", format_detail_payload(output)),
                    tone: TranscriptToolCallDetailTone::Secondary,
                });
            }
        }
    }

    let disclosure_state = tool_disclosure_state(tool_call, tool_output_expanded);

    TranscriptToolCallSection {
        header: TranscriptToolCallHeader {
            tool_id: tool_call.tool_id.clone(),
            title: decorate_tool_call_title(title, tool_call, task_row, timestamps_visible),
            icon,
            status: tool_call.status,
            visual_style,
            struck_out,
            disclosure_state,
        },
        detail_blocks,
    }
}

fn generic_tool_title(tool_call: &crate::app::ToolCallEntry, tool_id: &str) -> String {
    let metadata = compact_tool_input_metadata(&tool_call.args_summary, &[], 3);
    if metadata.is_empty() {
        generic_tool_name(tool_id)
    } else {
        format!("{} · {}", generic_tool_name(tool_id), metadata.join(" · "))
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

fn lsp_tool_title(tool_call: &crate::app::ToolCallEntry) -> String {
    let operation = tool_summary_string(&tool_call.args_summary, &["operation", "op"])
        .or_else(|| tool_summary_string(&tool_call.args_summary, &["tool_name"]))
        .unwrap_or_else(|| "request".to_string());
    format!("LSP {}", collapse_inline_whitespace(&operation))
}

fn build_agent_spawn_tool_row(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
) -> (String, Option<&'static str>, TranscriptToolCallVisualStyle) {
    let title = agent_spawn_title(tool_call);
    if let Some(line) = agent_spawn_context_line(tool_call, task_row) {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: line,
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
    if let Some(summary) = agent_spawn_result_summary(tool_call, task_row) {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: summary,
            tone: if tool_call.status == ToolCallDisplayStatus::Failed {
                TranscriptToolCallDetailTone::Error
            } else {
                TranscriptToolCallDetailTone::Secondary
            },
        });
    }
    (title, Some("↗"), TranscriptToolCallVisualStyle::Block)
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

    match (profile, description) {
        (Some(profile), Some(description)) => format!("Spawn {profile} · {description}"),
        (Some(profile), None) => format!("Spawn {profile}"),
        (None, Some(description)) => format!("Spawn task · {description}"),
        (None, None) => "Spawn task".to_string(),
    }
}

fn agent_spawn_context_line(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
) -> Option<String> {
    let mode = tool_json_string(tool_call.output_json.as_ref(), &["mode"]);
    let child_session = task_row
        .and_then(|row| row.effective_child_session_id().map(str::to_string))
        .or_else(|| {
            tool_call
                .lineage
                .as_ref()
                .and_then(|lineage| lineage.child_session_id.as_deref())
                .map(str::to_string)
        })
        .or_else(|| {
            tool_json_string(
                tool_call.output_json.as_ref(),
                &["child_session_id", "session_id"],
            )
        });
    let child_request = task_row
        .and_then(|row| row.effective_child_request_id().map(str::to_string))
        .or_else(|| {
            tool_call
                .lineage
                .as_ref()
                .and_then(|lineage| lineage.child_request_id.as_deref())
                .map(str::to_string)
        })
        .or_else(|| {
            tool_json_string(
                tool_call.output_json.as_ref(),
                &["child_request_id", "request_id"],
            )
        });
    let status = task_row
        .map(|row| orchestration_task_state_label(row.state).to_string())
        .or_else(|| tool_json_string(tool_call.output_json.as_ref(), &["status"]));
    let child_tool_call_count = task_row
        .map(|row| row.child_tool_call_count as u64)
        .filter(|count| *count > 0)
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
    let mut parts = Vec::new();
    if let Some(mode) = mode {
        parts.push(mode);
    }
    if let Some(child_session) = child_session {
        parts.push(child_session);
    }
    if let Some(child_request) = child_request {
        parts.push(child_request);
    }
    if let Some(status) = status {
        parts.push(status);
    }
    if let Some(count) = child_tool_call_count {
        parts.push(format!(
            "{count} child tool call{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    if resumed_existing_session {
        parts.push("resumed session".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn agent_spawn_result_summary(
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
) -> Option<String> {
    task_row
        .and_then(|row| row.result_summary.as_deref())
        .map(collapse_inline_whitespace)
        .filter(|value| !value.is_empty())
        .or_else(|| tool_json_string(tool_call.output_json.as_ref(), &["result_summary"]))
        .or_else(|| {
            tool_call.output_summary.as_deref().and_then(|output| {
                let trimmed = output.trim();
                (!trimmed.is_empty()).then(|| format_detail_payload(trimmed))
            })
        })
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
            .map(collapse_inline_whitespace)
            .filter(|value| !value.is_empty())
    })
}

fn push_tool_identity_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    let Some(alias_source) = tool_call.alias_source_tool_id.as_deref() else {
        return;
    };
    let canonical = tool_call.canonical_tool_id();
    if alias_source == canonical {
        return;
    }
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: format!("Compat alias · {alias_source} → {canonical}"),
        tone: TranscriptToolCallDetailTone::Secondary,
    });
}

fn push_tool_attachment_blocks(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
    expanded: bool,
) {
    if tool_call.artifact_refs.is_empty() {
        return;
    }

    let visible = if expanded {
        tool_call.artifact_refs.len()
    } else {
        2
    };
    let lines = tool_call
        .artifact_refs
        .iter()
        .take(visible)
        .map(|artifact| {
            let digest = artifact
                .digest
                .as_deref()
                .map(|digest| format!(" · {digest}"))
                .unwrap_or_default();
            format!("Attachment · {}{digest}", artifact.path)
        })
        .collect::<Vec<_>>();

    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: lines.join("\n"),
        tone: TranscriptToolCallDetailTone::Secondary,
    });

    if !expanded && tool_call.artifact_refs.len() > visible {
        let hidden = tool_call.artifact_refs.len().saturating_sub(visible);
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: format!(
                "↳ {hidden} more attachment{} hidden",
                if hidden == 1 { "" } else { "s" }
            ),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    }
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
    let output = tool_call.output_summary.as_deref().unwrap_or_default();
    let output_line_count = output.lines().count();
    !tool_call.artifact_refs.is_empty()
        || match tool_call.canonical_tool_id() {
            "shell.run" => output_line_count > 10,
            "edit.hashline_apply" => tool_call
                .edit
                .as_ref()
                .and_then(|edit| edit.diff_rel_path.as_ref())
                .is_some(),
            "agent.spawn" => true,
            _ => !output.trim().is_empty() && output_line_count > 3,
        }
}

fn tool_prefers_generic_output_block(display_tool_id: &str) -> bool {
    matches!(
        display_tool_id,
        "web.fetch" | "search.web" | "search.code" | "tool.batch" | "code.lsp"
    ) || display_tool_id.starts_with("mcp.")
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

fn shell_block_title(args_summary: &str) -> String {
    let description = tool_summary_string(args_summary, &["description"]);
    let cwd = tool_summary_string(args_summary, &["cwd", "workdir"]);
    let command = tool_summary_string(args_summary, &["cmd", "command"]);
    let base = description
        .or(command)
        .unwrap_or_else(|| "Shell".to_string());

    match cwd {
        Some(cwd) if !cwd.is_empty() && !base.contains(&cwd) => format!("# {base} in {cwd}"),
        _ => format!("# {base}"),
    }
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

fn compact_tool_input_metadata(
    args_summary: &str,
    omit_keys: &[&str],
    max_parts: usize,
) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(args_summary) else {
        return Vec::new();
    };
    let Some(object) = value.as_object() else {
        return Vec::new();
    };

    let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();

    let mut parts = Vec::new();
    for key in keys.into_iter().filter(|key| !omit_keys.contains(key)) {
        if let Some(value) = object.get(key) {
            collect_input_parts(&mut parts, key.to_string(), value, max_parts);
        }
        if parts.len() >= max_parts {
            break;
        }
    }
    parts
}

fn collect_input_parts(
    parts: &mut Vec<String>,
    path: String,
    value: &serde_json::Value,
    max_parts: usize,
) {
    if parts.len() >= max_parts {
        return;
    }

    match value {
        serde_json::Value::String(text) => {
            let rendered = collapse_inline_whitespace(text);
            if !rendered.is_empty() {
                parts.push(format!("{path}={rendered}"));
            }
        }
        serde_json::Value::Number(number) => parts.push(format!("{path}={number}")),
        serde_json::Value::Bool(flag) => parts.push(format!("{path}={flag}")),
        serde_json::Value::Null => parts.push(format!("{path}=null")),
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_input_parts(parts, format!("{path}[{index}]"), item, max_parts);
                if parts.len() >= max_parts {
                    break;
                }
            }
        }
        serde_json::Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for key in keys {
                if let Some(item) = map.get(key) {
                    collect_input_parts(parts, format!("{path}.{key}"), item, max_parts);
                    if parts.len() >= max_parts {
                        break;
                    }
                }
            }
        }
    }
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
                    .filter(|line| !line.trim().is_empty())
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
    let output_lines = formatted.lines().collect::<Vec<_>>();
    if output_lines.len() > max_lines && !expanded {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: output_lines[..max_lines].join("\n"),
            tone,
        });
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: format!(
                "↳ {} more line{} hidden",
                output_lines.len() - max_lines,
                if output_lines.len() - max_lines == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            tone: TranscriptToolCallDetailTone::Secondary,
        });
    } else {
        detail_blocks.push(TranscriptToolCallDetailBlock::Message {
            text: formatted,
            tone,
        });
    }
}

fn decorate_tool_call_title(
    title: String,
    tool_call: &crate::app::ToolCallEntry,
    task_row: Option<&crate::app::OrchestrationTaskRow>,
    timestamps_visible: bool,
) -> String {
    let mut suffixes = Vec::new();
    if timestamps_visible {
        if let Some(timestamp) = task_row
            .and_then(crate::app::OrchestrationTaskRow::transcript_timestamp)
            .or_else(|| tool_call.transcript_timestamp())
        {
            suffixes.push(format_transcript_timestamp(timestamp));
        }
    }
    if let Some(task_row) = task_row {
        match task_row.state {
            crate::app::OrchestrationTaskState::Queued => suffixes.push("queued".to_string()),
            crate::app::OrchestrationTaskState::Running => suffixes.push("running".to_string()),
            crate::app::OrchestrationTaskState::Stale => suffixes.push("stale".to_string()),
            crate::app::OrchestrationTaskState::Cancelled => suffixes.push("cancelled".to_string()),
            crate::app::OrchestrationTaskState::LateResult => {
                suffixes.push("late result".to_string())
            }
            crate::app::OrchestrationTaskState::Completed => {
                if let Some(duration_ms) = task_row.duration_ms() {
                    suffixes.push(format_duration_ms(duration_ms));
                }
            }
        }
    } else {
        match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => {
                suffixes.push("pending permission".to_string())
            }
            ToolCallDisplayStatus::Queued => suffixes.push("queued".to_string()),
            ToolCallDisplayStatus::Running => suffixes.push("running".to_string()),
            ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed => {
                if let Some(duration_ms) = tool_call.duration_ms() {
                    suffixes.push(format_duration_ms(duration_ms));
                }
            }
        }
    }

    if suffixes.is_empty() {
        title
    } else {
        format!("{title} · {}", suffixes.join(" · "))
    }
}

fn tool_call_denied(tool_call: &crate::app::ToolCallEntry) -> bool {
    tool_call.permissions.iter().any(|permission| {
        permission.resolved_decision == Some(harness_core::event::PermissionDecision::Deny)
    })
}

fn tool_error_text(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    if tool_call.status != ToolCallDisplayStatus::Failed {
        return None;
    }

    tool_call
        .permissions
        .iter()
        .find_map(|permission| {
            (permission.resolved_decision == Some(harness_core::event::PermissionDecision::Deny))
                .then(|| permission.resolution_reason.clone())
                .flatten()
        })
        .or_else(|| {
            tool_call
                .output_summary
                .as_deref()
                .map(format_detail_payload)
        })
}

fn collapse_inline_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
        for (surface_index, surface) in surfaces.into_iter().enumerate() {
            let top_offset = content_height + usize::from(surface_index > 0);
            let render_width = transcript_surface_render_width(width, surface.show_outer_rail);
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
            });
        }
        let leading_gap_height = 0;
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
                surface.height,
                surface.rail_color,
                surface.surface,
            ))
            .style(Style::default().bg(surface.surface))
            .scroll((u16::try_from(local_scroll).unwrap_or(u16::MAX), 0)),
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
    let paragraph = Paragraph::new(Text::from(surface.lines.clone()))
        .style(panel_style(surface.surface, theme.text.primary))
        .scroll((u16::try_from(local_scroll).unwrap_or(u16::MAX), 0));
    if surface.show_outer_rail {
        frame.render_widget(paragraph, content_rect);
    } else {
        frame.render_widget(paragraph.wrap(Wrap { trim: false }), content_rect);
    }
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
    _turn: &TranscriptTurnSection,
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> TranscriptRenderSurface {
    let surface = transcript_emphasized_surface(theme, base_surface);
    let content_width = transcript_surface_content_width(width, true);
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
    if let Some(timestamp) = user_msg.timestamp.as_deref() {
        lines.push(build_user_surface_metadata_line(
            timestamp,
            theme,
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
        show_outer_rail: true,
        rail_color: theme.text.accent,
        surface,
        lines,
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
    let mut answer_lines = Vec::new();
    let mut context_lines = Vec::new();
    let (assistant_icon, assistant_color, assistant_status) = match turn.header.status {
        ActivityStatus::Streaming => (
            theme.live_shell.glyphs.streaming,
            theme.status.info,
            "active",
        ),
        ActivityStatus::Done => (theme.live_shell.glyphs.done, theme.status.success, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };

    for block in &turn.body_blocks {
        match block {
            TranscriptBodyBlock::RichText(text) => {
                append_rich_text_block(
                    &mut answer_lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                    theme,
                    transcript_surface_content_width(width, false),
                );
            }
            TranscriptBodyBlock::WaitingResponse => {
                append_text_block(
                    &mut answer_lines,
                    "Waiting for response…",
                    theme.text.secondary,
                    TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                );
            }
        }
    }

    if !answer_lines.is_empty() {
        answer_lines.push(build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
        ));
    }

    if let Some(thinking) = &turn.thinking {
        append_labeled_text_block(
            &mut context_lines,
            &thinking.text,
            LabeledTextBlockStyle {
                indent: TRANSCRIPT_NESTED_INDENT,
                label: "Thinking:",
                rail_color: transcript_nested_rail_color(theme),
                surface: transcript_nested_surface(theme, base_surface),
                label_style: Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::ITALIC),
                body_style: subdued_payload_style(theme),
            },
            theme,
            width,
        );
    }

    for tool_call in &turn.tool_calls {
        append_tool_call_section_lines(&mut context_lines, tool_call, theme, width, base_surface);
    }

    if let Some(error) = &turn.error {
        append_labeled_text_block(
            &mut context_lines,
            &error.text,
            LabeledTextBlockStyle {
                indent: TRANSCRIPT_NESTED_INDENT,
                label: "error",
                rail_color: transcript_nested_rail_color(theme),
                surface: transcript_nested_surface(theme, base_surface),
                label_style: Style::default().fg(theme.status.error),
                body_style: Style::default().fg(theme.status.error),
            },
            theme,
            width,
        );
    }

    let mut surfaces = vec![TranscriptRenderSurface {
        show_outer_rail: false,
        rail_color: assistant_primary_rail_color(turn, theme),
        surface: transcript_flat_surface(base_surface),
        lines: answer_lines,
    }];
    if !context_lines.is_empty() {
        surfaces.push(TranscriptRenderSurface {
            show_outer_rail: false,
            rail_color: transcript_nested_rail_color(theme),
            surface: transcript_nested_surface(theme, base_surface),
            lines: context_lines,
        });
    }
    surfaces
}

fn append_tool_call_section_lines(
    lines: &mut Vec<Line<'static>>,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    match tool_call.header.visual_style {
        TranscriptToolCallVisualStyle::Inline => {
            append_opencode_inline_tool_section_lines(lines, tool_call, theme, width, base_surface)
        }
        TranscriptToolCallVisualStyle::Block => {
            append_opencode_block_tool_section_lines(lines, tool_call, theme, width, base_surface)
        }
    }
}

fn append_opencode_inline_tool_section_lines(
    lines: &mut Vec<Line<'static>>,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let fg = inline_tool_color(tool_call, theme);
    let style = tool_call_header_style(tool_call, fg);

    let mut spans = Vec::new();
    if let Some(disclosure_state) = tool_call.header.disclosure_state {
        spans.push(Span::styled(
            format!("{} ", disclosure_glyph(disclosure_state)),
            style,
        ));
    }
    if let Some(icon) = tool_call.header.icon {
        spans.push(Span::styled(format!("{icon} "), style));
    }
    spans.push(Span::styled(tool_call.header.title.clone(), style));

    append_surface_row(
        lines,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        transcript_flat_surface(base_surface),
        spans,
        transcript_surface_content_width(width, false),
    );

    append_tool_call_detail_blocks(lines, tool_call, theme, width, base_surface, false);
}

fn append_opencode_block_tool_section_lines(
    lines: &mut Vec<Line<'static>>,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let active = tool_call_is_active(tool_call.header.status);
    let surface = if active {
        transcript_emphasized_surface(theme, base_surface)
    } else {
        transcript_flat_surface(base_surface)
    };
    let rail_color = transcript_nested_rail_color(theme);
    let title_style = tool_call_header_style(tool_call, block_tool_color(tool_call, theme));

    let mut title_spans = Vec::new();
    if let Some(disclosure_state) = tool_call.header.disclosure_state {
        title_spans.push(Span::styled(
            format!("{} ", disclosure_glyph(disclosure_state)),
            title_style,
        ));
    }
    title_spans.push(Span::styled(tool_call.header.title.clone(), title_style));

    append_nested_surface_row(
        lines,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        rail_color,
        surface,
        title_spans,
        transcript_surface_content_width(width, false),
    );

    append_tool_call_detail_blocks(lines, tool_call, theme, width, base_surface, active);
}

fn append_tool_call_detail_blocks(
    lines: &mut Vec<Line<'static>>,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    block_surface: bool,
) {
    for detail_block in &tool_call.detail_blocks {
        match detail_block {
            TranscriptToolCallDetailBlock::Message { text, tone } => {
                append_tool_call_message_block(
                    lines,
                    text,
                    *tone,
                    theme,
                    width,
                    base_surface,
                    block_surface,
                );
            }
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                fallback_path,
                force_stacked,
            } => append_tool_call_diff_block(
                lines,
                diff_content,
                fallback_path.as_deref(),
                *force_stacked,
                theme,
                width,
                base_surface,
            ),
        }
    }
}

fn append_tool_call_message_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    tone: TranscriptToolCallDetailTone,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    block_surface: bool,
) {
    let style = match tone {
        TranscriptToolCallDetailTone::Secondary => subdued_payload_style(theme),
        TranscriptToolCallDetailTone::Error => Style::default().fg(theme.status.error),
    };

    let indent = TRANSCRIPT_OPCODE_EDIT_INDENT;

    let surface = if block_surface {
        transcript_emphasized_surface(theme, base_surface)
    } else {
        transcript_flat_surface(base_surface)
    };

    for row in text.split('\n') {
        let spans = if row.is_empty() {
            Vec::new()
        } else {
            vec![Span::styled(row.to_string(), style)]
        };
        append_surface_row(
            lines,
            indent,
            surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }
}

fn inline_tool_color(tool_call: &TranscriptToolCallSection, theme: &Theme) -> Color {
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission => theme.status.warning,
        ToolCallDisplayStatus::Queued => theme.text.secondary,
        ToolCallDisplayStatus::Running => theme.status.info,
        ToolCallDisplayStatus::Succeeded => theme.text.tertiary,
        ToolCallDisplayStatus::Failed => theme.status.error,
    }
}

fn block_tool_color(tool_call: &TranscriptToolCallSection, theme: &Theme) -> Color {
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission => theme.status.warning,
        ToolCallDisplayStatus::Running => theme.status.info,
        ToolCallDisplayStatus::Failed => theme.status.error,
        ToolCallDisplayStatus::Queued => theme.text.secondary,
        ToolCallDisplayStatus::Succeeded => theme.text.tertiary,
    }
}

fn tool_call_header_style(tool_call: &TranscriptToolCallSection, color: Color) -> Style {
    let mut style = Style::default().fg(color);
    match tool_call.header.status {
        ToolCallDisplayStatus::PendingPermission
        | ToolCallDisplayStatus::Queued
        | ToolCallDisplayStatus::Running
        | ToolCallDisplayStatus::Failed => {
            style = style.add_modifier(Modifier::BOLD);
        }
        ToolCallDisplayStatus::Succeeded => {
            style = style.add_modifier(Modifier::DIM);
        }
    }
    if tool_call.header.struck_out {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

fn tool_call_is_active(status: ToolCallDisplayStatus) -> bool {
    matches!(
        status,
        ToolCallDisplayStatus::PendingPermission
            | ToolCallDisplayStatus::Queued
            | ToolCallDisplayStatus::Running
    )
}

fn disclosure_glyph(state: TranscriptToolCallDisclosureState) -> &'static str {
    match state {
        TranscriptToolCallDisclosureState::Collapsed => "▸",
        TranscriptToolCallDisclosureState::Expanded => "▾",
    }
}

fn append_tool_call_diff_block(
    lines: &mut Vec<Line<'static>>,
    diff_content: &str,
    fallback_path: Option<&str>,
    force_stacked: bool,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let nested_width = transcript_surface_content_width(width, false);
    let content_width = transcript_surface_content_width(nested_width, false);
    if let Some(diff_lines) = render_structured_diff_lines(
        diff_content,
        fallback_path,
        "",
        content_width,
        force_stacked,
        theme,
    ) {
        append_prebuilt_surface_lines(
            lines,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            transcript_nested_surface(theme, base_surface),
            diff_lines,
            nested_width,
        );
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
    indent.chars().count()
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
        visible_width += span.content.chars().count();
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
        show_outer_rail: true,
        rail_color: theme.status.warning,
        surface: transcript_emphasized_surface(theme, base_surface),
        lines,
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

pub(super) fn append_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: ratatui::style::Color,
    prefix: &str,
) {
    for line in text.lines() {
        let body = if line.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}{line}")
        };
        lines.push(Line::from(Span::styled(body, Style::default().fg(color))));
    }

    if text.is_empty() {
        lines.push(Line::from(Span::styled(
            prefix.to_string(),
            Style::default().fg(color),
        )));
    }
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
    let prefix_width = TRANSCRIPT_USER_BODY_PREFIX.chars().count();
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
        .saturating_sub(prefix.chars().count())
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

    let prefix_width = prefix.chars().count();
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
                    base_style
                        .fg(theme.status.success)
                        .bg(theme.surface.panel_elevated),
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

                if let Some(highlighted) = render_highlighted_code_block(
                    language.as_deref(),
                    &body,
                    &raw,
                    prefix,
                    color,
                    theme,
                ) {
                    lines.extend(highlighted);
                } else {
                    append_text_block(lines, &raw, color, prefix);
                }
            }
        }
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
    raw: &str,
    prefix: &str,
    color: ratatui::style::Color,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let language = language?;
    let syntax_assets = syntax_highlight_assets();
    let syntax = syntax_assets.syntax_set.find_syntax_by_token(language)?;
    let mut highlighter = HighlightLines::new(syntax, &syntax_assets.theme);
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("{prefix}```{language}"),
        muted_meta_style(theme),
    )));

    for source_line in body.lines() {
        let regions = highlighter
            .highlight_line(source_line, &syntax_assets.syntax_set)
            .ok()?;
        let mut spans = vec![Span::styled(
            prefix.to_string(),
            transcript_prefix_style(theme),
        )];
        if regions.is_empty() {
            spans.push(Span::styled(
                source_line.to_string(),
                Style::default().fg(color),
            ));
        } else {
            spans.extend(regions.into_iter().map(|(style, content)| {
                Span::styled(content.to_string(), syntect_style_to_ratatui(style))
            }));
        }
        lines.push(Line::from(spans));
    }

    if body.is_empty() {
        lines.push(Line::from(Span::styled(
            prefix.to_string(),
            Style::default().fg(color),
        )));
    }

    lines.push(Line::from(Span::styled(
        format!("{prefix}```"),
        muted_meta_style(theme),
    )));

    if lines.is_empty() {
        append_text_block(&mut lines, raw, color, prefix);
    }

    Some(lines)
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
    let mut rendered = Style::default()
        .fg(syntect_color_to_ratatui(style.foreground))
        .bg(syntect_color_to_ratatui(style.background));

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
    hit_areas
        .transcript
        .filter(|area| rect_contains(*area, column, row))
        .map(|_| WheelTarget::Transcript)
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
) -> u16 {
    let viewport_height = usize::from(inner_area.height);
    if viewport_height == 0 {
        return 0;
    }

    let max_scroll = layout.total_height.saturating_sub(viewport_height);
    if max_scroll == 0 {
        return 0;
    }

    if app.follow_mode {
        return u16::try_from(max_scroll).unwrap_or(u16::MAX);
    }

    let scroll_back = usize::from(app.transcript_scroll);
    let scroll = max_scroll.saturating_sub(scroll_back);
    u16::try_from(scroll).unwrap_or(u16::MAX)
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

fn transcript_surface_render_width(width: u16, _show_outer_rail: bool) -> u16 {
    width.max(1)
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
    if !turn.header.model_id.trim().is_empty() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(
            turn.header.model_id.clone(),
            muted_meta_style(theme),
        ));
    }
    if turn.header.status != ActivityStatus::Streaming {
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
    Line::from(spans)
}

fn titlecase_label(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

fn assistant_footer_label(value: &str) -> String {
    if value.trim().is_empty()
        || value.eq_ignore_ascii_case("unknown")
        || value.eq_ignore_ascii_case("default")
    {
        return "Assistant".to_string();
    }
    titlecase_label(value)
}

fn assistant_primary_label_color(turn: &TranscriptTurnSection, theme: &Theme) -> Color {
    match turn.header.status {
        ActivityStatus::Streaming => theme.text.accent,
        ActivityStatus::Done => theme.status.success,
        ActivityStatus::Error => theme.status.error,
    }
}

fn assistant_primary_rail_color(turn: &TranscriptTurnSection, theme: &Theme) -> Color {
    match turn.header.status {
        ActivityStatus::Streaming => theme.text.accent,
        ActivityStatus::Done | ActivityStatus::Error => theme.text.secondary,
    }
}

fn transcript_nested_rail_color(theme: &Theme) -> Color {
    theme.text.secondary
}

fn transcript_nested_surface(_theme: &Theme, base_surface: Color) -> Color {
    transcript_flat_surface(base_surface)
}

fn build_user_surface_metadata_line(
    metadata: &str,
    theme: &Theme,
    width: u16,
    surface: Color,
) -> Line<'static> {
    user_surface_line(
        vec![Span::styled(metadata.to_string(), muted_meta_style(theme))],
        Style::default().fg(theme.text.primary),
        width,
        surface,
    )
}

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 60_000 {
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

fn format_transcript_timestamp(timestamp: &str) -> String {
    let trimmed = timestamp.trim();
    if trimmed.len() >= 16 && trimmed.as_bytes().get(10) == Some(&b'T') {
        trimmed[11..16].to_string()
    } else {
        trimmed.to_string()
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
    Span::styled(text.into(), style.bg(surface))
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
    indent.chars().count() + TRANSCRIPT_RAIL_GLYPH.chars().count() + 1
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
        visible_width += span.content.chars().count();
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
        let token_width = token_text.chars().count();
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
            let chunk = take_char_prefix(remainder, width);
            current_width = chunk.chars().count();
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

fn take_char_prefix(text: &str, width: usize) -> &str {
    if width == 0 {
        return "";
    }

    let mut split_at = text.len();
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == width {
            split_at = idx;
            break;
        }
    }
    &text[..split_at]
}

fn append_labeled_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    block_style: LabeledTextBlockStyle<'_>,
    theme: &Theme,
    width: u16,
) {
    let continuation_prefix = format!("{}  ", block_style.indent);
    if text.contains("```") {
        append_nested_surface_row(
            lines,
            block_style.indent,
            block_style.rail_color,
            block_style.surface,
            vec![
                Span::styled(block_style.label.to_string(), block_style.label_style),
                Span::styled(" · code", muted_meta_style(theme)),
            ],
            transcript_surface_content_width(width, false),
        );
        append_rich_text_block(
            lines,
            text,
            block_style.body_style.fg.unwrap_or(theme.text.primary),
            &continuation_prefix,
            theme,
            transcript_surface_content_width(width, false),
        );
        return;
    }

    let mut rows = text.split('\n');
    if let Some(first) = rows.next() {
        let mut spans = vec![Span::styled(
            block_style.label.to_string(),
            block_style.label_style,
        )];
        if !first.is_empty() {
            spans.push(Span::styled(" · ", muted_meta_style(theme)));
            spans.push(Span::styled(first.to_string(), block_style.body_style));
        }
        append_nested_surface_row(
            lines,
            block_style.indent,
            block_style.rail_color,
            block_style.surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }

    for row in rows {
        let spans = if row.is_empty() {
            Vec::new()
        } else {
            vec![Span::styled(row.to_string(), block_style.body_style)]
        };
        append_nested_surface_row(
            lines,
            block_style.indent,
            block_style.rail_color,
            block_style.surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }

    if text.is_empty() {
        append_nested_surface_row(
            lines,
            block_style.indent,
            block_style.rail_color,
            block_style.surface,
            vec![Span::styled(
                block_style.label.to_string(),
                block_style.label_style,
            )],
            transcript_surface_content_width(width, false),
        );
    }
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
        args_summary: r#"{"cmd":"false"}"#.to_string(),
        args_digest: "digest".to_string(),
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
            header: TranscriptToolCallHeader {
                tool_id: "shell.run".to_string(),
                title: "# false · 1ms".to_string(),
                icon: None,
                status: ToolCallDisplayStatus::Failed,
                visual_style: TranscriptToolCallVisualStyle::Block,
                struck_out: false,
                disclosure_state: None,
            },
            detail_blocks: vec![
                TranscriptToolCallDetailBlock::Message {
                    text: "$ false".to_string(),
                    tone: TranscriptToolCallDetailTone::Secondary,
                },
                TranscriptToolCallDetailBlock::Message {
                    text: "command failed".to_string(),
                    tone: TranscriptToolCallDetailTone::Error,
                },
            ],
        }
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_answer_precedes_nested_context() {
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
        args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
        args_digest: "digest".to_string(),
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

    let answer_row = lines
        .iter()
        .position(|line| line.contains("assistant answer"))
        .expect("assistant answer row");
    let thinking_row = lines
        .iter()
        .enumerate()
        .skip(answer_row + 1)
        .find_map(|(index, line)| line.contains("Thinking:").then_some(index))
        .expect("thinking row");
    let tool_row = lines
        .iter()
        .enumerate()
        .skip(thinking_row + 1)
        .find_map(|(index, line)| line.contains("Read src/ui.rs").then_some(index))
        .expect("tool row");

    assert!(answer_row < thinking_row);
    assert!(thinking_row < tool_row);
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_edit_tool_matches_opencode_inline_diff_shape() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("opencode-inline.diff"),
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
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit".to_string(),
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
            diff_rel_path: Some("artifacts/opencode-inline.diff".to_string()),
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

    assert!(rendered.contains("← Patched crates/harness-tui/src/ui.rs"));
    assert!(rendered.contains("render_diff_tab"));
    assert!(rendered.contains("render_live_details_overlay"));
    assert!(rendered.lines().any(|line| {
        line.contains("render_diff_tab")
            && line.contains("render_live_details_overlay")
            && line.contains('│')
    }));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_proposed_edit_renders_opencode_header() {
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-edit-proposed", ActivityStatus::Done, "");
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-proposed".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-proposed".to_string(),
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

    assert!(rendered.contains("← Edit crates/harness-tui/src/ui.rs"));
    assert!(!rendered.contains("tool edit.hashline_apply"));
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
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-rejected".to_string(),
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

    assert!(rendered.contains("← Edit crates/harness-tui/src/ui.rs"));
    assert!(rendered.contains("ANCHOR_MISMATCH at line 45"));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_follow_mode_uses_measured_surface_heights() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity(
            "request-long",
            ActivityStatus::Done,
            "this reply is intentionally long enough to wrap across several measured surface rows",
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

    assert_eq!(usize::from(scroll), expected);
    assert!(
        usize::from(scroll) > 1,
        "follow mode should scroll by measured surface height, not just section count"
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

    assert_eq!(layout.sections.len(), 2);
    assert_eq!(layout.sections[0].kind, MeasuredTranscriptSectionKind::Turn);
    assert_eq!(
        layout.sections[1].kind,
        MeasuredTranscriptSectionKind::PendingPermission
    );
    assert!(
        layout.sections[1].top_row
            >= layout.sections[0].top_row + layout.sections[0].total_height(),
        "pending permission should stay after the last activity in measured layout"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_native_tool_transcript_rows_show_disclosure_timestamps_and_task_metadata()
{
    let theme = Theme::default();

    let mut native_read = transcript_section_model_test_tool_call("tc-native-read", "fs.read");
    native_read.args_summary = r#"{"path":"src/ui.rs","offset":12,"limit":24}"#.to_string();
    native_read.status = ToolCallDisplayStatus::Succeeded;
    native_read.output_summary = Some("24 lines read from src/ui.rs".to_string());
    native_read.truncated_output = native_read.output_summary.clone();
    native_read.last_mono_ms = 1_250;
    native_read.last_timestamp = Some("2026-03-22T14:35:44Z".to_string());

    let mut alias_read = transcript_section_model_test_tool_call("tc-alias-read", "read");
    alias_read.canonical_tool_id = Some("fs.read".to_string());
    alias_read.alias_source_tool_id = Some("read".to_string());
    alias_read.args_summary = native_read.args_summary.clone();
    alias_read.status = ToolCallDisplayStatus::Succeeded;
    alias_read.output_summary = native_read.output_summary.clone();
    alias_read.truncated_output = native_read.truncated_output.clone();
    alias_read.last_mono_ms = native_read.last_mono_ms;
    alias_read.last_timestamp = native_read.last_timestamp.clone();

    let native_read_section =
        build_opencode_tool_call_section(&native_read, None, true, false, false, false, None);
    let alias_read_section =
        build_opencode_tool_call_section(&alias_read, None, true, false, false, false, None);
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
    assert_eq!(alias_read_section.header.disclosure_state, None);
    assert!(alias_read_section
        .detail_blocks
        .iter()
        .any(|block| matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, .. }
                if text.contains("Compat alias · read → fs.read")
        )));

    let mut task_call = transcript_section_model_test_tool_call("tc-task", "task");
    task_call.canonical_tool_id = Some("agent.spawn".to_string());
    task_call.alias_source_tool_id = Some("task".to_string());
    task_call.args_summary =
        r#"{"description":"audit transcript parity","subagent_type":"researcher"}"#.to_string();
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

    let task_section =
        build_opencode_tool_call_section(&task_call, None, true, false, false, false, None);
    assert_eq!(
        task_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    let mut task_lines = Vec::new();
    append_tool_call_section_lines(
        &mut task_lines,
        &task_section,
        &theme,
        120,
        theme.surface.panel,
    );
    let task_text = transcript_test_line_texts(task_lines).join("\n");
    assert!(task_text.contains("▸ Spawn researcher · audit transcript parity · 14:36 · 1.6s"));
    assert!(task_text
        .contains("foreground · agent_worker · req_child · completed · 3 child tool calls"));
    assert!(task_text.contains("Found the inline transcript path."));
    assert!(task_text.contains("Compat alias · task → agent.spawn"));
    assert!(!task_text.contains("Task audit transcript parity"));

    let mut fetch_call = transcript_section_model_test_tool_call("tc-fetch", "webfetch");
    fetch_call.canonical_tool_id = Some("web.fetch".to_string());
    fetch_call.alias_source_tool_id = Some("webfetch".to_string());
    fetch_call.args_summary =
        r#"{"url":"https://example.test/report.pdf","format":"markdown"}"#.to_string();
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

    let fetch_section =
        build_opencode_tool_call_section(&fetch_call, None, true, false, false, false, None);
    assert_eq!(
        fetch_section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    let mut fetch_lines = Vec::new();
    append_tool_call_section_lines(
        &mut fetch_lines,
        &fetch_section,
        &theme,
        120,
        theme.surface.panel,
    );
    let fetch_text = transcript_test_line_texts(fetch_lines).join("\n");
    assert!(fetch_text.contains("▸ Fetch https://example.test/report.pdf · 14:37 · 2.4s"));
    assert!(fetch_text.contains("report ready"));
    assert!(!fetch_text.contains("stored inline artifact"));
    assert!(fetch_text.contains("↳ 1 more line hidden"));
    assert!(fetch_text.contains(
        "Attachment · artifacts/toolcalls/tc-fetch/web.fetch.pdf · digest-fetch-artifact"
    ));
    assert!(fetch_text.contains("Compat alias · webfetch → web.fetch"));

    let mut generic_call = transcript_section_model_test_tool_call("tc-generic", "vendor.magic");
    generic_call.args_summary =
        r#"{"path":"notes.md","query":"child parity","limit":3}"#.to_string();
    generic_call.status = ToolCallDisplayStatus::Succeeded;
    generic_call.timing_elapsed_ms = Some(800);
    let generic_section =
        build_opencode_tool_call_section(&generic_call, None, true, false, false, false, None);
    assert_eq!(
        generic_section.header.title,
        "vendor.magic · limit=3 · path=notes.md · query=child parity · 800ms"
    );
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
                    canonical_tool_id: Some("agent.spawn".to_string()),
                    alias_source_tool_id: Some("task".to_string()),
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
    let running_section = build_opencode_tool_call_section(
        running_tool,
        running_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let mut running_lines = Vec::new();
    append_tool_call_section_lines(
        &mut running_lines,
        &running_section,
        &Theme::default(),
        120,
        Theme::default().surface.panel,
    );
    let running_text = transcript_test_line_texts(running_lines).join("\n");
    assert!(running_text.contains("Spawn researcher · audit transcript parity · 14:36 · running"));
    assert!(running_text.contains("agent_worker · req_child · running · 1 child tool call"));

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
        9,
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
    let completed_section = build_opencode_tool_call_section(
        completed_tool,
        completed_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let mut completed_lines = Vec::new();
    append_tool_call_section_lines(
        &mut completed_lines,
        &completed_section,
        &Theme::default(),
        120,
        Theme::default().surface.panel,
    );
    let completed_text = transcript_test_line_texts(completed_lines).join("\n");
    assert!(completed_text.contains("Spawn researcher · audit transcript parity · 14:36 · 1.6s"));
    assert!(completed_text.contains("agent_worker · req_child · completed · 2 child tool calls"));
    assert!(completed_text.contains("Found the inline transcript path."));
    assert!(!completed_text.contains("child session finished"));
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
        args_summary: "{}".to_string(),
        args_digest: "digest".to_string(),
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
    fn transcript_answer_precedes_nested_context() {
        exact_test_transcript_answer_precedes_nested_context();
    }

    #[test]
    fn transcript_follow_mode_uses_measured_surface_heights() {
        exact_test_transcript_follow_mode_uses_measured_surface_heights();
    }

    #[test]
    fn transcript_pending_permission_stays_after_last_activity() {
        exact_test_transcript_pending_permission_stays_after_last_activity();
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

        assert_eq!(
            lines[0],
            "┃ ◷ permission checkpoint (fail-closed · review required)"
        );
        assert_eq!(lines[1], "┃ Apply hashline edit to demo.txt");
        assert!(lines
            .iter()
            .all(|line| !line.contains('╭') && !line.contains('╰') && !line.contains('│')));
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

        assert_eq!(lines[0], "   Waiting for response…");
        assert_eq!(lines[1], "   ◐ Assistant · gpt-5.4-mini · active");
    }

    #[test]
    fn user_message_surface_matches_opencode_panel_body_and_metadata() {
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
        assert!(lines.iter().any(|line| line.contains("09:45")));
        assert!(lines.iter().any(|line| line.contains("hello")));
        assert!(lines.iter().any(|line| line.contains("reply")));
    }
}
