use super::*;

pub(in crate::ui) fn selection_rows_for_markdownish_text_block(
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) -> Vec<SelectionRow> {
    let base_style = Style::default().fg(color);
    let display_source = markdown_display_source(text);
    let source_rows = display_source.lines().collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut index = 0;

    while let Some(line) = source_rows.get(index).copied() {
        if line.is_empty() && rows.last().is_some_and(selection_row_is_blank) {
            index += 1;
            continue;
        }
        if let Some((table_lines, consumed, table_links)) =
            try_render_markdown_table_block(&source_rows[index..], color, prefix, theme, width)
        {
            rows.extend(selection_rows_for_rendered_table_lines(
                table_lines,
                width,
                display_width(prefix),
                &table_links,
            ));
            index += consumed;
            continue;
        }

        rows.extend(selection_rows_for_markdownish_line(
            line, color, prefix, base_style, theme, width,
        ));
        index += 1;
    }

    if text.is_empty() {
        rows.extend(selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            Vec::new(),
            width,
            display_width(prefix),
        ));
    }

    rows
}

pub(in crate::ui) fn selection_rows_for_rich_text_block(
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
    is_streaming: bool,
) -> Option<Vec<SelectionRow>> {
    let Some(blocks) = (text.contains("```") || text.contains("~~~"))
        .then(|| {
            if is_streaming {
                Some(parse_streaming_fenced_text_blocks(text))
            } else {
                parse_fenced_text_blocks(text)
            }
        })
        .flatten()
    else {
        return Some(selection_rows_for_markdownish_text_block(
            text, color, prefix, theme, width,
        ));
    };

    let base_style = Style::default().fg(color);
    let copy_offset = display_width(prefix);
    let mut rows = Vec::new();
    for block in blocks {
        match block {
            ParsedTextBlock::Plain(plain) => {
                let mut plain_rows =
                    selection_rows_for_markdownish_text_block(&plain, color, prefix, theme, width);
                if rows.last().is_some_and(selection_row_is_blank)
                    && plain_rows.first().is_some_and(selection_row_is_blank)
                {
                    plain_rows.remove(0);
                }
                rows.extend(plain_rows);
                if rows.last().is_some_and(|row| !selection_row_is_blank(row)) {
                    rows.push(blank_selection_row());
                }
            }
            ParsedTextBlock::Code { language, body, .. } => {
                if is_mermaid_language(language.as_deref())
                    || matches!(language.as_deref(), Some("diff" | "patch"))
                {
                    return None;
                }
                if rows.last().is_some_and(|row| !selection_row_is_blank(row)) {
                    rows.push(blank_selection_row());
                }
                for line in body.lines() {
                    rows.extend(selection_rows_for_preformatted_line(
                        line,
                        prefix,
                        base_style,
                        width,
                        copy_offset,
                    ));
                }
                rows.push(blank_selection_row());
            }
        }
    }
    Some(rows)
}

fn selection_rows_for_preformatted_line(
    line: &str,
    prefix: &str,
    style: Style,
    width: u16,
    copy_offset: usize,
) -> Vec<SelectionRow> {
    let source_spans = vec![Span::styled(line.to_string(), style)];
    let expanded =
        crate::ui::ui_transcript_surface::wrap_preformatted_spans(source_spans.clone(), usize::MAX)
            .into_iter()
            .flatten()
            .map(|span| span.content.into_owned())
            .collect::<String>();
    let wrapped = crate::ui::ui_transcript_surface::wrap_preformatted_spans(
        source_spans,
        usize::from(width).saturating_sub(copy_offset).max(1),
    );
    let mut consumed = 0;
    let mut trailing = String::new();
    let mut rows = Vec::new();
    for (index, spans) in wrapped.into_iter().enumerate() {
        let text = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let rest = &expanded[consumed..];
        let gap = rest.find(&text).unwrap_or_default();
        let joiner = format!("{trailing}{}", &rest[..gap]);
        consumed += gap + text.len();
        trailing = text[text.trim_end_matches(' ').len()..].to_string();
        let mut rendered = vec![Span::styled(prefix.to_string(), style)];
        rendered.extend(spans);
        let mut selected = selection_rows_for_rendered_line(&Line::from(rendered), width);
        for row in &mut selected {
            row.continues_previous |= index > 0;
            if row.has_content() {
                row.start_cell = copy_offset;
            }
            row.copy_joiner = Some(joiner.clone());
        }
        rows.extend(selected);
    }
    rows
}

fn selection_row_is_blank(row: &SelectionRow) -> bool {
    row.text.chars().all(char::is_whitespace)
}

fn selection_rows_for_rendered_table_lines(
    lines: Vec<Line<'static>>,
    width: u16,
    copy_offset: usize,
    links: &[TableLinkRun],
) -> Vec<SelectionRow> {
    lines
        .iter()
        .enumerate()
        .flat_map(|(line_index, line)| {
            selection_rows_for_rendered_line(line, width)
                .into_iter()
                .map(move |mut row| {
                    row.exclude_prefix(copy_offset);
                    if row.has_content() {
                        row.links = links
                            .iter()
                            .filter(|link| link.row == line_index)
                            .map(|link| TranscriptSelectionLink {
                                continues_previous: false,
                                start_cell: link.start_cell,
                                end_cell: link.end_cell,
                                destination: link.destination.clone(),
                            })
                            .collect();
                    }
                    row
                })
        })
        .collect()
}

