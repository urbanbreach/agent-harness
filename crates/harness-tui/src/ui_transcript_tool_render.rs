// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::*;

use crate::app::{ToolCallPresentation, ToolCallPresentationStatus};

pub(super) fn build_tool_header_spans(
    header: &TranscriptToolCallHeader,
    theme: &Theme,
    title_style: Style,
    marker_style: Style,
    width: usize,
) -> Vec<Span<'static>> {
    use super::super::ui_tool_output::safe_tool_text;
    let search = matches!(
        header.tool_id.as_str(),
        "fs.glob" | "glob" | "fs.grep" | "grep"
    );
    let title_style = if header.selected {
        title_style.fg(theme.text.primary)
    } else {
        title_style
    };
    let title = collapse_inline_whitespace(&safe_tool_text(&header.title));
    let (label, argument) = split_tool_header_title(&title, &header.tool_id);
    let mut spans = Vec::new();
    let marker = completed_tool_marker(header.presentation.status, theme);
    spans.push(Span::styled(format!("{marker} "), marker_style));
    let _ = header.icon;
    spans.push(Span::styled(
        label.to_string(),
        title_style.add_modifier(Modifier::BOLD),
    ));
    let mut variable = None;
    if !argument.is_empty() {
        spans.push(Span::raw(" "));
        variable = (!search).then_some(spans.len());
        let argument_style = if header.visual_style == TranscriptToolCallVisualStyle::TaskInline
            || title_style.fg == Some(theme.text.secondary)
        {
            title_style.fg(theme.text.secondary)
        } else if matches!(
            header.tool_id.as_str(),
            "fs.glob" | "glob" | "fs.grep" | "grep"
        ) {
            title_style.fg(theme.status.success)
        } else if matches!(
            header.tool_id.as_str(),
            "web.fetch" | "webfetch" | "search.web" | "websearch"
        ) || is_mcp_tool_id(&header.tool_id)
        {
            title_style.fg(theme.status.warning)
        } else {
            title_style
        };
        spans.push(Span::styled(argument.to_string(), argument_style));
    }
    if let Some(path_metadata) = header.path_metadata.as_deref() {
        spans.push(Span::styled(
            if matches!(
                header.tool_id.as_str(),
                "fs.glob" | "glob" | "fs.grep" | "grep"
            ) {
                " in "
            } else {
                " "
            },
            title_style,
        ));
        variable = (header.disclosure_state != Some(TranscriptToolCallDisclosureState::Expanded)
            && !unfinished_edit_header(header))
        .then_some(spans.len());
        spans.push(Span::styled(
            super::super::ui_tool_paths::tool_header_path(
                path_metadata,
                header.disclosure_state == Some(TranscriptToolCallDisclosureState::Expanded)
                    || unfinished_edit_header(header)
                    || search,
            ),
            title_style.fg(if title_style.fg == Some(theme.text.secondary) {
                theme.text.secondary
            } else {
                super::super::ui_tool_paths::tool_path_color(theme)
            }),
        ));
    }
    if let Some(subtitle) = header.subtitle.as_deref() {
        let subtitle = collapse_inline_whitespace(&safe_tool_text(subtitle));
        let expanded = header.disclosure_state == Some(TranscriptToolCallDisclosureState::Expanded);
        let show_subtitle = if search && !expanded {
            fit_search_header_subtitle(&mut spans, variable, &subtitle, width)
        } else {
            expanded
                || display_width(label)
                    .saturating_add(display_width(&subtitle))
                    .saturating_add(6)
                    < width
        };
        if show_subtitle {
            append_tool_subtitle_spans(&mut spans, subtitle, &header.tool_id, theme);
        }
    }
    if let Some(index) =
        variable.filter(|_| header.visual_style != TranscriptToolCallVisualStyle::TaskInline)
    {
        let fixed = spans
            .iter()
            .enumerate()
            .filter(|(candidate, _)| *candidate != index)
            .map(|(_, span)| display_width(&span.content))
            .sum::<usize>();
        spans[index].content =
            truncate_plain_text(&spans[index].content, width.saturating_sub(fixed)).into();
    }
    // EntryRenderer paints a single header row. A long unbreakable tool name
    // or fixed subtitle must not wrap and push following diamonds down.
    super::super::ui_tool_wrapping::clip(spans, width)
}

fn append_tool_subtitle_spans(
    spans: &mut Vec<Span<'static>>,
    subtitle: String,
    tool_id: &str,
    theme: &Theme,
) {
    let separator = if subtitle.starts_with(['(', '+']) {
        " "
    } else {
        " · "
    };
    spans.push(Span::styled(separator, muted_meta_style(theme)));
    if let Some((added, removed)) = subtitle.split_once("/-") {
        spans.push(Span::styled(
            added.to_string(),
            Style::default().fg(theme.terminal_colors.diff_added_highlight),
        ));
        spans.push(Span::styled("/", muted_meta_style(theme)));
        spans.push(Span::styled(
            format!("-{removed}"),
            Style::default().fg(theme.terminal_colors.diff_removed_highlight),
        ));
    } else {
        let style = if matches!(tool_id, "fs.ls" | "list") {
            Style::default().fg(theme.text.secondary)
        } else if subtitle.starts_with('(') {
            Style::default().fg(theme.terminal_colors.muted)
        } else {
            muted_meta_style(theme)
        };
        spans.push(Span::styled(subtitle, style));
    }
}

