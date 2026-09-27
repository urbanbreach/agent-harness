use super::*;

impl ToolPainter<'_> {
    pub(super) fn detail(
        &mut self,
        detail: &TranscriptToolCallDetailBlock,
        tool_id: &str,
        expanded: bool,
        path: Option<&str>,
    ) {
        let start = self.render.lines.len();
        let theme = self.theme;
        match detail {
            TranscriptToolCallDetailBlock::Recorded(output) => {
                let width = transcript_surface_content_width(self.width, false)
                    .saturating_sub(
                        u16::try_from(surface_prefix_width(TRANSCRIPT_TOOL_BODY_PREFIX))
                            .unwrap_or(u16::MAX),
                    )
                    .max(1);
                self.prebuilt(
                    TRANSCRIPT_TOOL_BODY_PREFIX,
                    self.surface,
                    output.lines(theme, width, expanded),
                );
            }
            TranscriptToolCallDetailBlock::ReadOutput { text, start_line } => {
                self.read(text, *start_line, path, expanded)
            }
            TranscriptToolCallDetailBlock::Message { text, tone } => {
                self.message(text, *tone, tool_id)
            }
            TranscriptToolCallDetailBlock::BashPanel {
                command,
                output,
                description,
            } => {
                append_harness_bash_panel(
                    &mut self.render.lines,
                    HarnessBashPanel {
                        command,
                        output,
                        description: description.as_deref(),
                        expanded,
                    },
                    theme,
                    self.width,
                    self.surface,
                );
            }
            TranscriptToolCallDetailBlock::StructuredDiff {
                diff_content,
                before_source,
                fallback_path,
                force_stacked,
                plain_numbered,
                highlight_syntax,
                show_file_header,
            } => {
                let width = transcript_surface_content_width(self.width, false)
                    .saturating_sub(
                        u16::try_from(surface_prefix_width(TRANSCRIPT_NESTED_INDENT))
                            .unwrap_or(u16::MAX),
                    )
                    .max(1);
                if let Some((lines, offsets)) = crate::ui::ui_diff::render_diff_with_recorded_source(
                    diff_content,
                    fallback_path.as_deref(),
                    "",
                    width,
                    StructuredDiffRenderOptions {
                        force_stacked: *force_stacked,
                        plain_numbered: *plain_numbered,
                        highlight_intraline: false,
                        highlight_syntax: *highlight_syntax,
                        show_file_header: *show_file_header,
                        show_hunk_header: false,
                    },
                    theme,
                    before_source.as_deref(),
                ) {
                    if !lines.is_empty()
                        && self
                            .render
                            .lines
                            .last()
                            .is_some_and(|line| !line.spans.is_empty())
                    {
                        self.render.lines.push(Line::default());
                    }
                    let start = self.render.lines.len();
                    self.prebuilt(TRANSCRIPT_NESTED_INDENT, self.surface, lines);
                    self.render.diff_hunk_offsets.extend(
                        offsets
                            .into_iter()
                            .map(|offset| start.saturating_add(offset)),
                    );
                }
            }
            TranscriptToolCallDetailBlock::FileSection(file) => {
                self.file(file);
                return; // The file header and its children add their own interaction rows.
            }
        }
        append_noninteractive_rows(&self.render.lines, &mut self.render.interaction_rows, start);
    }

    fn prebuilt(&mut self, indent: &str, surface: Color, rows: Vec<Line<'static>>) {
        append_prebuilt_surface_lines(
            &mut self.render.lines,
            indent,
            surface,
            rows,
            transcript_surface_content_width(self.width, false),
        );
    }

    fn file(&mut self, file: &TranscriptToolCallFileSection) {
        let mut spans = vec![Span::styled(
            file.title.clone(),
            Style::default().fg(self.theme.text.primary),
        )];
        if let Some(subtitle) = &file.subtitle {
            spans.push(Span::styled(" · ", muted_meta_style(self.theme)));
            spans.push(Span::styled(subtitle.clone(), muted_meta_style(self.theme)));
        }
        self.row(
            TRANSCRIPT_TOOL_BODY_PREFIX,
            spans,
            Some(TranscriptMouseTarget::PatchFile {
                tool_call_id: file.tool_call_id.clone(),
                file_path: file.file_path.clone(),
            }),
        );
        if file.disclosure_state == TranscriptToolCallDisclosureState::Expanded {
            for detail in &file.detail_blocks {
                // File details use generic wrapping, full output, and no parent syntax hint.
                self.detail(detail, "", true, None);
            }
        }
    }

    fn message(&mut self, text: &str, tone: TranscriptToolCallDetailTone, tool_id: &str) {
        let style = match tone {
            TranscriptToolCallDetailTone::Primary => Style::default().fg(self.theme.text.primary),
            TranscriptToolCallDetailTone::Secondary => muted_meta_style(self.theme),
            TranscriptToolCallDetailTone::Error => Style::default().fg(self.theme.status.error),
        };
        if crate::ui::ui_tool_titles::generic_tool_id(tool_id) {
            let rows = text
                .split('\n')
                .flat_map(|line| {
                    crate::ui::ui_tool_wrapping::words(
                        vec![Span::styled(line.to_string(), style)],
                        tool_header_width(self.width).saturating_sub(2).max(20),
                    )
                    .into_iter()
                    .map(Line::from)
                })
                .collect();
            self.prebuilt(TRANSCRIPT_TOOL_BODY_PREFIX, self.surface, rows);
        } else {
            for row in text.split('\n') {
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
                append_surface_row(
                    &mut self.render.lines,
                    &indent,
                    self.surface,
                    spans,
                    transcript_surface_content_width(self.width, false),
                );
            }
        }
    }

    fn read(
        &mut self,
        text: &str,
        start_line: Option<u64>,
        language: Option<&str>,
        expanded: bool,
    ) {
        let theme = self.theme;
        let width = self.width;
        let text = crate::ui::ui_tool_output::safe_tool_text(text);
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
        let highlighted = crate::ui::ui_syntax_highlight::render_highlighted_code_block(
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
            source.extend(crate::ui::ui_transcript_surface::expand_preformatted_tabs(
                line.spans,
            ));
            let wrapped =
                crate::ui::ui_transcript_surface::wrap_preformatted_spans(source, content_width);
            rows.extend(wrapped.into_iter().map(Line::from));
        }
        let rows = crate::ui::ui_tool_output::measured_output_preview(
            rows,
            (5, 3),
            expanded,
            Style::default().fg(theme.text.secondary),
        );
        if !rows.is_empty() {
            self.render.lines.push(Line::default());
        }
        append_prebuilt_surface_lines(
            &mut self.render.lines,
            TRANSCRIPT_TOOL_BODY_PREFIX,
            theme.markdown.code_background,
            rows,
            transcript_surface_content_width(width, false),
        );
    }
}