fn selection_rows_for_markdownish_line(
    line: &str,
    color: Color,
    prefix: &str,
    base_style: Style,
    theme: &Theme,
    width: u16,
) -> Vec<SelectionRow> {
    if line.is_empty() {
        return selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            Vec::new(),
            width,
            display_width(prefix),
        );
    }

    let indent_width = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let indent = " ".repeat(indent_width);
    let trimmed = line.trim_start();
    let content_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);

    if let Some(text) = markdown_heading_text(trimmed) {
        return selection_rows_for_prefixed_wrapped_inline(
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown(
                text,
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                theme.text.accent,
                theme,
            ),
            width,
            display_width(prefix),
            theme.markdown_native,
        );
    }

    if markdown_rule(trimmed) {
        return selection_rows_for_prefixed_wrapped_spans(
            prefix,
            base_style,
            vec![Span::styled(
                "─".repeat(content_width),
                Style::default().fg(theme.text.secondary),
            )],
            width,
            display_width(prefix),
        );
    }

    if let Some((depth, text)) = markdown_quote_prefix(trimmed) {
        let quote_prefix = format!("{prefix}{indent}{}", "│ ".repeat(depth));
        return selection_rows_for_prefixed_wrapped_inline(
            &quote_prefix,
            Style::default().fg(theme.markdown.block_quote),
            parse_inline_markdown(
                text,
                Style::default().fg(theme.markdown.block_quote),
                theme.markdown.block_quote,
                theme,
            ),
            width,
            display_width(&quote_prefix),
            theme.markdown_native,
        );
    }

    if let Some((list_prefix, text, list_style, text_style)) = markdown_list_prefix(trimmed, theme)
    {
        return selection_rows_for_prefixed_wrapped_inline(
            &format!("{prefix}{indent}{list_prefix}"),
            list_style,
            parse_inline_markdown(text, text_style, color, theme),
            width,
            display_width(prefix),
            theme.markdown_native,
        );
    }

    selection_rows_for_prefixed_wrapped_inline(
        prefix,
        base_style,
        parse_inline_markdown(trimmed, base_style, color, theme),
        width,
        display_width(prefix),
        theme.markdown_native,
    )
}

fn selection_rows_for_prefixed_wrapped_inline(
    prefix: &str,
    prefix_style: Style,
    parsed: ParsedInlineMarkdown,
    width: u16,
    copy_offset: usize,
    native: bool,
) -> Vec<SelectionRow> {
    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let source = parsed
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let mut consumed = 0;
    let source_links = parsed
        .links
        .into_iter()
        .map(|link| SurfaceLinkRun {
            continues_previous: false,
            start_cell: link.start_cell,
            end_cell: link.end_cell,
            destination: link.destination,
        })
        .collect::<Vec<_>>();
    wrap_surface_spans_with_links(parsed.spans, &source_links, content_width, native)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let text = row
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            let start = consumed + source[consumed..].find(&text).unwrap_or_default();
            let joiner = source[consumed..start].to_owned();
            consumed = start + text.len();
            let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
            spans.extend(row.spans);
            let mut selected = selection_rows_for_rendered_line(&Line::from(spans), width)
                .into_iter()
                .next()
                .unwrap_or_else(blank_selection_row);
            selected.continues_previous = index > 0;
            if native {
                selected.copy_joiner = Some(joiner);
            }
            selected.exclude_prefix(copy_offset);
            if selected.has_content() {
                selected.links = row
                    .links
                    .into_iter()
                    .map(|link| TranscriptSelectionLink {
                        continues_previous: link.continues_previous,
                        start_cell: prefix_width.saturating_add(link.start_cell),
                        end_cell: prefix_width.saturating_add(link.end_cell),
                        destination: link.destination,
                    })
                    .collect();
            }
            selected
        })
        .collect()
}

fn selection_rows_for_prefixed_wrapped_spans(
    prefix: &str,
    prefix_style: Style,
    content_spans: Vec<Span<'static>>,
    width: u16,
    copy_offset: usize,
) -> Vec<SelectionRow> {
    let rendered_lines = if content_spans.is_empty() {
        vec![Line::from(Span::styled(prefix.to_string(), prefix_style))]
    } else {
        let prefix_width = display_width(prefix);
        let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
        wrap_surface_spans(content_spans, content_width)
            .into_iter()
            .map(|row| {
                let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
                spans.extend(row);
                Line::from(spans)
            })
            .collect::<Vec<_>>()
    };

    rendered_lines
        .into_iter()
        .enumerate()
        .map(|(idx, row)| {
            let mut selected = selection_rows_for_rendered_line(&row, width)
                .into_iter()
                .next()
                .unwrap_or_else(blank_selection_row);
            selected.continues_previous = idx > 0;
            selected.exclude_prefix(copy_offset);
            selected
        })
        .collect()
}