fn fit_search_header_subtitle(
    spans: &mut [Span<'static>],
    path_index: Option<usize>,
    subtitle: &str,
    width: usize,
) -> bool {
    let suffix_width = display_width(subtitle).saturating_add(1);
    if let Some(index) = path_index {
        let fixed = spans
            .iter()
            .enumerate()
            .filter(|(candidate, _)| *candidate != index)
            .map(|(_, span)| span.width())
            .sum::<usize>();
        spans[index].content = super::super::ui_tool_paths::shorten_search_path(
            &spans[index].content,
            width.saturating_sub(fixed.saturating_add(suffix_width)),
        )
        .into();
    }
    spans
        .iter()
        .map(Span::width)
        .sum::<usize>()
        .saturating_add(suffix_width)
        <= width
}

fn tool_header_width(width: u16) -> usize {
    usize::from(transcript_surface_content_width(width, false))
        .saturating_sub(surface_prefix_width(TRANSCRIPT_ASSISTANT_BODY_PREFIX))
}

fn unfinished_edit_header(header: &TranscriptToolCallHeader) -> bool {
    matches!(
        header.tool_id.as_str(),
        "edit" | "write" | "fs.write" | "edit.hashline_apply"
    ) && matches!(
        header.presentation.status,
        ToolCallPresentationStatus::Queued
            | ToolCallPresentationStatus::Running
            | ToolCallPresentationStatus::Waiting
    )
}

pub(super) fn completed_tool_marker(
    status: ToolCallPresentationStatus,
    theme: &Theme,
) -> &'static str {
    // The reference keeps one bullet through the lifecycle; color carries state.
    // Preserve distinct status cues in the accessibility fallback.
    if theme.glyph_mode() == crate::theme::GlyphMode::Preferred {
        return theme.live_shell.transcript_glyphs.tool_marker;
    }
    match status {
        ToolCallPresentationStatus::Queued => theme.live_shell.glyphs.queued,
        ToolCallPresentationStatus::Running => theme.live_shell.glyphs.running,
        ToolCallPresentationStatus::Waiting => theme.live_shell.glyphs.pending_permission,
        ToolCallPresentationStatus::Succeeded => theme.live_shell.transcript_glyphs.success_marker,
        ToolCallPresentationStatus::Failed => theme.live_shell.glyphs.failed,
        ToolCallPresentationStatus::Cancelled => theme.live_shell.glyphs.cancelled,
    }
}

fn split_tool_header_title<'a>(title: &'a str, tool_id: &str) -> (&'a str, &'a str) {
    if super::super::ui_tool_titles::generic_tool_id(tool_id) {
        return title.split_once(": ").unwrap_or((title, ""));
    }
    [
        "Parallel Web Search",
        "Exa Web Search",
        "Web Search",
        "Exa Code Search",
        "AST Search",
        "AST Replace",
    ]
    .into_iter()
    .find_map(|label| {
        title
            .strip_prefix(label)
            .and_then(|rest| rest.strip_prefix(' '))
            .map(|argument| (label, argument))
    })
    .or_else(|| title.split_once(' '))
    .unwrap_or((title, ""))
}

fn tool_call_marker_style(
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    inactive_color: Color,
) -> Style {
    let color = if matches!(
        tool_call.header.presentation.status,
        ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Running
    ) && matches!(tool_call.rail_motion, ToolRailMotion::Running { .. })
    {
        let accent = if tool_call.header.visual_style == TranscriptToolCallVisualStyle::TaskInline {
            super::ui_transcript_style::blend_color(theme.surface.shell, theme.text.accent, 0.5)
        } else {
            theme.text.accent
        };
        super::ui_transcript_surface::tool_rail_motion_color(
            theme.surface.shell,
            accent,
            Some(tool_call.rail_motion),
            0,
            tool_call.animation_phase,
        )
    } else {
        match tool_call.header.presentation.status {
            ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Running
                if unfinished_edit_header(&tool_call.header) =>
            {
                theme.text.tertiary
            }
            ToolCallPresentationStatus::Running => inactive_color,
            ToolCallPresentationStatus::Waiting => theme.status.warning,
            ToolCallPresentationStatus::Failed => theme.terminal_colors.error,
            ToolCallPresentationStatus::Cancelled
                if tool_call.header.visual_style == TranscriptToolCallVisualStyle::TaskInline =>
            {
                theme.terminal_colors.error
            }
            ToolCallPresentationStatus::Cancelled => theme.status.disabled,
            ToolCallPresentationStatus::Succeeded
                if tool_call
                    .tool_call_id
                    .starts_with("background-notification:") =>
            {
                theme.status.success
            }
            ToolCallPresentationStatus::Succeeded
                if shell_tool_uses_harness_bash_card(tool_call) =>
            {
                theme.status.success
            }
            ToolCallPresentationStatus::Succeeded
                if tool_call.header.visual_style == TranscriptToolCallVisualStyle::TaskInline
                    || matches!(tool_call.header.tool_id.as_str(), "skill" | "skill.load") =>
            {
                theme.text.secondary
            }
            ToolCallPresentationStatus::Queued
                if (is_mcp_tool_id(&tool_call.header.tool_id)
                    || super::super::ui_tool_titles::generic_tool_id(
                        &tool_call.header.tool_id,
                    ))
                    && !tool_call.expanded
                    && !tool_call.details_preview_visible =>
            {
                theme.text.secondary
            }
            ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Succeeded => {
                if tool_call.details_visible() {
                    theme.text.tertiary
                } else {
                    theme.text.secondary
                }
            }
        }
    };
    let dim_terminal = !tool_call.header.selected
        && ((shell_tool_uses_harness_bash_card(tool_call) && !tool_call.details_visible())
            || (tool_call.header.presentation.status == ToolCallPresentationStatus::Failed
                && !tool_call.details_visible())
            || (tool_call.header.visual_style == TranscriptToolCallVisualStyle::TaskInline
                && tool_call
                    .tool_call_id
                    .starts_with("background-notification:")));
    let color = if dim_terminal
        && matches!(
            tool_call.header.presentation.status,
            ToolCallPresentationStatus::Succeeded
                | ToolCallPresentationStatus::Failed
                | ToolCallPresentationStatus::Cancelled
        ) {
        super::ui_transcript_style::blend_color(theme.surface.shell, color, 0.5)
    } else {
        color
    };
    tool_call_header_style(tool_call.header.struck_out, color)
}

