// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::*;

fn build_tool_header_spans(
    header: &TranscriptToolCallHeader,
    theme: &Theme,
    title_style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if let Some(icon) = header.icon {
        spans.push(Span::styled(format!("{icon} "), title_style));
    }
    spans.push(Span::styled(header.title.clone(), title_style));
    if let Some(subtitle) = header.subtitle.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(header.disclosure_state) {
        spans.push(Span::styled("  ", muted_meta_style(theme)));
        spans.push(Span::styled(disclosure, muted_meta_style(theme)));
    }
    spans
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

    let spans = build_tool_header_spans(&tool_call.header, theme, style);

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
    if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }

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
            TranscriptToolCallDetailBlock::Markdown { text } => {
                let start = render.lines.len();
                append_rich_text_block(
                    &mut render.lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_OPCODE_EDIT_INDENT,
                    theme,
                    transcript_surface_content_width(width, false),
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
    let card_shell = Some(TranscriptToolCardShell {
        indent: TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        rail_color: block_tool_rail_color(tool_call.header.status, theme),
        surface,
    });
    let title_style = tool_call_header_style(
        tool_call.header.struck_out,
        block_tool_color(tool_call.header.status, theme),
    );
    let header_target = tool_header_target(
        &tool_call.tool_call_id,
        tool_call.header.disclosure_state.is_some(),
    );

    let title_spans = build_tool_header_spans(&tool_call.header, theme, title_style);

    append_card_surface_row_with_target(
        &mut render.lines,
        &mut render.interaction_rows,
        header_target.clone(),
        card_shell,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        surface,
        title_spans,
        transcript_surface_content_width(width, false),
    );

    if let Some(path_metadata) = tool_call.header.path_metadata.as_deref() {
        let path_spans = vec![Span::styled(
            path_metadata.to_string(),
            muted_meta_style(theme),
        )];
        append_card_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            header_target,
            card_shell,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            surface,
            path_spans,
            transcript_surface_content_width(width, false),
        );
    }

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, card_shell);
}

pub(super) fn shell_tool_uses_harness_bash_card(tool_call: &TranscriptToolCallSection) -> bool {
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
            TranscriptToolCallDetailBlock::Markdown { text } => {
                append_rich_text_block(
                    &mut render.lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_OPCODE_EDIT_INDENT,
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

    append_card_surface_row_with_target(
        &mut render.lines,
        &mut render.interaction_rows,
        header_target,
        card_shell,
        TRANSCRIPT_OPCODE_EDIT_INDENT,
        base_surface,
        spans,
        transcript_surface_content_width(width, false),
    );

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
        TranscriptToolCallDetailTone::Secondary => muted_meta_style(theme),
        TranscriptToolCallDetailTone::Error => Style::default().fg(theme.status.error),
    };

    for row in text.split('\n') {
        let spans = if row.is_empty() {
            Vec::new()
        } else {
            vec![Span::styled(row.to_string(), style)]
        };
        append_card_surface_row(
            lines,
            card_shell,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            base_surface,
            spans,
            transcript_surface_content_width(width, false),
        );
    }
}

pub(super) fn append_assistant_error_box(
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
    if items.is_empty() {
        return;
    }
    let render_width = transcript_surface_content_width(width, false);
    let ordered = ordered_todo_items(items);

    if !lines.is_empty() {
        append_card_surface_row(
            lines,
            card_shell,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            base_surface,
            Vec::new(),
            render_width,
        );
    }

    for (index, item) in ordered.iter().enumerate() {
        if index > 0 {
            append_card_surface_row(
                lines,
                card_shell,
                TRANSCRIPT_OPCODE_EDIT_INDENT,
                base_surface,
                Vec::new(),
                render_width,
            );
        }
        let marker_style = item.status.style(theme);
        let content_style = item.status.content_style(theme);
        let spans = vec![
            Span::styled(format!("{} ", item.status.checkbox_glyph()), marker_style),
            Span::styled(item.content.clone(), content_style),
        ];
        append_card_surface_row(
            lines,
            card_shell,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            base_surface,
            spans,
            render_width,
        );
    }

    for _ in 0..2 {
        append_card_surface_row(
            lines,
            card_shell,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
            base_surface,
            Vec::new(),
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
        append_card_prebuilt_surface_lines(
            &mut render.lines,
            card_shell,
            TRANSCRIPT_OPCODE_EDIT_INDENT,
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
