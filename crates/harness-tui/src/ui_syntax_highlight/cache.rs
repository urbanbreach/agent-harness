//! Resume at the last complete line of a growing fence. Exact repeats also
//! reuse the final partial line; non-append edits rebuild from a matching prefix.
use std::cell::RefCell;

use syntect::{
    easy::HighlightLines,
    highlighting::{HighlightState, Style, Theme},
    parsing::{ParseState, SyntaxReference, SyntaxSet},
};

type HighlightedLine = Vec<(Style, String)>;
const MAX_SOURCE_BYTES: usize = 256 * 1024;
const MAX_BLOCKS: usize = 8;

thread_local! {
    static CACHE: RefCell<Vec<CachedBlock>> = const { RefCell::new(Vec::new()) };
    #[cfg(test)]
    static PARSED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct CachedBlock {
    syntax: String,
    dark: bool,
    source: String,
    committed_bytes: usize,
    committed: Vec<HighlightedLine>,
    rendered: Vec<HighlightedLine>,
    highlight_state: HighlightState,
    parse_state: ParseState,
}

pub(super) fn highlight(
    syntax: &SyntaxReference,
    syntaxes: &SyntaxSet,
    palette: &Theme,
    dark: bool,
    body: &str,
) -> Option<Vec<HighlightedLine>> {
    CACHE.with_borrow_mut(|cache| {
        let cached = cache
            .iter()
            .rposition(|entry| {
                entry.syntax == syntax.name
                    && entry.dark == dark
                    && body.starts_with(&entry.source[..entry.committed_bytes])
            })
            .map(|index| cache.remove(index));
        if let Some(entry) = cached.as_ref().filter(|entry| entry.source == body) {
            let rendered = entry.rendered.clone();
            cache.extend(cached);
            return Some(rendered);
        }
        let (mut highlighter, mut committed, start) = if let Some(entry) = cached {
            (
                HighlightLines::from_state(palette, entry.highlight_state, entry.parse_state),
                entry.committed,
                entry.committed_bytes,
            )
        } else {
            (HighlightLines::new(syntax, palette), Vec::new(), 0)
        };
        let committed_bytes = body.rfind('\n').map_or(0, |index| index + 1);
        for line in body[start..committed_bytes].split_inclusive('\n') {
            committed.push(highlight_line(&mut highlighter, line, syntaxes)?);
        }
        let (highlight_state, parse_state) = highlighter.state();
        let mut rendered = committed.clone();
        if committed_bytes < body.len() {
            let mut tail =
                HighlightLines::from_state(palette, highlight_state.clone(), parse_state.clone());
            rendered.push(highlight_line(
                &mut tail,
                &format!("{}\n", &body[committed_bytes..]),
                syntaxes,
            )?);
        }
        if body.len() <= MAX_SOURCE_BYTES {
            while !cache.is_empty()
                && (cache.len() >= MAX_BLOCKS
                    || cache.iter().map(|entry| entry.source.len()).sum::<usize>() + body.len()
                        > MAX_SOURCE_BYTES)
            {
                cache.remove(0);
            }
            cache.push(CachedBlock {
                syntax: syntax.name.clone(),
                dark,
                source: body.to_string(),
                committed_bytes,
                committed,
                rendered: rendered.clone(),
                highlight_state,
                parse_state,
            });
        }
        Some(rendered)
    })
}

fn highlight_line(
    highlighter: &mut HighlightLines<'_>,
    line: &str,
    syntaxes: &SyntaxSet,
) -> Option<HighlightedLine> {
    #[cfg(test)]
    PARSED_LINES.set(PARSED_LINES.get() + 1);
    Some(
        highlighter
            .highlight_line(line, syntaxes)
            .ok()?
            .into_iter()
            .map(|(style, text)| (style, text.trim_end_matches(['\r', '\n']).to_string()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn streaming_reuses_committed_lines_and_invalidates_edits_and_palette_changes() {
        let syntaxes = super::super::syntax_highlight_assets();
        let syntax = syntaxes
            .syntax_set
            .find_syntax_by_token("rust")
            .unwrap_or_else(|| syntaxes.syntax_set.find_syntax_plain_text());
        CACHE.with_borrow_mut(Vec::clear);
        PARSED_LINES.set(0);
        let mut source = String::new();
        let theme = crate::theme::Theme::default();
        let palette = super::super::syntax_theme(&theme).unwrap_or_abort();
        for _ in 0..30 {
            source.push_str("let value = 42;\n");
            assert!(highlight(syntax, &syntaxes.syntax_set, palette, true, &source).is_some());
        }
        assert_eq!(
            PARSED_LINES.get(),
            30,
            "streaming must not reparse committed rows"
        );
        for (body, theme) in [
            ("let text = r#\"\ninside", theme),
            ("let text = r#\"\ninside\n\"#;", theme),
            ("// replaced\nlet value = 1;", theme),
            (
                "// replaced\nlet value = 1;",
                crate::theme::Theme::harness_light(),
            ),
        ] {
            let palette = super::super::syntax_theme(&theme).unwrap_or_abort();
            let cached = highlight(syntax, &syntaxes.syntax_set, palette, theme.is_dark(), body);
            CACHE.with_borrow_mut(Vec::clear);
            let fresh = highlight(syntax, &syntaxes.syntax_set, palette, theme.is_dark(), body);
            assert_eq!(cached, fresh);
        }
    }
}