#[expect(
    clippy::too_many_arguments,
    reason = "card shell dispatch keeps transcript styling explicit at the call site"
)]
fn append_card_surface_row_with_target(
    lines: &mut Vec<Line<'static>>,
    interaction_rows: &mut Vec<Option<TranscriptInteractionRow>>,
    target: Option<TranscriptMouseTarget>,
    card_shell: Option<TranscriptToolCardShell>,
    fallback_indent: &str,
    fallback_surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    if let Some(shell) = card_shell {
        append_nested_surface_row_with_target(
            lines,
            interaction_rows,
            target,
            NestedSurfaceChrome {
                indent: shell.indent,
                rail_color: shell.rail_color,
                surface: shell.surface,
                content_leading_spaces: shell.content_leading_spaces,
            },
            content_spans,
            width,
        );
    } else {
        append_surface_row_with_target(
            lines,
            interaction_rows,
            target,
            fallback_indent,
            fallback_surface,
            content_spans,
            width,
        );
    }
}

fn append_card_surface_row(
    lines: &mut Vec<Line<'static>>,
    card_shell: Option<TranscriptToolCardShell>,
    fallback_indent: &str,
    fallback_surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    if let Some(shell) = card_shell {
        append_nested_surface_row(
            lines,
            shell.indent,
            shell.rail_color,
            shell.surface,
            shell.content_leading_spaces,
            content_spans,
            width,
        );
    } else {
        append_surface_row(
            lines,
            fallback_indent,
            fallback_surface,
            content_spans,
            width,
        );
    }
}

fn append_card_prebuilt_surface_lines(
    lines: &mut Vec<Line<'static>>,
    card_shell: Option<TranscriptToolCardShell>,
    fallback_indent: &str,
    fallback_surface: Color,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    if let Some(shell) = card_shell {
        append_prebuilt_nested_surface_lines(
            lines,
            shell.indent,
            shell.rail_color,
            shell.surface,
            prebuilt,
            width,
        );
    } else {
        append_prebuilt_surface_lines(lines, fallback_indent, fallback_surface, prebuilt, width);
    }
}

pub(super) fn append_tool_call_section_lines(
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
    super::ui_transcript_tool_hooks::append(&mut render, tool_call, theme, width, base_surface);
    paint_tool_completion_rail(&mut render.lines, tool_call, theme);
    if transcript_target_is_hovered(
        coalesced_tool_header_target(tool_call).as_ref(),
        tool_call.hovered_target.as_ref(),
    ) {
        if let Some(header) = render.lines.first_mut() {
            apply_header_hover(header, tool_call.expanded, theme);
        }
    }
    render
}

pub(super) fn apply_header_hover(line: &mut Line<'static>, expanded: bool, theme: &Theme) {
    if expanded {
        return;
    }
    let caret = if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
        if expanded {
            "v"
        } else {
            ">"
        }
    } else if expanded {
        "⌄"
    } else {
        "›"
    };
    let hover = super::ui_transcript_style::blend_color(
        theme.surface.shell,
        theme.markdown.code_background,
        0.5,
    );
    line.style = line.style.bg(hover);
    for span in &mut line.spans {
        span.style = span.style.bg(hover);
        if let Some(rest) = span
            .content
            .strip_prefix(theme.live_shell.transcript_glyphs.tool_marker)
            .or_else(|| {
                span.content
                    .strip_prefix(theme.live_shell.transcript_glyphs.group_marker)
            })
        {
            span.content = format!("{caret}{rest}").into();
        }
    }
}

fn append_inline_tool_section_lines(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let fg = if (tool_call.details_visible() && tool_call.has_detail_content())
        || unfinished_edit_header(&tool_call.header)
    {
        theme.text.primary
    } else {
        theme.text.secondary
    };
    let style = tool_call_header_style(tool_call.header.struck_out, fg);

    let marker_style = tool_call_marker_style(tool_call, theme, fg);
    let hanging_indent = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  ");
    for (index, spans) in tool_header_rows(tool_call, theme, style, marker_style, width)
        .into_iter()
        .enumerate()
    {
        append_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            coalesced_tool_header_target(tool_call),
            if index == 0 {
                TRANSCRIPT_ASSISTANT_BODY_PREFIX
            } else {
                &hanging_indent
            },
            base_surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, None);
}

