use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Theme as SyntectTheme};
use syntect::parsing::SyntaxSet;

use super::*;

use crate::theme::DIFF_SIDE_BY_SIDE_MIN_WIDTH;

use super::ui_secondary::render_structured_diff_lines;

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
    rail_color: Color,
    surface: Color,
    lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone)]
struct MeasuredTranscriptSurface {
    top_offset: usize,
    height: usize,
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
    Turn(TranscriptTurnSection),
    PendingPermission(TranscriptPendingPermissionSection),
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
    request_id: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTurnHeader {
    status: ActivityStatus,
    is_selected: bool,
    provider_id: String,
    model_id: String,
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
    inline_summary: Option<String>,
    detail_blocks: Vec<TranscriptToolCallDetailBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptToolCallHeader {
    tool_id: String,
    status: ToolCallDisplayStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TranscriptToolCallDetailBlock {
    State(String),
    Args(String),
    Result(String),
    Error(String),
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

const TRANSCRIPT_SURFACE_RAIL_WIDTH: u16 = 2;
const TRANSCRIPT_SURFACE_BODY_PREFIX: &str = " ";
const TRANSCRIPT_NESTED_INDENT: &str = "  ";
const TRANSCRIPT_RAIL_GLYPH: &str = "▎ ";

pub(super) fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if !app.replay_mode {
        let inner_area = inset_rect(area, theme.live_shell.rhythm.transcript_gutter_x, 0);

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
    let layout = build_measured_transcript_layout_for_width(app, theme, inner_area.width);
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
    let sections = build_transcript_sections(app);
    measure_transcript_layout(&sections, theme, width)
}

fn build_transcript_sections(app: &AppState) -> Vec<TranscriptSection> {
    let mut sections = Vec::new();

    for (index, activity) in app.activities.iter().enumerate() {
        sections.push(TranscriptSection::Turn(build_turn_section(
            activity,
            index == app.selected_activity_index,
            app.transcript_thinking_visible(),
            app.successful_tool_details_visible(),
        )));
    }

    for (_permission_id, summary) in app.transcript_pending_permissions() {
        sections.push(TranscriptSection::PendingPermission(
            TranscriptPendingPermissionSection { summary },
        ));
    }

    sections
}

fn build_turn_section(
    activity: &ActivityEntry,
    is_selected: bool,
    thinking_visible: bool,
    show_successful_tool_details: bool,
) -> TranscriptTurnSection {
    let user_message =
        activity
            .user_message
            .as_ref()
            .map(|user_msg| TranscriptUserMessageSection {
                request_id: activity.request_id.clone(),
                text: user_msg.text.clone(),
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
            provider_id: activity.provider_id.clone(),
            model_id: activity.model_id.clone(),
        },
        body_blocks,
        tool_calls: activity
            .tool_calls
            .iter()
            .map(|tool_call| build_tool_call_section(tool_call, show_successful_tool_details))
            .collect(),
        thinking,
        error: activity
            .error_message
            .as_ref()
            .map(|text| TranscriptErrorSection { text: text.clone() }),
    }
}

fn build_tool_call_section(
    tool_call: &crate::app::ToolCallEntry,
    show_successful_tool_details: bool,
) -> TranscriptToolCallSection {
    let collapse_success =
        tool_call.status == ToolCallDisplayStatus::Succeeded && !show_successful_tool_details;
    let inline_summary = collapse_success.then(|| {
        tool_call
            .output_summary
            .as_deref()
            .and_then(|output| compact_inline_payload(output, 84))
            .or_else(|| tool_state_summary(tool_call).map(str::to_string))
    });

    let mut detail_blocks = Vec::new();
    if !collapse_success {
        if let Some(state_summary) = tool_state_summary(tool_call) {
            detail_blocks.push(TranscriptToolCallDetailBlock::State(
                state_summary.to_string(),
            ));
        }

        if let Some(args_summary) = compact_inline_payload(&tool_call.args_summary, 96) {
            detail_blocks.push(TranscriptToolCallDetailBlock::Args(args_summary));
        }

        if let Some(output) = tool_call
            .truncated_output
            .as_deref()
            .and_then(|output| compact_inline_payload(output, 96))
        {
            if tool_call.status == ToolCallDisplayStatus::Failed {
                detail_blocks.push(TranscriptToolCallDetailBlock::Error(output));
            } else {
                detail_blocks.push(TranscriptToolCallDetailBlock::Result(output));
            }
        }
    }

    TranscriptToolCallSection {
        header: TranscriptToolCallHeader {
            tool_id: tool_call.tool_id.clone(),
            status: tool_call.status,
        },
        inline_summary: inline_summary.flatten(),
        detail_blocks,
    }
}

fn measure_transcript_layout(
    sections: &[TranscriptSection],
    theme: &Theme,
    width: u16,
) -> MeasuredTranscriptLayout {
    let mut top_row = 0;
    let mut measured_sections = Vec::with_capacity(sections.len());

    for (index, section) in sections.iter().enumerate() {
        let surfaces = build_transcript_render_surfaces(section, theme, width);
        let lines = render_transcript_surface_lines(&surfaces);
        let content_width = usize::from(transcript_surface_content_width(width));
        let mut content_height = 0usize;
        let mut measured_surfaces = Vec::with_capacity(surfaces.len());
        for (surface_index, surface) in surfaces.into_iter().enumerate() {
            let top_offset = content_height + usize::from(surface_index > 0);
            let height = transcript_visual_rows(&surface.lines, content_width);
            content_height = top_offset + height;
            measured_surfaces.push(MeasuredTranscriptSurface {
                top_offset,
                height,
                rail_color: surface.rail_color,
                surface: surface.surface,
                lines: surface.lines,
            });
        }
        let leading_gap_height = usize::from(index > 0);
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
                area.width,
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

    let rail_width = area.width.min(TRANSCRIPT_SURFACE_RAIL_WIDTH);
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
    frame.render_widget(
        Paragraph::new(Text::from(surface.lines.clone()))
            .style(panel_style(surface.surface, theme.text.primary))
            .scroll((u16::try_from(local_scroll).unwrap_or(u16::MAX), 0))
            .wrap(Wrap { trim: false }),
        content_rect,
    );
}

fn build_transcript_render_surfaces(
    section: &TranscriptSection,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptRenderSurface> {
    match section {
        TranscriptSection::Turn(turn) => build_turn_render_surfaces(turn, theme, width),
        TranscriptSection::PendingPermission(section) => {
            vec![build_pending_permission_render_surface(
                section, theme, width,
            )]
        }
    }
}

fn build_turn_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptRenderSurface> {
    let mut surfaces = Vec::with_capacity(3);
    if let Some(user_message) = turn.user_message.as_ref() {
        surfaces.push(build_user_render_surface(turn, user_message, theme, width));
    }
    surfaces.extend(build_assistant_render_surfaces(turn, theme, width));
    surfaces
}

fn build_user_render_surface(
    turn: &TranscriptTurnSection,
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    width: u16,
) -> TranscriptRenderSurface {
    let mut lines = Vec::new();
    let is_selected = turn.header.is_selected;
    let header_style = transcript_label_style(theme, is_selected);
    let meta_style = muted_meta_style(theme);
    let mut user_line = vec![Span::raw(TRANSCRIPT_SURFACE_BODY_PREFIX.to_string())];
    user_line.extend([
        Span::styled(
            format!("{} ", theme.live_shell.transcript_glyphs.user_marker),
            Style::default().fg(theme.text.accent),
        ),
        Span::styled("user", header_style),
    ]);
    append_parenthesized_meta(
        &mut user_line,
        vec![(
            request_id_label(&user_msg.request_id).into_owned(),
            meta_style,
        )],
        theme,
    );
    lines.push(Line::from(user_line));
    append_rich_text_block(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        TRANSCRIPT_SURFACE_BODY_PREFIX,
        theme,
        transcript_surface_content_width(width),
    );

    TranscriptRenderSurface {
        rail_color: theme.text.accent,
        surface: transcript_top_level_surface(theme),
        lines,
    }
}

fn build_assistant_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
) -> Vec<TranscriptRenderSurface> {
    let mut answer_lines = Vec::new();
    let mut context_lines = Vec::new();
    let is_selected = turn.header.is_selected;
    let header_style = transcript_label_style(theme, is_selected);
    let meta_style = muted_meta_style(theme);
    let (assistant_icon, assistant_color, assistant_status) = match turn.header.status {
        ActivityStatus::Streaming => (
            theme.live_shell.glyphs.streaming,
            theme.status.info,
            "active",
        ),
        ActivityStatus::Done => (theme.live_shell.glyphs.done, theme.status.success, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };
    let mut assistant_meta = Vec::new();
    if turn.header.status != ActivityStatus::Streaming
        && (!turn.header.provider_id.is_empty() || !turn.header.model_id.is_empty())
    {
        assistant_meta.push(format!(
            "{}/{}",
            if turn.header.provider_id.is_empty() {
                "-"
            } else {
                turn.header.provider_id.as_str()
            },
            if turn.header.model_id.is_empty() {
                "-"
            } else {
                turn.header.model_id.as_str()
            }
        ));
    }
    let mut assistant_line = vec![Span::raw(TRANSCRIPT_SURFACE_BODY_PREFIX.to_string())];
    assistant_line.extend([
        Span::styled(
            format!("{} ", assistant_icon),
            Style::default().fg(assistant_color),
        ),
        Span::styled("assistant", header_style),
    ]);
    let mut assistant_segments = vec![(
        assistant_status.to_string(),
        Style::default().fg(assistant_color),
    )];
    if is_selected {
        assistant_segments.push((
            "current".to_string(),
            Style::default().fg(theme.text.accent),
        ));
    }
    if !assistant_meta.is_empty() {
        assistant_segments.push((assistant_meta.join(" · "), meta_style));
    }
    append_parenthesized_meta(&mut assistant_line, assistant_segments, theme);
    answer_lines.push(Line::from(assistant_line));

    for block in &turn.body_blocks {
        match block {
            TranscriptBodyBlock::RichText(text) => {
                append_rich_text_block(
                    &mut answer_lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_SURFACE_BODY_PREFIX,
                    theme,
                    transcript_surface_content_width(width),
                );
            }
            TranscriptBodyBlock::WaitingResponse => {
                append_text_block(
                    &mut answer_lines,
                    "Waiting for response…",
                    theme.text.secondary,
                    TRANSCRIPT_SURFACE_BODY_PREFIX,
                );
            }
        }
    }

    if let Some(thinking) = &turn.thinking {
        append_labeled_text_block(
            &mut context_lines,
            &thinking.text,
            LabeledTextBlockStyle {
                indent: TRANSCRIPT_NESTED_INDENT,
                label: "Thinking:",
                rail_color: theme.text.accent,
                surface: transcript_nested_surface(theme),
                label_style: Style::default().fg(theme.text.accent),
                body_style: subdued_payload_style(theme),
            },
            theme,
            width,
        );
    }

    for tool_call in &turn.tool_calls {
        append_tool_call_section_lines(&mut context_lines, tool_call, theme, width);
    }

    if let Some(error) = &turn.error {
        append_labeled_text_block(
            &mut context_lines,
            &error.text,
            LabeledTextBlockStyle {
                indent: TRANSCRIPT_NESTED_INDENT,
                label: "error",
                rail_color: theme.status.error,
                surface: transcript_nested_surface(theme),
                label_style: Style::default().fg(theme.status.error),
                body_style: Style::default().fg(theme.status.error),
            },
            theme,
            width,
        );
    }

    let mut surfaces = vec![TranscriptRenderSurface {
        rail_color: assistant_color,
        surface: transcript_top_level_surface(theme),
        lines: answer_lines,
    }];
    if !context_lines.is_empty() {
        surfaces.push(TranscriptRenderSurface {
            rail_color: assistant_context_rail_color(turn, theme, assistant_color),
            surface: transcript_nested_surface(theme),
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
) {
    let (status_icon, status_color, status_label, _) =
        tool_status_tokens(tool_call.header.status, theme);

    let mut header = vec![
        Span::styled(
            format!("{} ", status_icon),
            Style::default().fg(status_color),
        ),
        Span::styled("tool ", muted_meta_style(theme)),
        Span::styled(
            tool_call.header.tool_id.clone(),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    append_parenthesized_meta(
        &mut header,
        vec![(status_label.to_string(), Style::default().fg(status_color))],
        theme,
    );

    if let Some(summary) = &tool_call.inline_summary {
        header.push(Span::styled(" · ", muted_meta_style(theme)));
        header.push(Span::styled(summary.clone(), subdued_payload_style(theme)));
        append_nested_surface_row(
            lines,
            TRANSCRIPT_NESTED_INDENT,
            status_color,
            transcript_nested_surface(theme),
            header,
            transcript_surface_content_width(width),
        );
        return;
    }

    append_nested_surface_row(
        lines,
        TRANSCRIPT_NESTED_INDENT,
        status_color,
        transcript_nested_surface(theme),
        header,
        transcript_surface_content_width(width),
    );

    for detail_block in &tool_call.detail_blocks {
        let (label, value, value_style) = match detail_block {
            TranscriptToolCallDetailBlock::State(value) => {
                ("state", value.as_str(), subdued_payload_style(theme))
            }
            TranscriptToolCallDetailBlock::Args(value) => {
                ("args", value.as_str(), subdued_payload_style(theme))
            }
            TranscriptToolCallDetailBlock::Result(value) => {
                ("result", value.as_str(), subdued_payload_style(theme))
            }
            TranscriptToolCallDetailBlock::Error(value) => (
                "error",
                value.as_str(),
                Style::default().fg(theme.status.error),
            ),
        };
        append_tool_detail_row(
            lines,
            TRANSCRIPT_NESTED_INDENT,
            status_color,
            transcript_nested_surface(theme),
            label,
            value,
            tool_detail_label_style(label, theme, tool_call.header.status),
            value_style,
            theme,
            transcript_surface_content_width(width),
        );
    }
}

fn build_pending_permission_render_surface(
    section: &TranscriptPendingPermissionSection,
    theme: &Theme,
    width: u16,
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
        transcript_surface_content_width(width),
    );

    TranscriptRenderSurface {
        rail_color: theme.status.warning,
        surface: transcript_nested_surface(theme),
        lines,
    }
}

fn render_transcript_surface_lines(surfaces: &[TranscriptRenderSurface]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, surface) in surfaces.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        lines.extend(surface.lines.iter().cloned().map(|line| {
            prepend_transcript_surface_rail(line, surface.rail_color, surface.surface)
        }));
    }
    lines
}

pub(super) fn append_text_block<'a>(
    lines: &mut Vec<Line<'a>>,
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

fn append_rich_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: ratatui::style::Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    if !text.contains("```") {
        append_text_block(lines, text, color, prefix);
        return;
    }

    let Some(blocks) = parse_fenced_text_blocks(text) else {
        append_text_block(lines, text, color, prefix);
        return;
    };

    for block in blocks {
        match block {
            ParsedTextBlock::Plain(plain) => append_text_block(lines, &plain, color, prefix),
            ParsedTextBlock::Code {
                language,
                body,
                raw,
            } => {
                if let Some(language) = language.as_deref() {
                    if matches!(language, "diff" | "patch") {
                        if let Some(diff_lines) =
                            render_structured_diff_lines(&body, None, prefix, width, theme)
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

fn transcript_surface_content_width(width: u16) -> u16 {
    width.saturating_sub(TRANSCRIPT_SURFACE_RAIL_WIDTH).max(1)
}

fn transcript_top_level_surface(theme: &Theme) -> Color {
    elevated_card_surface(theme)
}

fn assistant_context_rail_color(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    assistant_color: Color,
) -> Color {
    if turn.body_blocks.is_empty() {
        assistant_color
    } else {
        theme.text.secondary
    }
}

fn transcript_nested_surface(theme: &Theme) -> Color {
    theme.surface.panel
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
    let mut spans = Vec::new();
    if !indent.is_empty() {
        spans.push(Span::raw(indent.to_string()));
    }
    spans.push(Span::styled(
        TRANSCRIPT_RAIL_GLYPH,
        Style::default().fg(rail_color).bg(surface),
    ));
    spans.push(surface_span(" ", Style::default(), surface));
    let mut visible_width = indent.chars().count() + TRANSCRIPT_RAIL_GLYPH.chars().count() + 1;
    for span in content_spans {
        visible_width += span.content.chars().count();
        spans.push(surface_span(span.content.into_owned(), span.style, surface));
    }
    let remaining = usize::from(width).saturating_sub(visible_width);
    if remaining > 0 {
        spans.push(surface_span(
            " ".repeat(remaining),
            Style::default(),
            surface,
        ));
    }
    lines.push(Line::from(spans));
}

fn append_tool_detail_row(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    rail_color: Color,
    surface: Color,
    label: &str,
    value: &str,
    label_style: Style,
    value_style: Style,
    theme: &Theme,
    width: u16,
) {
    let mut spans = Vec::new();
    spans.push(Span::styled(label.to_string(), label_style));
    if !value.is_empty() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(value.to_string(), value_style));
    }
    append_nested_surface_row(lines, indent, rail_color, surface, spans, width);
}

fn append_parenthesized_meta(
    spans: &mut Vec<Span<'static>>,
    segments: Vec<(String, Style)>,
    theme: &Theme,
) {
    if segments.is_empty() {
        return;
    }

    let separator_style = muted_meta_style(theme);
    spans.push(Span::styled(" (", separator_style));
    for (index, (text, style)) in segments.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", separator_style));
        }
        spans.push(Span::styled(text, style));
    }
    spans.push(Span::styled(")", separator_style));
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
            transcript_surface_content_width(width),
        );
        append_rich_text_block(
            lines,
            text,
            block_style.body_style.fg.unwrap_or(theme.text.primary),
            &continuation_prefix,
            theme,
            transcript_surface_content_width(width),
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
            transcript_surface_content_width(width),
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
            transcript_surface_content_width(width),
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
            transcript_surface_content_width(width),
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
        tool_id: "shell".to_string(),
        args_summary: r#"{"cmd":"false"}"#.to_string(),
        args_digest: "digest".to_string(),
        status: ToolCallDisplayStatus::Failed,
        output_summary: Some("command failed".to_string()),
        output_digest: Some("out-digest".to_string()),
        truncated_output: Some("command failed".to_string()),
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
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
                tool_id: "shell".to_string(),
                status: ToolCallDisplayStatus::Failed,
            },
            inline_summary: None,
            detail_blocks: vec![
                TranscriptToolCallDetailBlock::Args("cmd=false".to_string()),
                TranscriptToolCallDetailBlock::Error("command failed".to_string()),
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
        args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
        args_digest: "digest".to_string(),
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("24 lines read".to_string()),
        output_digest: Some("out-digest".to_string()),
        truncated_output: Some("24 lines read".to_string()),
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
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

    let assistant_row = lines
        .iter()
        .position(|line| line.contains("assistant"))
        .expect("assistant row");
    let answer_row = lines
        .iter()
        .enumerate()
        .skip(assistant_row + 1)
        .find_map(|(index, line)| line.contains("assistant answer").then_some(index))
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
        .find_map(|(index, line)| line.contains("tool fs.read").then_some(index))
        .expect("tool row");

    assert!(assistant_row < answer_row);
    assert!(answer_row < thinking_row);
    assert!(thinking_row < tool_row);
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
        layout.sections[0].content_height > layout.sections[0].lines.len(),
        "wrapped transcript section should measure taller than its raw line count"
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
fn transcript_section_model_test_activity(
    request_id: &str,
    status: ActivityStatus,
    transcript_text: &str,
) -> ActivityEntry {
    ActivityEntry {
        request_id: request_id.to_string(),
        model_id: "gpt-5.4".to_string(),
        provider_id: "openai".to_string(),
        status,
        user_message: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: transcript_text.to_string(),
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
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
            "▎  ◷ permission checkpoint (fail-closed · review required)"
        );
        assert_eq!(lines[1], "▎  Apply hashline edit to demo.txt");
        assert!(lines
            .iter()
            .all(|line| !line.contains('╭') && !line.contains('╰') && !line.contains('│')));
    }

    #[test]
    fn streaming_assistant_header_uses_reserved_active_label() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "request-streaming-header".to_string(),
            model_id: "gpt-5.4".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Streaming,
            user_message: None,
            request_data: None,
            thinking_text: String::new(),
            transcript_text: String::new(),
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
        }]);
        app.selected_activity_index = 0;

        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            &app,
            &Theme::default(),
            80,
        ));

        assert_eq!(lines[0], "▎  ◐ assistant (active · current)");
        assert_eq!(lines[1], "▎  Waiting for response…");
    }
}
