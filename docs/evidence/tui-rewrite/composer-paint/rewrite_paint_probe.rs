use harness_tui::UnwrapOrAbort;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
// Only the scalar bounds are consumed by the extracted styling functions.
mod app {
    pub struct FileMentionTag {
        pub start: usize,
        pub end: usize,
    }
}
mod legacy {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    pub(crate) fn composer_line_with_file_tags(
        line: &str,
        line_start: usize,
        tags: &[crate::app::FileMentionTag],
        base_style: Style,
        tag_style: Style,
        selection: Option<std::ops::Range<usize>>,
    ) -> Line<'static> {
        if line.is_empty() {
            return Line::from(Span::styled(String::new(), base_style));
        }

        let mut spans = Vec::new();
        let mut current = String::new();
        let mut current_style = None;
        let mut char_index = line_start;
        for grapheme in line.graphemes(true) {
            let end = char_index + grapheme.chars().count();
            let mut style = if tags
                .iter()
                .any(|tag| char_index >= tag.start && char_index < tag.end)
            {
                tag_style
            } else {
                base_style
            };
            if selection
                .as_ref()
                .is_some_and(|range| char_index < range.end && end > range.start)
            {
                style = style.add_modifier(Modifier::REVERSED);
            }
            char_index = end;
            if current_style == Some(style) {
                current.push_str(grapheme);
            } else {
                if !current.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current),
                        current_style.unwrap_or_abort(),
                    ));
                }
                current_style = Some(style);
                current.push_str(grapheme);
            }
        }
        if !current.is_empty() {
            spans.push(Span::styled(current, current_style.unwrap_or(base_style)));
        }
        Line::from(spans)
    }
}
mod candidate {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    pub(crate) fn composer_line_with_file_tags<'a>(
        line: &'a str,
        line_start: usize,
        tags: &[crate::app::FileMentionTag],
        base_style: Style,
        tag_style: Style,
        selection: Option<std::ops::Range<usize>>,
    ) -> Line<'a> {
        let mut spans = Vec::new();
        let mut start = 0;
        let mut current_style = base_style;
        let mut char_index = line_start;
        for (byte, grapheme) in line.grapheme_indices(true) {
            let end = char_index + grapheme.chars().count();
            let mut style = if tags
                .iter()
                .any(|tag| char_index >= tag.start && char_index < tag.end)
            {
                tag_style
            } else {
                base_style
            };
            if selection
                .as_ref()
                .is_some_and(|range| char_index < range.end && end > range.start)
            {
                style = style.add_modifier(Modifier::REVERSED);
            }
            char_index = end;
            if byte > 0 && current_style != style {
                spans.push(Span::styled(&line[start..byte], current_style));
                start = byte;
            }
            current_style = style;
        }
        spans.push(Span::styled(&line[start..], current_style));
        Line::from(spans)
    }
}
#[test]
fn recorded_composer_style_contract() {
    let pieces = [
        "a",
        " ",
        "\t",
        "\n",
        "\r\n",
        "界",
        "e\u{301}",
        "👩‍💻",
        "\u{301}",
        "\u{200b}",
    ];
    let mut texts = vec![
        String::new(),
        "text ".repeat(100),
        "e\u{301}👩‍💻界".repeat(20),
        "@long/path/name\nnext".into(),
    ];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                texts.push(format!("{a}{b}{c}"));
            }
        }
    }
    let mut cases = 0;
    for text in texts {
        for start in [0, 4] {
            let n = text.chars().count();
            for bounds in [
                vec![],
                vec![(start, start + 1)],
                vec![(start + 1, start + 2)],
                vec![(start + 2, start + 4)],
                vec![(start, start + n)],
                vec![(start + 1, start + 4), (start + 2, start + 5)],
            ] {
                let tags: Vec<_> = bounds
                    .into_iter()
                    .map(|(start, end)| app::FileMentionTag { start, end })
                    .collect();
                for selection in [
                    None,
                    Some(start..start),
                    Some(start + 1..start + 1),
                    Some(start..start + n),
                    Some(start + 1..start + 2),
                    Some(start + 2..start + 4),
                    Some(start..start + n + 2),
                    Some(start + n + 1..start + n + 2),
                ] {
                    for (base, tag) in [
                        (
                            Style::default().fg(Color::White).bg(Color::Black),
                            Style::default()
                                .fg(Color::Yellow)
                                .bg(Color::Black)
                                .add_modifier(Modifier::BOLD),
                        ),
                        (Style::default(), Style::default()),
                        (
                            Style::default().add_modifier(Modifier::REVERSED),
                            Style::default()
                                .add_modifier(Modifier::BOLD)
                                .remove_modifier(Modifier::REVERSED),
                        ),
                    ] {
                        assert_eq!(
                            candidate::composer_line_with_file_tags(
                                &text,
                                start,
                                &tags,
                                base,
                                tag,
                                selection.clone()
                            ),
                            legacy::composer_line_with_file_tags(
                                &text,
                                start,
                                &tags,
                                base,
                                tag,
                                selection.clone()
                            ),
                            "text {text:?}, start {start}, selection {selection:?}"
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    eprintln!("{cases} exact complete styled-line comparisons");
}