fn coalesced_tool_header_target(
    tool_call: &TranscriptToolCallSection,
) -> Option<TranscriptMouseTarget> {
    let viewer_only =
        tool_call.details_collapsed_by_default && tool_call.header.disclosure_state.is_none();
    if tool_call.header.disclosure_state.is_none() && !viewer_only {
        None
    } else {
        tool_header_target(&tool_call.tool_call_id, true)
    }
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
    let fg = theme.text.secondary;
    let style = tool_call_header_style(tool_call.header.struck_out, fg);
    let surface = base_surface;
    let spans = build_tool_header_spans(
        &tool_call.header,
        theme,
        style,
        tool_call_marker_style(tool_call, theme, fg),
        tool_header_width(width),
    );
    append_surface_row_with_bounded_target(
        &mut render.lines,
        &mut render.interaction_rows,
        target.clone(),
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        surface,
        spans,
        transcript_surface_content_width(width, false),
    );

    if !tool_call.details_visible() {
        return;
    }

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
                        TRANSCRIPT_TOOL_BODY_PREFIX,
                        surface,
                        spans,
                        transcript_surface_content_width(width, false),
                    );
                }
            }
            TranscriptToolCallDetailBlock::Markdown { text } => {
                let start = render.lines.len();
                append_rich_text_block(
                    &mut render.lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_TOOL_BODY_PREFIX,
                    theme,
                    transcript_surface_content_width(width, false),
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
                        group: Default::default(),
                        hook_executions: Vec::new(),
                        tool_call_id: tool_call.tool_call_id.clone(),
                        coalesced_tool_call_ids: tool_call.coalesced_tool_call_ids.clone(),
                        child_session_id: tool_call.child_session_id.clone(),
                        subagent_background: tool_call.subagent_background,
                        output_truncated: tool_call.output_truncated,
                        replay_read_only: tool_call.replay_read_only,
                        hovered_target: tool_call.hovered_target.clone(),
                        header: tool_call.header.clone(),
                        detail_blocks: vec![detail_block.clone()],
                        details_collapsed_by_default: tool_call.details_collapsed_by_default,
                        details_preview_visible: tool_call.details_preview_visible,
                        animation_phase: tool_call.animation_phase,
                        expanded: tool_call.expanded,
                        rail_motion: tool_call.rail_motion,
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
        append_shell_tool_harness_card(render, tool_call, theme, width, base_surface);
        return;
    }

    let is_todo_block = tool_call_is_todo(tool_call);
    let surface = base_surface;
    // Like EntryRenderer, reserve the same chrome for every tool family.
    // A nested todo shell used to shift its diamond when details opened.
    let card_shell = None;
    let title_style = tool_call_header_style(
        tool_call.header.struck_out,
        if !unfinished_edit_header(&tool_call.header)
            && (is_todo_block || !tool_call.details_visible() || !tool_call.has_detail_content())
        {
            theme.text.secondary
        } else {
            theme.text.primary
        },
    );
    let header_target = coalesced_tool_header_target(tool_call);

    let marker_style = tool_call_marker_style(tool_call, theme, title_style.fg.unwrap_or_default());
    let hanging_indent = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  ");
    for (index, title_spans) in tool_header_rows(tool_call, theme, title_style, marker_style, width)
        .into_iter()
        .enumerate()
    {
        append_card_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target.clone(),
            card_shell,
            if index == 0 {
                TRANSCRIPT_ASSISTANT_BODY_PREFIX
            } else {
                &hanging_indent
            },
            surface,
            title_spans,
            transcript_surface_content_width(width, false),
        );
    }

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, card_shell);
}

pub(super) fn tool_call_is_todo(tool_call: &TranscriptToolCallSection) -> bool {
    matches!(
        tool_call.header.tool_id.as_str(),
        "todo.write" | "todowrite"
    )
}

fn tool_header_rows(
    tool: &TranscriptToolCallSection,
    theme: &Theme,
    title_style: Style,
    marker_style: Style,
    width: u16,
) -> Vec<Vec<Span<'static>>> {
    let wrap =
        tool.details_visible() && matches!(tool.header.tool_id.as_str(), "web.fetch" | "webfetch");
    let spans = build_tool_header_spans(
        &tool.header,
        theme,
        title_style,
        marker_style,
        if wrap {
            usize::MAX
        } else {
            tool_header_width(width)
        },
    );
    if !wrap {
        return vec![spans];
    }
    let marker = spans.first().cloned();
    super::super::ui_tool_wrapping::words(
        spans.into_iter().skip(1).collect(),
        tool_header_width(width).saturating_sub(2),
    )
    .into_iter()
    .enumerate()
    .map(|(index, mut row)| {
        if index == 0 {
            row.insert(0, marker.clone().unwrap_or_else(|| Span::raw("  ")));
        }
        row
    })
    .collect()
}

pub(super) fn shell_tool_uses_harness_bash_card(tool_call: &TranscriptToolCallSection) -> bool {
    matches!(tool_call.header.tool_id.as_str(), "shell.run" | "bash")
}

