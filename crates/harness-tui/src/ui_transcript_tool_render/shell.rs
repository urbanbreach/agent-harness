use super::*;

impl ToolPainter<'_> {
    pub(super) fn shell(&mut self, tool_call: &TranscriptToolCallSection) {
        let theme = self.theme;
        let width = self.width;
        let bash_header =
            tool_call
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
        let title_style = tool_call_header_style(
            tool_call.header.struck_out,
            if tool_call.header.selected
                || (tool_call.details_visible() && tool_call.has_detail_content())
            {
                theme.text.primary
            } else {
                theme.text.secondary
            },
        );
        let marker_style =
            tool_call_marker_style(tool_call, theme, title_style.fg.unwrap_or_default());
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
            let marker = completed_tool_marker(tool_call.header.presentation.status, theme);
            let mut rows = vec![vec![
                Span::styled(format!("{marker} "), marker_style),
                Span::styled("Run ", title_style.add_modifier(Modifier::BOLD)),
            ]];
            let prefix_width = rows[0].iter().map(Span::width).sum::<usize>();
            let description =
                collapse_inline_whitespace(&crate::ui::ui_tool_output::safe_tool_text(description));
            for (index, mut row) in crate::ui::ui_tool_wrapping::words(
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

            vec![build_tool_header_spans(
                &header,
                theme,
                title_style,
                marker_style,
                tool_header_width(width),
            )]
        };
        let header_target = tool_header_target(
            &tool_call.tool_call_id,
            tool_call.header.disclosure_state.is_some(),
        );
        // Keep the hanging indent in the surface prefix so wrapping does not trim it.
        self.header_rows(header_rows, "    ", header_target.clone());
        if !tool_call.details_visible() {
            return;
        }
        for detail in &tool_call.detail_blocks {
            if let TranscriptToolCallDetailBlock::BashPanel {
                command,
                output,
                description,
            } = detail
            {
                let start = self.render.lines.len();
                append_harness_bash_panel(
                    &mut self.render.lines,
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
                    self.surface,
                );
                let hint = if tool_call.expanded {
                    "Click to collapse"
                } else {
                    "Click to expand"
                };
                for line in &self.render.lines[start..] {
                    let target = header_target
                        .clone()
                        .filter(|_| line.spans.iter().any(|span| span.content.contains(hint)));
                    self.render
                        .interaction_rows
                        .push(bounded_interaction_row(target, line));
                }
            } else {
                self.tool_detail(detail, tool_call);
            }
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
    let command = crate::ui::ui_tool_output::safe_tool_text(command);
    let command = if tool.details_visible() {
        command
    } else {
        truncate_plain_text(
            &collapse_inline_whitespace(&command),
            tool_header_width(width).saturating_sub(6),
        )
    };
    let highlighted = crate::ui::ui_syntax_highlight::render_highlighted_code_block(
        Some("bash"),
        &command,
        &command,
        "",
        theme.text.primary,
        theme,
    );
    let rows =
        crate::ui::ui_tool_wrapping::shell(highlighted, tool_header_width(width).saturating_sub(6));
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
                crate::ui::ui_tool_wrapping::clip(
                    row,
                    tool_header_width(width).saturating_sub(if index == 0 { 0 } else { 4 }),
                )
            })
            .collect(),
    )
}
