use super::*;
use header::{
    build_tool_header_spans, completed_tool_marker, tool_call_marker_style, tool_header_rows,
    tool_header_width, unfinished_edit_header,
};

#[path = "ui_transcript_tool_render/details.rs"]
mod details;
#[path = "ui_transcript_tool_render/errors.rs"]
mod errors;
#[path = "ui_transcript_tool_render/eval.rs"]
mod eval;
#[path = "ui_transcript_tool_render/header.rs"]
mod header;
#[path = "ui_transcript_tool_render/shell.rs"]
mod shell;
#[cfg(test)]
#[path = "ui_transcript_tool_render/tests.rs"]
mod tests;

pub(super) use errors::append_assistant_error_box;

struct ToolPainter<'a> {
    theme: &'a Theme,
    width: u16,
    surface: Color,
    render: ToolSectionRender,
}

pub(super) fn append_tool_call_section_lines(
    tool: &TranscriptToolCallSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> ToolSectionRender {
    let mut painter = ToolPainter {
        theme,
        width,
        surface: base_surface,
        render: ToolSectionRender {
            lines: Vec::new(),
            interaction_rows: Vec::new(),
            diff_hunk_offsets: Vec::new(),
        },
    };
    match tool.header.visual_style {
        TranscriptToolCallVisualStyle::TaskInline => painter.task(tool),
        TranscriptToolCallVisualStyle::Block if shell_tool_uses_harness_bash_card(tool) => {
            painter.shell(tool)
        }
        _ => painter.ordinary(tool),
    }
    let mut render = painter.render;
    super::ui_transcript_tool_hooks::append(&mut render, tool, theme, width, base_surface);
    paint_tool_completion_rail(&mut render.lines, tool, theme);
    if transcript_target_is_hovered(
        coalesced_tool_header_target(tool).as_ref(),
        tool.hovered_target.as_ref(),
    ) {
        if let Some(header) = render.lines.first_mut() {
            apply_header_hover(header, tool.expanded, theme);
        }
    }
    render
}

impl ToolPainter<'_> {
    fn row(
        &mut self,
        indent: &str,
        spans: Vec<Span<'static>>,
        target: Option<TranscriptMouseTarget>,
    ) {
        append_surface_row_with_target(
            &mut self.render.lines,
            &mut self.render.interaction_rows,
            target,
            indent,
            self.surface,
            spans,
            transcript_surface_content_width(self.width, false),
        );
    }

    fn bounded_row(
        &mut self,
        indent: &str,
        spans: Vec<Span<'static>>,
        target: Option<TranscriptMouseTarget>,
    ) {
        append_surface_row_with_bounded_target(
            &mut self.render.lines,
            &mut self.render.interaction_rows,
            target,
            indent,
            self.surface,
            spans,
            transcript_surface_content_width(self.width, false),
        );
    }

    fn header_rows(
        &mut self,
        rows: Vec<Vec<Span<'static>>>,
        hanging: &str,
        target: Option<TranscriptMouseTarget>,
    ) {
        let continuation = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}{hanging}");
        for (index, row) in rows.into_iter().enumerate() {
            self.row(
                if index == 0 {
                    TRANSCRIPT_ASSISTANT_BODY_PREFIX
                } else {
                    &continuation
                },
                row,
                target.clone(),
            );
        }
    }

    fn ordinary(&mut self, tool: &TranscriptToolCallSection) {
        let fg = if (tool.details_visible() && tool.has_detail_content())
            || unfinished_edit_header(&tool.header)
        {
            self.theme.text.primary
        } else {
            self.theme.text.secondary
        };
        let style = tool_call_header_style(tool.header.struck_out, fg);
        let rows = tool_header_rows(
            tool,
            self.theme,
            style,
            tool_call_marker_style(tool, self.theme, fg),
            self.width,
        );
        self.header_rows(rows, "  ", coalesced_tool_header_target(tool));
        if tool.details_visible() {
            for detail in &tool.detail_blocks {
                self.tool_detail(detail, tool);
            }
        }
    }

    fn task(&mut self, tool: &TranscriptToolCallSection) {
        let target = subagent_session_target(tool.child_session_id.as_deref())
            .or_else(|| tool_header_target(&tool.tool_call_id, true));
        let theme = self.theme;
        let style = tool_call_header_style(tool.header.struck_out, theme.text.secondary);
        let spans = build_tool_header_spans(
            &tool.header,
            theme,
            style,
            tool_call_marker_style(tool, theme, theme.text.secondary),
            tool_header_width(self.width),
        );
        self.bounded_row(TRANSCRIPT_ASSISTANT_BODY_PREFIX, spans, target.clone());
        // Keep the agent and model visible when they do not fit beside the task.
        if let Some(subtitle) = tool.header.subtitle.as_deref().filter(|subtitle| {
            let label = tool.header.title.split('“').next().unwrap_or_default();
            display_width(label.trim_end()) + display_width(subtitle) + 6
                >= tool_header_width(self.width)
        }) {
            let text = crate::ui::ui_tool_output::safe_tool_text(subtitle);
            for spans in crate::ui::ui_tool_wrapping::words(
                vec![Span::styled(
                    collapse_inline_whitespace(&text),
                    muted_meta_style(theme),
                )],
                tool_header_width(self.width).saturating_sub(2),
            ) {
                self.bounded_row(
                    &format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  "),
                    spans,
                    target.clone(),
                );
            }
        }
        if !tool.details_visible() {
            return;
        }
        for detail in &tool.detail_blocks {
            let TranscriptToolCallDetailBlock::Message { text, tone } = detail else {
                self.tool_detail(detail, tool);
                continue;
            };
            let style = if *tone == TranscriptToolCallDetailTone::Error {
                Style::default().fg(theme.status.error)
            } else {
                style
            };
            for row in text.split('\n') {
                let spans = if row.is_empty() {
                    Vec::new()
                } else {
                    vec![Span::styled(row.to_string(), style)]
                };
                self.bounded_row(TRANSCRIPT_TOOL_BODY_PREFIX, spans, target.clone());
            }
        }
    }

    fn tool_detail(
        &mut self,
        detail: &TranscriptToolCallDetailBlock,
        tool: &TranscriptToolCallSection,
    ) {
        self.detail(
            detail,
            &tool.header.tool_id,
            tool.expanded,
            tool.header.path_metadata.as_deref(),
        );
    }
}

fn coalesced_tool_header_target(tool: &TranscriptToolCallSection) -> Option<TranscriptMouseTarget> {
    tool_header_target(
        &tool.tool_call_id,
        tool.header.disclosure_state.is_some() || tool.details_collapsed_by_default,
    )
}

fn shell_tool_uses_harness_bash_card(tool: &TranscriptToolCallSection) -> bool {
    matches!(tool.header.tool_id.as_str(), "shell.run" | "bash")
}

pub(super) fn apply_header_hover(line: &mut Line<'static>, expanded: bool, theme: &Theme) {
    if expanded {
        return;
    }
    let caret = if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
        ">"
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