fn append_shell_tool_harness_card(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let bash_header = tool_call
        .detail_blocks
        .iter()
        .find_map(|detail_block| match detail_block {
            TranscriptToolCallDetailBlock::BashPanel {
                command,
                description,
                ..
            } => Some((command.as_str(), description.as_deref())),
            _ => None,
        });
    let mut header = tool_call.header.clone();
    if let Some((command, description)) = bash_header {
        header.title = format!(
            "Run {}",
            description
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(command)
                .trim()
        );
    } else if !header.title.starts_with("Run ") {
        header.title = format!("Run {}", header.title.trim());
    }

    let title_style = tool_call_header_style(
        header.struck_out,
        if tool_call.header.selected
            || (tool_call.details_visible() && tool_call.has_detail_content())
        {
            theme.text.primary
        } else {
            theme.text.secondary
        },
    );
    let marker_style = tool_call_marker_style(tool_call, theme, title_style.fg.unwrap_or_default());
    let header_rows = if let Some(rows) = shell_command_title_rows(
        tool_call,
        bash_header,
        theme,
        width,
        title_style,
        marker_style,
    ) {
        rows
    } else if let Some(description) = bash_header
        .and_then(|(_, description)| description)
        .filter(|description| tool_call.details_visible() && !description.trim().is_empty())
    {
        let marker = completed_tool_marker(header.presentation.status, theme);
        let mut rows = vec![vec![
            Span::styled(format!("{marker} "), marker_style),
            Span::styled("Run ", title_style.add_modifier(Modifier::BOLD)),
        ]];
        let prefix_width = rows[0].iter().map(Span::width).sum::<usize>();
        let description =
            collapse_inline_whitespace(&super::super::ui_tool_output::safe_tool_text(description));
        for (index, mut row) in super::super::ui_tool_wrapping::words(
            vec![Span::styled(description, title_style)],
            tool_header_width(width).saturating_sub(prefix_width).max(1),
        )
        .into_iter()
        .enumerate()
        {
            if index == 0 {
                rows[0].append(&mut row);
            } else {
                rows.push(row);
            }
        }
        rows
    } else {
        vec![build_tool_header_spans(
            &header,
            theme,
            title_style,
            marker_style,
            tool_header_width(width),
        )]
    };
    let header_target =
        tool_header_target(&tool_call.tool_call_id, header.disclosure_state.is_some());
    // Keep the hanging indent in the surface prefix so wrapping does not trim it.
    let hanging_indent = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}    ");
    for (index, row) in header_rows.into_iter().enumerate() {
        append_card_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target.clone(),
            None,
            if index == 0 {
                TRANSCRIPT_ASSISTANT_BODY_PREFIX
            } else {
                &hanging_indent
            },
            base_surface,
            row,
            transcript_surface_content_width(width, false),
        );
    }

    if !tool_call.details_visible() {
        return;
    }

    for detail_block in &tool_call.detail_blocks {
        let start = render.lines.len();
        match detail_block {
            TranscriptToolCallDetailBlock::BashPanel {
                command,
                output,
                description,
                expand_hint: _,
            } => {
                append_harness_bash_panel(
                    &mut render.lines,
                    HarnessBashPanel {
                        command: if description
                            .as_deref()
                            .is_some_and(|value| !value.trim().is_empty())
                        {
                            command
                        } else {
                            ""
                        },
                        output,
                        description: None,
                        expanded: tool_call.expanded,
                    },
                    theme,
                    width,
                    base_surface,
                );
                let target = tool_header_target(
                    &tool_call.tool_call_id,
                    tool_call.header.disclosure_state.is_some(),
                );
                for line in &render.lines[start..] {
                    let hint = if tool_call.expanded {
                        "Click to collapse"
                    } else {
                        "Click to expand"
                    };
                    let interaction = Some(hint)
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
                    base_surface,
                    None,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
                        group: Default::default(),
                        hook_executions: Vec::new(),
                        tool_call_id: tool_call.tool_call_id.clone(),
                        coalesced_tool_call_ids: tool_call.coalesced_tool_call_ids.clone(),
                        child_session_id: tool_call.child_session_id.clone(),
                        subagent_background: tool_call.subagent_background,
                        output_truncated: tool_call.output_truncated,
                        replay_read_only: tool_call.replay_read_only,
                        hovered_target: tool_call.hovered_target.clone(),
                        header: tool_call.header.clone(),
                        detail_blocks: vec![detail_block.clone()],
                        details_collapsed_by_default: tool_call.details_collapsed_by_default,
                        details_preview_visible: tool_call.details_preview_visible,
                        animation_phase: tool_call.animation_phase,
                        expanded: tool_call.expanded,
                        rail_motion: tool_call.rail_motion,
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

fn paint_tool_completion_rail(
    lines: &mut [Line<'static>],
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
) {
    let flashing = tool_call.details_visible()
        && matches!(tool_call.rail_motion, ToolRailMotion::FinishFlash { .. });
    for line in lines {
        if line.spans.is_empty() {
            // Empty separators reserve the same accent column as content rows,
            // even after the completion flash has settled.
            line.spans.push(Span::raw(" "));
        }
        if !flashing {
            continue;
        }
        if let Some(prefix) = line
            .spans
            .first_mut()
            .filter(|span| span.content.starts_with(' '))
        {
            // Replace an existing gutter cell so the transient rail cannot reflow content.
            prefix
                .content
                .to_mut()
                .replace_range(0..1, theme.live_shell.transcript_glyphs.rail);
            prefix.style = prefix.style.fg(theme.status.success);
        }
    }
}

fn shell_command_title_rows(
    tool: &TranscriptToolCallSection,
    bash_header: Option<(&str, Option<&str>)>,
    theme: &Theme,
    width: u16,
    title_style: Style,
    marker_style: Style,
) -> Option<Vec<Vec<Span<'static>>>> {
    let (command, _) = bash_header.filter(|(_, description)| {
        (tool.details_visible() || tool.header.selected)
            && description.is_none_or(|description| description.trim().is_empty())
    })?;
    let command = super::super::ui_tool_output::safe_tool_text(command);
    let command = if tool.details_visible() {
        command
    } else {
        truncate_plain_text(
            &collapse_inline_whitespace(&command),
            tool_header_width(width).saturating_sub(6),
        )
    };
    let highlighted = super::super::ui_syntax_highlight::render_highlighted_code_block(
        Some("bash"),
        &command,
        &command,
        "",
        theme.text.primary,
        theme,
    );
    let rows = super::super::ui_tool_wrapping::shell(
        highlighted,
        tool_header_width(width).saturating_sub(6),
    );
    Some(
        rows.into_iter()
            .enumerate()
            .map(|(index, mut row)| {
                if index == 0 {
                    row.insert(
                        0,
                        Span::styled("Run ", title_style.add_modifier(Modifier::BOLD)),
                    );
                    row.insert(
                        0,
                        Span::styled(
                            format!(
                                "{} ",
                                completed_tool_marker(tool.header.presentation.status, theme)
                            ),
                            marker_style,
                        ),
                    );
                }
                super::super::ui_tool_wrapping::clip(
                    row,
                    tool_header_width(width).saturating_sub(if index == 0 { 0 } else { 4 }),
                )
            })
            .collect(),
    )
}

pub(super) fn append_tool_call_detail_blocks(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    if !tool_call.details_visible() {
        return;
    }

    for detail_block in &tool_call.detail_blocks {
        let start = render.lines.len();
        match detail_block {
            TranscriptToolCallDetailBlock::Recorded(output) => {
                let content_width = transcript_surface_content_width(width, false)
                    .saturating_sub(
                        u16::try_from(surface_prefix_width(TRANSCRIPT_TOOL_BODY_PREFIX))
                            .unwrap_or(u16::MAX),
                    )
                    .max(1);
                let rows = output.lines(theme, content_width, tool_call.expanded);
                append_prebuilt_surface_lines(
                    &mut render.lines,
                    TRANSCRIPT_TOOL_BODY_PREFIX,
                    base_surface,
                    rows,
                    transcript_surface_content_width(width, false),
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::ReadOutput { text, start_line } => {
                append_read_output(
                    render,
                    tool_call,
                    text,
                    *start_line,
                    theme,
                    width,
                    theme.markdown.code_background,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::Message { text, tone }
                if super::super::ui_tool_titles::generic_tool_id(&tool_call.header.tool_id) =>
            {
                let style = match tone {
                    TranscriptToolCallDetailTone::Primary => {
                        Style::default().fg(theme.text.primary)
                    }
                    TranscriptToolCallDetailTone::Secondary => muted_meta_style(theme),
                    TranscriptToolCallDetailTone::Error => Style::default().fg(theme.status.error),
                };
                let rows = text
                    .split('\n')
                    .flat_map(|line| {
                        super::super::ui_tool_wrapping::words(
                            vec![Span::styled(line.to_string(), style)],
                            tool_header_width(width).saturating_sub(2).max(20),
                        )
                        .into_iter()
                        .map(Line::from)
                    })
                    .collect();
                append_prebuilt_surface_lines(
                    &mut render.lines,
                    TRANSCRIPT_TOOL_BODY_PREFIX,
                    base_surface,
                    rows,
                    transcript_surface_content_width(width, false),
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
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
            TranscriptToolCallDetailBlock::Markdown { text } => {
                append_rich_text_block(
                    &mut render.lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_TOOL_BODY_PREFIX,
                    theme,
                    transcript_surface_content_width(width, false),
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
                expand_hint: _,
            } => {
                append_harness_bash_panel(
                    &mut render.lines,
                    HarnessBashPanel {
                        command,
                        output,
                        description: description.as_deref(),
                        expanded: tool_call.expanded,
                    },
                    theme,
                    width,
                    base_surface,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::StructuredDiff {
                before_source,
                diff_content,
                fallback_path,
                force_stacked,
                plain_numbered,
                highlight_syntax,
                show_file_header,
            } => {
                append_tool_call_diff_block(
                    render,
                    before_source.as_deref(),
                    diff_content,
                    fallback_path.as_deref(),
                    *force_stacked,
                    *plain_numbered,
                    *highlight_syntax,
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

fn append_read_output(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    text: &str,
    start_line: Option<u64>,
    theme: &Theme,
    width: u16,
    surface: Color,
) {
    let text = super::super::ui_tool_output::safe_tool_text(text);
    let body_width = usize::from(transcript_surface_content_width(width, false))
        .saturating_sub(surface_prefix_width(TRANSCRIPT_TOOL_BODY_PREFIX));
    let gutter_width = start_line
        .map(|start| {
            start
                .saturating_add(
                    u64::try_from(text.lines().count().saturating_sub(1)).unwrap_or(u64::MAX),
                )
                .to_string()
                .len()
        })
        .unwrap_or(0)
        .min(body_width.saturating_sub(3));
    let content_width = body_width
        .saturating_sub(if gutter_width > 0 {
            gutter_width + 2
        } else {
            0
        })
        .max(1);
    let mut rows = Vec::new();
    let language = tool_call.header.path_metadata.as_deref();
    let highlighted = super::super::ui_syntax_highlight::render_highlighted_code_block(
        language,
        &text,
        &text,
        "",
        theme.text.primary,
        theme,
    );
    for (index, line) in highlighted.into_iter().enumerate() {
        let mut source = Vec::new();
        if gutter_width > 0 {
            let number = start_line
                .and_then(|start| {
                    u64::try_from(index)
                        .ok()
                        .and_then(|index| start.checked_add(index))
                })
                .map(|number| number.to_string())
                .unwrap_or_default();
            source.push(Span::styled(
                format!("{number:>gutter_width$}  "),
                Style::default().fg(theme.terminal_colors.muted),
            ));
        }
        // Expand source tabs before adding the line-number gutter to their
        // column budget, then wrap the combined line like the reference.
        source.extend(super::super::ui_transcript_surface::expand_preformatted_tabs(line.spans));
        let wrapped =
            super::super::ui_transcript_surface::wrap_preformatted_spans(source, content_width);
        rows.extend(wrapped.into_iter().map(Line::from));
    }
    let rows = super::super::ui_tool_output::measured_output_preview(
        rows,
        (5, 3),
        tool_call.expanded,
        Style::default().fg(theme.text.secondary),
    );
    if !rows.is_empty() {
        render.lines.push(Line::default());
    }
    append_prebuilt_surface_lines(
        &mut render.lines,
        TRANSCRIPT_TOOL_BODY_PREFIX,
        surface,
        rows,
        transcript_surface_content_width(width, false),
    );
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
    append_card_surface_row_with_target(
        &mut render.lines,
        &mut render.interaction_rows,
        header_target,
        card_shell,
        TRANSCRIPT_TOOL_BODY_PREFIX,
        base_surface,
        spans,
        transcript_surface_content_width(width, false),
    );

    if file_section.disclosure_state == TranscriptToolCallDisclosureState::Expanded {
        let nested_tool = TranscriptToolCallSection {
            group: Default::default(),
            hook_executions: Vec::new(),
            tool_call_id: file_section.tool_call_id.clone(),
            coalesced_tool_call_ids: vec![file_section.tool_call_id.clone()],
            child_session_id: None,
            subagent_background: false,
            output_truncated: false,
            replay_read_only: false,
            hovered_target: None,
            header: TranscriptToolCallHeader {
                selected: false,
                tool_id: String::new(),
                title: String::new(),
                subtitle: None,
                path_metadata: None,
                icon: None,
                presentation: ToolCallPresentation::from_display_status(
                    ToolCallDisplayStatus::Succeeded,
                ),
                visual_style: TranscriptToolCallVisualStyle::Block,
                struck_out: false,
                disclosure_state: None,
            },
            detail_blocks: file_section.detail_blocks.clone(),
            details_collapsed_by_default: false,
            details_preview_visible: false,
            animation_phase: 0,
            expanded: true,
            rail_motion: ToolRailMotion::Settled,
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
        TranscriptToolCallDetailTone::Secondary => muted_meta_style(theme),
        TranscriptToolCallDetailTone::Error => Style::default().fg(theme.status.error),
    };

    for row in text.split('\n') {
        // Keep the reference's per-line Q&A/output indentation outside the
        // prose wrapper, which otherwise trims leading spaces.
        let content = row.trim_start_matches(' ');
        let indent = format!(
            "{TRANSCRIPT_TOOL_BODY_PREFIX}{}",
            " ".repeat(row.len() - content.len())
        );
        let spans = if row.is_empty() {
            Vec::new()
        } else {
            vec![Span::styled(content.to_string(), style)]
        };
        append_card_surface_row(
            lines,
            card_shell,
            &indent,
            base_surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }
}

fn format_assistant_error_display(text: &str) -> String {
    let trimmed = text.trim_end();
    let body = trimmed.trim_start();
    if body.starts_with("Retry failed:") || is_cancel_error_message(body) {
        trimmed.to_string()
    } else {
        format!("Retry failed: {body}")
    }
}

fn is_cancel_error_message(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("interrupted")
        || lower.contains("cancelled")
        || lower.contains("canceled")
        || lower.contains("user cancel")
}

pub(super) fn append_assistant_error_box(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let surface = base_surface;
    let style = Style::default().fg(theme.status.error);
    let trimmed = text.trim_end();
    let display = format_assistant_error_display(trimmed);
    for row in display.lines() {
        let content = row.trim_start_matches(' ');
        let indent = format!(
            "{TRANSCRIPT_ASSISTANT_BODY_PREFIX}{}",
            " ".repeat(row.len().saturating_sub(content.len()))
        );
        if content.is_empty() {
            append_surface_row(lines, "", surface, Vec::new(), width);
            continue;
        }
        let first_w = usize::from(width)
            .saturating_sub(surface_prefix_width(&indent))
            .max(1);
        let wrapped = wrap_surface_spans(vec![Span::styled(content.to_string(), style)], first_w);
        match wrapped.as_slice() {
            [] => append_surface_row(lines, &indent, surface, Vec::new(), width),
            [first] => {
                let first_text: String = first.iter().map(|s| s.content.to_string()).collect();
                append_surface_row(
                    lines,
                    &indent,
                    surface,
                    vec![Span::styled(first_text, style)],
                    width,
                );
            }
            [first, rest @ ..] => {
                let first_text: String = first.iter().map(|s| s.content.to_string()).collect();
                append_surface_row(
                    lines,
                    &indent,
                    surface,
                    vec![Span::styled(first_text, style)],
                    width,
                );
                let rest_text = rest
                    .iter()
                    .map(|visual| {
                        visual
                            .iter()
                            .map(|s| s.content.as_ref())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                append_surface_row(
                    lines,
                    &indent,
                    surface,
                    vec![Span::styled(rest_text, style)],
                    width,
                );
            }
        }
    }
}

fn append_tool_call_todo_list(
    lines: &mut Vec<Line<'static>>,
    items: &[TranscriptTodoItem],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    if items.is_empty() {
        return;
    }
    let render_width = transcript_surface_content_width(width, false);
    let ordered = ordered_todo_items(items);

    lines.push(Line::default());
    for item in ordered {
        let marker_style = item.status.style(theme);
        let content_style = item.status.content_style(theme);
        let spans = vec![
            Span::styled(
                format!("{} ", item.status.checkbox_glyph(theme)),
                marker_style,
            ),
            Span::styled(item.content.clone(), content_style),
        ];
        append_card_surface_row(
            lines,
            card_shell,
            TRANSCRIPT_TOOL_BODY_PREFIX,
            base_surface,
            spans,
            render_width,
        );
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tool diff rendering keeps transcript shell styling explicit at the call site"
)]
fn append_tool_call_diff_block(
    render: &mut ToolSectionRender,
    before_source: Option<&str>,
    diff_content: &str,
    fallback_path: Option<&str>,
    force_stacked: bool,
    plain_numbered: bool,
    highlight_syntax: bool,
    show_file_header: bool,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    let nested_width = transcript_surface_content_width(width, false);
    // EditBlockConfig::indent adds two cells inside the shared content area,
    // for both creates and patches; status never changes the diff origin.
    let body_indent = TRANSCRIPT_NESTED_INDENT;
    let content_width = card_shell
        .map(|shell| {
            nested_width.saturating_sub(
                u16::try_from(nested_surface_prefix_width(shell.indent)).unwrap_or(u16::MAX),
            )
        })
        .unwrap_or_else(|| {
            nested_width.saturating_sub(
                u16::try_from(surface_prefix_width(body_indent)).unwrap_or(u16::MAX),
            )
        })
        .max(1);
    if let Some((diff_lines, hunk_offsets)) =
        super::super::ui_diff::render_diff_with_recorded_source(
            diff_content,
            fallback_path,
            "",
            content_width,
            StructuredDiffRenderOptions {
                force_stacked,
                plain_numbered,
                highlight_intraline: false,
                highlight_syntax,
                show_file_header,
                show_hunk_header: false,
            },
            theme,
            before_source,
        )
    {
        let blank_before = !diff_lines.is_empty()
            && render
                .lines
                .last()
                .is_some_and(|line| !line.spans.is_empty());
        if blank_before {
            render.lines.push(Line::default());
        }
        let start = render.lines.len();
        append_card_prebuilt_surface_lines(
            &mut render.lines,
            card_shell,
            body_indent,
            base_surface,
            diff_lines,
            nested_width,
        );
        render.diff_hunk_offsets.extend(
            hunk_offsets
                .into_iter()
                .map(|offset| start.saturating_add(offset)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_tool_bullet_stays_stable_across_lifecycle_states() {
        // arrange
        let theme = Theme::default();
        let cases = [
            (ToolCallPresentationStatus::Queued, "◆"),
            (ToolCallPresentationStatus::Running, "◆"),
            (ToolCallPresentationStatus::Waiting, "◆"),
            (ToolCallPresentationStatus::Succeeded, "◆"),
            (ToolCallPresentationStatus::Failed, "◆"),
            (ToolCallPresentationStatus::Cancelled, "◆"),
        ];

        // act
        let markers = cases.map(|(status, _)| completed_tool_marker(status, &theme));

        // assert
        assert_eq!(markers, cases.map(|(_, expected)| expected));
    }

    #[test]
    fn completed_tool_marker_uses_ascii_lifecycle_fallbacks() {
        // arrange
        let theme = Theme::default().with_glyph_mode(crate::theme::GlyphMode::Ascii);
        let cases = [
            (ToolCallPresentationStatus::Queued, "."),
            (ToolCallPresentationStatus::Running, "o"),
            (ToolCallPresentationStatus::Waiting, "?"),
            (ToolCallPresentationStatus::Succeeded, "v"),
            (ToolCallPresentationStatus::Failed, "x"),
            (ToolCallPresentationStatus::Cancelled, "-"),
        ];

        // act
        let markers = cases.map(|(status, _)| completed_tool_marker(status, &theme));

        // assert
        assert_eq!(markers, cases.map(|(_, expected)| expected));
    }

    #[test]
    fn generic_tool_header_omits_terminal_count_and_timing_metadata() {
        // arrange
        let header = TranscriptToolCallHeader {
            selected: false,
            tool_id: "edit".to_string(),
            title: "Edit".to_string(),
            subtitle: None,
            path_metadata: Some("src/main.rs".to_string()),
            icon: None,
            presentation: ToolCallPresentation {
                status: ToolCallPresentationStatus::Succeeded,
                duration_ms: Some(1_250),
                result_count: Some(7),
            },
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: Some(TranscriptToolCallDisclosureState::Expanded),
        };

        // act
        let spans = build_tool_header_spans(
            &header,
            &Theme::default(),
            Style::default(),
            Style::default(),
            80,
        );
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        // assert
        assert!(rendered.contains("Edit src/main.rs"));
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[3].style.fg, Some(Color::Rgb(255, 158, 100)));
        assert!(!rendered.contains("7 results"), "{rendered:?}");
        assert!(!rendered.contains("1.2s"), "{rendered:?}");
    }
}
