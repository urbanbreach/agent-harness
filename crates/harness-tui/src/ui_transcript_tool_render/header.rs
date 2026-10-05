use super::*;

pub(super) fn build_tool_header_spans(
    header: &TranscriptToolCallHeader,
    theme: &Theme,
    title_style: Style,
    marker_style: Style,
    width: usize,
) -> Vec<Span<'static>> {
    if header.tool_id == "eval" || header.subtitle.as_deref() == Some("writing") {
        return super::eval::header(header, theme, title_style, marker_style, width);
    }
    if header.visual_style == TranscriptToolCallVisualStyle::TaskInline
        && header.title.starts_with("Subagent ")
        && header.subtitle.is_none()
    {
        let title =
            collapse_inline_whitespace(&crate::ui::ui_tool_output::safe_tool_text(&header.title));
        let style = if header.selected {
            title_style.fg(theme.text.primary)
        } else {
            title_style
        };
        return crate::ui::ui_tool_wrapping::clip(
            subagent_header_spans(header, &title, theme, style, marker_style, width),
            width,
        );
    }
    ordinary_tool_header_spans(header, theme, title_style, marker_style, width)
}

fn ordinary_tool_header_spans(
    header: &TranscriptToolCallHeader,
    theme: &Theme,
    title_style: Style,
    marker_style: Style,
    width: usize,
) -> Vec<Span<'static>> {
    use crate::ui::ui_tool_output::safe_tool_text;
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
    let (label, argument) = if header.visual_style == TranscriptToolCallVisualStyle::TaskInline {
        title.split_once('“').unwrap_or((&title, ""))
    } else {
        split_tool_header_title(&title, &header.tool_id)
    };
    let label = label.trim_end();
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
        spans.push(Span::styled(
            if header.visual_style == TranscriptToolCallVisualStyle::TaskInline {
                format!("“{argument}")
            } else {
                argument.to_string()
            },
            argument_style,
        ));
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
            crate::ui::ui_tool_paths::tool_header_path(
                path_metadata,
                header.disclosure_state == Some(TranscriptToolCallDisclosureState::Expanded)
                    || unfinished_edit_header(header)
                    || search,
            ),
            title_style.fg(if title_style.fg == Some(theme.text.secondary) {
                theme.text.secondary
            } else {
                crate::ui::ui_tool_paths::tool_path_color(theme)
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
    if let Some(index) = variable {
        let fixed = spans
            .iter()
            .enumerate()
            .filter(|(candidate, _)| *candidate != index)
            .map(|(_, span)| display_width(&span.content))
            .sum::<usize>();
        let available = width.saturating_sub(fixed);
        let truncated = truncate_plain_text(&spans[index].content, available);
        spans[index].content = if header.visual_style == TranscriptToolCallVisualStyle::TaskInline
            && available > 1
            && truncated.starts_with('“')
            && !truncated.contains('”')
        {
            format!(
                "{}”",
                truncate_plain_text(&spans[index].content, available - 1)
            )
            .into()
        } else {
            truncated.into()
        };
    }
    // EntryRenderer paints a single header row. A long unbreakable tool name
    // or fixed subtitle must not wrap and push following diamonds down.
    crate::ui::ui_tool_wrapping::clip(spans, width)
}

fn subagent_header_spans(
    header: &TranscriptToolCallHeader,
    title: &str,
    theme: &Theme,
    label_style: Style,
    marker_style: Style,
    width: usize,
) -> Vec<Span<'static>> {
    let detail = title.strip_prefix("Subagent ").unwrap_or(title);
    let detail = if let Some((prefix, quoted)) = detail.split_once('“') {
        if let Some((description, suffix)) = quoted.rsplit_once('”') {
            let reserve =
                if prefix.starts_with("completed in ") || prefix.starts_with("cancelled in ") {
                    13
                } else {
                    11
                };
            let available =
                width.saturating_sub(reserve + display_width(prefix) + display_width(suffix));
            format!(
                "{prefix}“{}”{suffix}",
                if available == 0 {
                    "…".into()
                } else {
                    truncate_plain_text(description, available)
                }
            )
        } else {
            detail.to_owned()
        }
    } else {
        detail.to_owned()
    };
    vec![
        Span::styled(
            format!(
                "{} ",
                completed_tool_marker(header.presentation.status, theme)
            ),
            marker_style,
        ),
        Span::styled("Subagent ", label_style.add_modifier(Modifier::BOLD)),
        Span::styled(detail, Style::default().fg(theme.text.secondary)),
    ]
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
        spans[index].content = crate::ui::ui_tool_paths::shorten_search_path(
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

pub(super) fn tool_header_width(width: u16) -> usize {
    usize::from(transcript_surface_content_width(width, false))
        .saturating_sub(surface_prefix_width(TRANSCRIPT_ASSISTANT_BODY_PREFIX))
}

pub(super) fn unfinished_edit_header(header: &TranscriptToolCallHeader) -> bool {
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
    if crate::ui::ui_tool_titles::generic_tool_id(tool_id) {
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

pub(super) fn tool_call_marker_style(
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
            crate::ui::ui_transcript_style::blend_color(theme.surface.shell, theme.text.accent, 0.5)
        } else {
            theme.text.accent
        };
        crate::ui::ui_transcript_surface::tool_rail_motion_color(
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
                    .starts_with("background-notification:")
                    || (tool_call.header.visual_style
                        == TranscriptToolCallVisualStyle::TaskInline
                        && tool_call.header.title.starts_with("Subagent completed")) =>
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
                    || crate::ui::ui_tool_titles::generic_tool_id(&tool_call.header.tool_id))
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
                && (tool_call
                    .tool_call_id
                    .starts_with("background-notification:")
                    || tool_call.header.title.starts_with("Subagent completed")
                    || tool_call.header.title.starts_with("Subagent cancelled"))));
    let color = if dim_terminal
        && matches!(
            tool_call.header.presentation.status,
            ToolCallPresentationStatus::Succeeded
                | ToolCallPresentationStatus::Failed
                | ToolCallPresentationStatus::Cancelled
        ) {
        crate::ui::ui_transcript_style::blend_color(theme.surface.shell, color, 0.5)
    } else {
        color
    };
    tool_call_header_style(tool_call.header.struck_out, color)
}
pub(super) fn tool_header_rows(
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
    crate::ui::ui_tool_wrapping::words(
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
