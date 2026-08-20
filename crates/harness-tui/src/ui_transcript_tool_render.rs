// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::*;

use crate::app::{ToolCallPresentation, ToolCallPresentationStatus};

// Empty indent places the todo-block rail at column 0 (matching the user
// message box rail). One leading content space offsets the nested rail glyph
// and its trailing gap so todo text starts at column 3, matching user-message
// and assistant-body text.
const TRANSCRIPT_TODO_BLOCK_INDENT: &str = "   ";
const TRANSCRIPT_TODO_BLOCK_CONTENT_LEADING: &str = " ";

fn build_tool_header_spans(
    header: &TranscriptToolCallHeader,
    theme: &Theme,
    title_style: Style,
    marker_style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let marker = completed_tool_marker(header.presentation.status, theme);
    spans.push(Span::styled(format!("{marker} "), marker_style));
    let _ = header.icon;
    let compact_edit = matches!(header.tool_id.as_str(), "fs.write" | "write")
        && (header.title == "edit" || header.title.starts_with("edit "));
    let edit_style = title_style.fg(theme.text.primary);
    if compact_edit {
        spans.push(Span::styled(
            "edit",
            edit_style.add_modifier(Modifier::BOLD),
        ));
    } else if let Some(path) = header.title.strip_prefix("edit ") {
        spans.push(Span::styled(
            "edit ",
            edit_style.add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(path.to_string(), edit_style));
    } else if header.title == "edit" {
        spans.push(Span::styled(
            header.title.clone(),
            edit_style.add_modifier(Modifier::BOLD),
        ));
    } else {
        let title_style = if header.presentation.status == ToolCallPresentationStatus::Failed {
            title_style.add_modifier(Modifier::BOLD)
        } else {
            title_style
        };
        spans.push(Span::styled(header.title.clone(), title_style));
    }
    if let Some(path_metadata) = header.path_metadata.as_deref() {
        spans.push(Span::styled(" ", muted_meta_style(theme)));
        spans.push(Span::styled(
            path_metadata.to_string(),
            Style::default().fg(theme.text.accent),
        ));
    }
    if let Some(subtitle) = header.subtitle.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }
    if !compact_edit {
        if let Some(disclosure) = tool_header_disclosure_glyph(header.disclosure_state, theme) {
            spans.push(Span::styled("  ", muted_meta_style(theme)));
            spans.push(Span::styled(disclosure, muted_meta_style(theme)));
        }
    }
    spans
}

fn completed_tool_marker(status: ToolCallPresentationStatus, theme: &Theme) -> &'static str {
    match status {
        ToolCallPresentationStatus::Queued
        | ToolCallPresentationStatus::Running
        | ToolCallPresentationStatus::Waiting
        | ToolCallPresentationStatus::Succeeded
        | ToolCallPresentationStatus::Failed
        | ToolCallPresentationStatus::Cancelled => theme.live_shell.transcript_glyphs.tool_marker,
    }
}

fn tool_call_marker_style(
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    inactive_color: Color,
) -> Style {
    let edit = tool_call.header.title == "edit" || tool_call.header.title.starts_with("edit ");
    let color = if tool_call.header.presentation.status == ToolCallPresentationStatus::Running
        && matches!(tool_call.rail_motion, ToolRailMotion::Running { .. })
    {
        super::ui_transcript_surface::tool_rail_motion_color(
            theme.surface.shell,
            inactive_color,
            Some(tool_call.rail_motion),
            0,
            tool_call.animation_phase,
        )
    } else if edit {
        theme.reference_terminal.error
    } else {
        match tool_call.header.presentation.status {
            ToolCallPresentationStatus::Running => inactive_color,
            ToolCallPresentationStatus::Waiting => theme.status.warning,
            ToolCallPresentationStatus::Failed => theme.reference_terminal.error,
            ToolCallPresentationStatus::Cancelled => theme.status.disabled,
            ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Succeeded => {
                inactive_color
            }
        }
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
    render
}

fn append_inline_tool_section_lines(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) {
    let fg = inline_tool_color(tool_call.header.presentation.status, theme);
    let style = tool_call_header_style(tool_call.header.struck_out, fg);

    let marker_style = tool_call_marker_style(tool_call, theme, fg);
    let spans = build_tool_header_spans(&tool_call.header, theme, style, marker_style);

    append_surface_row_with_target(
        &mut render.lines,
        &mut render.interaction_rows,
        coalesced_tool_header_target(tool_call),
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        base_surface,
        spans,
        transcript_surface_content_width(width, false),
    );

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, None);
}

