//! Styled word wrapping borrows tokens and allocates only the resulting rows.
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::super::ui_chrome::display_width;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct SurfaceLinkRun {
    pub(in crate::ui) continues_previous: bool,
    pub(in crate::ui) start_cell: usize,
    pub(in crate::ui) end_cell: usize,
    pub(in crate::ui) destination: String,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct WrappedSurfaceRow {
    pub(in crate::ui) spans: Vec<Span<'static>>,
    pub(in crate::ui) links: Vec<SurfaceLinkRun>,
}

pub(in crate::ui) fn wrap_surface_spans_with_links(
    spans: Vec<Span<'static>>,
    links: &[SurfaceLinkRun],
    width: usize,
    native: bool,
) -> Vec<WrappedSurfaceRow> {
    let rows = if native {
        super::super::ui_tool_wrapping::words(spans.clone(), width)
    } else {
        wrap(&spans, width, false)
    };
    if links.is_empty() {
        return rows
            .into_iter()
            .map(|spans| WrappedSurfaceRow {
                spans,
                links: Vec::new(),
            })
            .collect();
    }
    let mut source = spans
        .iter()
        .flat_map(|span| span.content.graphemes(true))
        .scan(0usize, |cell, cluster| {
            let start = *cell;
            *cell += cluster.width();
            Some((cluster, start, *cell))
        })
        .peekable();
    rows.into_iter()
        .map(|spans| {
            let mut projected = Vec::<SurfaceLinkRun>::new();
            let mut output_cell = 0;
            for cluster in spans.iter().flat_map(|span| span.content.graphemes(true)) {
                while source.peek().is_some_and(|(text, _, _)| *text != cluster) {
                    source.next();
                }
                let Some((_, start, end)) = source.next() else {
                    break;
                };
                let output_end = output_cell + cluster.width();
                for link in links
                    .iter()
                    .filter(|link| link.start_cell < end && link.end_cell > start)
                {
                    if let Some(previous) = projected.last_mut().filter(|previous| {
                        previous.destination == link.destination && previous.end_cell == output_cell
                    }) {
                        previous.end_cell = output_end;
                    } else {
                        projected.push(SurfaceLinkRun {
                            continues_previous: link.start_cell < start,
                            start_cell: output_cell,
                            end_cell: output_end,
                            destination: link.destination.clone(),
                        });
                    }
                }
                output_cell = output_end;
            }
            WrappedSurfaceRow {
                spans,
                links: projected,
            }
        })
        .collect()
}

pub(in crate::ui) fn wrap_surface_spans(
    spans: Vec<Span<'static>>,
    width: usize,
) -> Vec<Vec<Span<'static>>> {
    wrap(&spans, width, false)
}

fn wrap(spans: &[Span<'static>], width: usize, preserve_indent: bool) -> Vec<Vec<Span<'static>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cells = 0usize;
    for (style, token) in spans
        .iter()
        .flat_map(|span| tokens(&span.content).map(move |token| (span.style, token)))
    {
        let whitespace = token.chars().all(char::is_whitespace);
        if whitespace && row.is_empty() && !(preserve_indent && rows.is_empty()) {
            continue;
        }
        let token_width = token.width();
        if cells.saturating_add(token_width) <= width {
            row.push(Span::styled(token.to_owned(), style));
            cells += token_width;
            continue;
        }
        if cells > 0 {
            rows.push(std::mem::take(&mut row));
        }
        cells = 0;
        if whitespace {
            continue;
        }
        let mut start = 0;
        if token_width > width {
            for (index, cluster) in token.grapheme_indices(true) {
                let cluster_width = cluster.width();
                if cells > 0 && cells.saturating_add(cluster_width) > width {
                    row.push(Span::styled(token[start..index].to_owned(), style));
                    rows.push(std::mem::take(&mut row));
                    start = index;
                    cells = 0;
                }
                cells += cluster_width;
            }
        } else {
            cells = token_width;
        }
        row.push(Span::styled(token[start..].to_owned(), style));
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

// Preserve whitespace runs and the reference's break after an alphanumeric hyphen.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    let mut graphemes = text.grapheme_indices(true).peekable();
    let mut previous = None;
    std::iter::from_fn(move || {
        let &(start, first) = graphemes.peek()?;
        let whitespace = first.chars().all(char::is_whitespace);
        let mut end = start;
        while let Some(&(index, cluster)) = graphemes.peek() {
            if cluster.chars().all(char::is_whitespace) != whitespace {
                break;
            }
            graphemes.next();
            end = index + cluster.len();
            let hyphen = cluster == "-"
                && previous.is_some_and(char::is_alphanumeric)
                && graphemes
                    .peek()
                    .and_then(|(_, next)| next.chars().next())
                    .is_some_and(char::is_alphanumeric);
            previous = cluster.chars().last();
            if hyphen {
                break;
            }
        }
        Some(&text[start..end])
    })
}

pub(in crate::ui) fn wrap_preformatted_spans(
    spans: Vec<Span<'static>>,
    width: usize,
) -> Vec<Vec<Span<'static>>> {
    let expanded = expand_preformatted_tabs(spans);
    let width = width.max(1);
    if expanded.iter().map(Span::width).sum::<usize>() <= width {
        return vec![expanded];
    }
    let mut rows = wrap(&expanded, width, true);
    if rows.is_empty() {
        rows.push(Vec::new());
    }
    rows
}

/// Tabs expand at source columns before wrapping, including across style spans.
pub(in crate::ui) fn expand_preformatted_tabs(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    if !spans.iter().any(|span| span.content.contains('\t')) {
        return spans;
    }
    let mut column = 0;
    spans
        .into_iter()
        .map(|span| {
            let mut text = String::new();
            for cluster in span.content.graphemes(true) {
                if cluster == "\t" {
                    let spaces = 4 - column % 4;
                    text.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                } else {
                    text.push_str(cluster);
                    column += display_width(cluster);
                }
            }
            Span::styled(text, span.style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_tokens_preserve_graphemes_and_cell_widths() {
        for (text, width, expected) in [
            ("abc界···xyz", 4, vec!["abc", "界··", "·xyz"]),
            ("e\u{301}x", 1, vec!["e\u{301}", "x"]),
            ("👨\u{200D}💻x", 2, vec!["👨\u{200D}💻", "x"]),
            ("🇫🇮x", 2, vec!["🇫🇮", "x"]),
            (" \u{301}x", 1, vec![" \u{301}", "x"]),
            ("\u{301}界", 1, vec!["\u{301}界"]),
        ] {
            let rows = wrap_surface_spans(vec![Span::raw(text.to_owned())], width)
                .into_iter()
                .map(|row| row.into_iter().map(|span| span.content).collect::<String>())
                .collect::<Vec<_>>();
            assert_eq!(rows, expected);
        }
        let mark = Span::styled("\u{301}", ratatui::style::Modifier::BOLD);
        assert_eq!(
            wrap_surface_spans(vec![mark.clone(), Span::raw("abcd")], 2),
            vec![vec![mark, Span::raw("ab")], vec![Span::raw("cd")]]
        );
    }
}
