use super::*;
use crate::ui::ui_transcript_surface::wrap_preformatted_spans;

pub(super) fn header(
    header: &TranscriptToolCallHeader,
    theme: &Theme,
    title_style: Style,
    marker_style: Style,
    width: usize,
) -> Vec<Span<'static>> {
    let mut metadata = header.subtitle.as_deref().unwrap_or("Eval").split(" · ");
    let language = metadata.next().unwrap_or("Eval");
    let language = if width < 60 {
        match language {
            "JavaScript" => "JS",
            "Python" => "Py",
            "Ruby" => "Rb",
            "Julia" => "Jl",
            other => other,
        }
    } else {
        language
    };
    let mut essential = language.to_owned();
    let mut optional = Vec::new();
    for part in metadata {
        if matches!(
            part,
            "writing"
                | "running"
                | "queued"
                | "detached"
                | "approval needed"
                | "cancelled"
                | "Failed"
        ) {
            essential.push_str(" · ");
            essential.push_str(part);
        } else {
            optional.push(part);
        }
    }
    let optional = if width >= 70 && !optional.is_empty() {
        format!(" · {}", optional.join(" · "))
    } else {
        String::new()
    };
    let reserve = display_width(&essential) + display_width(&optional) + 5;
    let title = truncate_plain_text(
        &crate::ui::ui_tool_output::safe_tool_text(&header.title),
        width.saturating_sub(reserve),
    );
    let (label, rest) = title.split_once(' ').unwrap_or((&title, ""));
    crate::ui::ui_tool_wrapping::clip(
        vec![
            Span::styled(
                format!(
                    "{} ",
                    completed_tool_marker(header.presentation.status, theme)
                ),
                marker_style,
            ),
            Span::styled(label.to_owned(), title_style.add_modifier(Modifier::BOLD)),
            Span::styled(
                if rest.is_empty() {
                    String::new()
                } else {
                    format!(" {rest}")
                },
                title_style,
            ),
            Span::styled(" · ", muted_meta_style(theme)),
            Span::styled(essential, Style::default().fg(theme.text.primary)),
            Span::styled(optional, muted_meta_style(theme)),
        ],
        width,
    )
}

impl ToolPainter<'_> {
    pub(super) fn input(&mut self, code: &str, language: &str, expanded: bool) {
        if code.is_empty() {
            return;
        }
        let theme = self.theme;
        let width = usize::from(transcript_surface_content_width(self.width, false))
            .saturating_sub(surface_prefix_width(TRANSCRIPT_TOOL_BODY_PREFIX) + 2)
            .max(1);
        let code = crate::ui::ui_tool_output::safe_tool_text(code);
        let language = match language {
            "js" => "javascript",
            "py" => "python",
            "rb" => "ruby",
            "jl" => "julia",
            other => other,
        };
        let highlighted = crate::ui::ui_syntax_highlight::render_highlighted_code_block(
            Some(language),
            &code,
            &code,
            "",
            theme.text.primary,
            theme,
        );
        // One extra logical line retains the omission marker. Only the visible
        // tail needs display-cell wrapping; syntax state still covers the source.
        let start = if expanded {
            0
        } else {
            highlighted.len().saturating_sub(6)
        };
        let rows = highlighted
            .into_iter()
            .skip(start)
            .flat_map(|line| {
                wrap_preformatted_spans(line.spans, width)
                    .into_iter()
                    .map(Line::from)
            })
            .collect();
        let rows = crate::ui::ui_tool_output::measured_output_preview(
            rows,
            (0, 5),
            expanded,
            Style::default().fg(theme.text.secondary),
        );
        self.prebuilt(
            TRANSCRIPT_TOOL_BODY_PREFIX,
            theme.markdown.code_background,
            with_gutter(
                rows,
                if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
                    "< "
                } else {
                    "← "
                },
                theme.text.accent,
            ),
        );
    }

    pub(super) fn eval(
        &mut self,
        code: &str,
        language: &str,
        output: &str,
        failed: bool,
        expanded: bool,
    ) {
        self.input(code, language, expanded);
        let theme = self.theme;
        let width = usize::from(transcript_surface_content_width(self.width, false))
            .saturating_sub(surface_prefix_width(TRANSCRIPT_TOOL_BODY_PREFIX) + 2)
            .max(1);
        let ascii = theme.glyph_mode() == crate::theme::GlyphMode::Ascii;
        if !output.is_empty() && (expanded || failed) {
            let output = crate::ui::ui_tool_output::safe_tool_text(output);
            if !code.is_empty() {
                self.render.lines.push(Line::default());
            }
            let color = if failed {
                theme.status.error
            } else {
                theme.text.primary
            };
            let rows =
                crate::ui::ui_terminal_output::render(&output, Style::default().fg(color), theme)
                    .into_iter()
                    .flat_map(|line| {
                        wrap_preformatted_spans(line.spans, width)
                            .into_iter()
                            .map(Line::from)
                    })
                    .collect();
            let rows = crate::ui::ui_tool_output::measured_output_preview(
                rows,
                (5, 3),
                expanded,
                Style::default().fg(theme.text.secondary),
            );
            self.prebuilt(
                TRANSCRIPT_TOOL_BODY_PREFIX,
                self.surface,
                with_gutter(
                    rows,
                    if ascii { "> " } else { "→ " },
                    if failed {
                        theme.status.error
                    } else {
                        theme.text.secondary
                    },
                ),
            );
        }
    }
}

fn with_gutter(rows: Vec<Line<'static>>, marker: &'static str, color: Color) -> Vec<Line<'static>> {
    rows.into_iter()
        .enumerate()
        .map(|(index, mut line)| {
            line.spans.insert(
                0,
                Span::styled(
                    if index == 0 { marker } else { "  " },
                    Style::default().fg(color),
                ),
            );
            line
        })
        .collect()
}