fn coalesced_tool_header_target(
    tool_call: &TranscriptToolCallSection,
) -> Option<TranscriptMouseTarget> {
    if tool_call.header.disclosure_state.is_none() {
        None
    } else if tool_call.coalesced_tool_call_ids.len() > 1 {
        Some(TranscriptMouseTarget::ToolGroup {
            tool_call_ids: tool_call.coalesced_tool_call_ids.clone(),
        })
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
    let hovered = transcript_target_is_hovered(target.as_ref(), tool_call.hovered_target.as_ref());
    let fg = task_inline_tool_color(
        tool_call.header.presentation.status,
        theme,
        hovered && target.is_some(),
    );
    let style = tool_call_header_style(tool_call.header.struck_out, fg);
    let surface = base_surface;
    let mut spans = Vec::new();
    spans.push(Span::styled("· ", muted_meta_style(theme)));
    let _ = tool_call.header.icon;
    spans.push(Span::styled(tool_call.header.title.clone(), style));
    if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(tool_call.header.disclosure_state, theme)
    {
        spans.push(Span::styled("  ", muted_meta_style(theme)));
        spans.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    append_surface_row_with_bounded_target(
        &mut render.lines,
        &mut render.interaction_rows,
        target.clone(),
        "     ",
        surface,
        spans,
        transcript_surface_content_width(width, false),
    );

    if !tool_call.details_visible() {
        append_collapsed_tool_error_summaries(render, tool_call, theme, width, base_surface, None);
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

    let is_todo_block = matches!(
        tool_call.header.tool_id.as_str(),
        "todo.write" | "todowrite"
    );
    let surface = base_surface;
    // Nested card shell is only for todo checklist cards. Non-todo Block tools
    // (write/edit with diffs) must stay flat like Thought / inline tools so
    // Creating titles share reference lead=5, not nested-rail lead=7.
    let card_shell = if is_todo_block && tool_call.details_visible() {
        Some(TranscriptToolCardShell {
            indent: TRANSCRIPT_TODO_BLOCK_INDENT,
            rail_color: theme.surface.shell,
            surface,
            content_leading_spaces: TRANSCRIPT_TODO_BLOCK_CONTENT_LEADING,
        })
    } else {
        None
    };
    let title_style = tool_call_header_style(
        tool_call.header.struck_out,
        if is_todo_block {
            theme.text.secondary
        } else {
            block_tool_color(tool_call.header.presentation.status, theme)
        },
    );
    let header_target = tool_header_target(
        &tool_call.tool_call_id,
        tool_call.header.disclosure_state.is_some(),
    );

    if is_todo_block && tool_call.details_visible() {
        append_card_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            None,
            card_shell,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            surface,
            Vec::new(),
            transcript_surface_content_width(width, false),
        );
    }

    let marker_style = tool_call_marker_style(tool_call, theme, title_style.fg.unwrap_or_default());
    let title_spans = build_tool_header_spans(&tool_call.header, theme, title_style, marker_style);

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

    append_tool_call_detail_blocks(render, tool_call, theme, width, base_surface, card_shell);

    if is_todo_block && tool_call.details_visible() {
        append_card_surface_row_with_target(
            &mut render.lines,
            &mut render.interaction_rows,
            None,
            card_shell,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            surface,
            Vec::new(),
            transcript_surface_content_width(width, false),
        );
    }
}

pub(super) fn tool_call_is_todo(tool_call: &TranscriptToolCallSection) -> bool {
    matches!(
        tool_call.header.tool_id.as_str(),
        "todo.write" | "todowrite"
    )
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
        header.title = format!("Run {}", command.trim());
        if header.subtitle.is_none() {
            header.subtitle = description
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned);
        }
    } else if !header.title.starts_with("Run ") {
        header.title = format!("Run {}", header.title.trim());
    }

    let title_style = tool_call_header_style(
        header.struck_out,
        block_tool_color(header.presentation.status, theme),
    );
    let marker_style = tool_call_marker_style(tool_call, theme, title_style.fg.unwrap_or_default());
    let title_spans = build_tool_header_spans(&header, theme, title_style, marker_style);
    let header_target =
        tool_header_target(&tool_call.tool_call_id, header.disclosure_state.is_some());
    append_card_surface_row_with_target(
        &mut render.lines,
        &mut render.interaction_rows,
        header_target,
        None,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        base_surface,
        title_spans,
        transcript_surface_content_width(width, false),
    );

    if !tool_call.details_visible() {
        append_collapsed_tool_error_summaries(render, tool_call, theme, width, base_surface, None);
        return;
    }

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
                        expand_hint: expand_hint.as_deref(),
                        tone: *tone,
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
                    base_surface,
                    None,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            _ => {
                append_tool_call_detail_blocks(
                    render,
                    &TranscriptToolCallSection {
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

pub(super) fn append_tool_call_detail_blocks(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    if !tool_call.details_visible() {
        append_collapsed_tool_error_summaries(
            render,
            tool_call,
            theme,
            width,
            base_surface,
            card_shell,
        );
        return;
    }

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
                    base_surface,
                );
                append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
            }
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                fallback_path,
                force_stacked,
                plain_numbered,
                highlight_syntax,
                show_file_header,
            } => {
                append_tool_call_diff_block(
                    render,
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
        tool_header_disclosure_glyph(Some(file_section.disclosure_state), theme)
            .unwrap_or(theme.live_shell.transcript_glyphs.disclosure_closed),
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
            coalesced_tool_call_ids: vec![file_section.tool_call_id.clone()],
            child_session_id: None,
            subagent_background: false,
            output_truncated: false,
            replay_read_only: false,
            hovered_target: None,
            header: TranscriptToolCallHeader {
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

fn append_collapsed_tool_error_summaries(
    render: &mut ToolSectionRender,
    tool_call: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    card_shell: Option<TranscriptToolCardShell>,
) {
    let mut rendered = std::collections::BTreeSet::new();
    for detail_block in &tool_call.detail_blocks {
        let text = match detail_block {
            TranscriptToolCallDetailBlock::Message {
                text,
                tone: TranscriptToolCallDetailTone::Error,
            }
            | TranscriptToolCallDetailBlock::BashPanel {
                output: text,
                tone: TranscriptToolCallDetailTone::Error,
                ..
            } => text,
            _ => continue,
        };
        let summary = text.trim();
        if summary.is_empty()
            || collapsed_failure_copy_is_redundant(summary)
            || !rendered.insert(summary.to_string())
        {
            continue;
        }
        let start = render.lines.len();
        append_tool_call_message_block(
            &mut render.lines,
            summary,
            TranscriptToolCallDetailTone::Error,
            theme,
            width,
            base_surface,
            card_shell,
        );
        append_noninteractive_rows(&render.lines, &mut render.interaction_rows, start);
    }
}

fn collapsed_failure_copy_is_redundant(text: &str) -> bool {
    let normalized = text.trim().trim_end_matches('.');
    normalized.eq_ignore_ascii_case("command failed")
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
        let indent = " ".repeat(row.len().saturating_sub(content.len()));
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
                    "",
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
            TRANSCRIPT_OPCODE_EDIT_INDENT,
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
    let body_indent = if plain_numbered {
        "     "
    } else {
        TRANSCRIPT_OPCODE_EDIT_INDENT
    };
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
    if let Some((diff_lines, hunk_offsets)) = render_structured_diff_lines_with_hunk_offsets(
        diff_content,
        fallback_path,
        "",
        content_width,
        StructuredDiffRenderOptions {
            force_stacked,
            plain_numbered,
            highlight_intraline: false,
            highlight_syntax: highlight_syntax && !plain_numbered,
            show_file_header,
            show_hunk_header: false,
        },
        theme,
    ) {
        // Reference permission state: blank packing row between Creating title and plain numbered body.
        // interaction_rows are padded by the caller via append_noninteractive_rows.
        let blank_before = plain_numbered && !diff_lines.is_empty();
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
    fn generic_tool_header_omits_terminal_count_and_timing_metadata() {
        // arrange
        let header = TranscriptToolCallHeader {
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
            disclosure_state: None,
        };

        // act
        let spans = build_tool_header_spans(
            &header,
            &Theme::default(),
            Style::default(),
            Style::default(),
        );
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        // assert
        assert!(rendered.contains("Edit src/main.rs"));
        assert!(!rendered.contains("7 results"), "{rendered:?}");
        assert!(!rendered.contains("1.2s"), "{rendered:?}");
    }
}
