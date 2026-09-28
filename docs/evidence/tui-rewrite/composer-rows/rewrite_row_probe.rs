#![allow(dead_code)]
use harness_tui::composer_atoms::{
    AtomBuffer, AtomKind, AttachmentId, ComposerAtom, FileMentionId, GraphemeCluster, WrappedLine,
};
mod text {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/text.rs"));
}
fn legacy_wrap(buffer: &AtomBuffer, width: u16) -> Vec<WrappedLine> {
    let mut lines = Vec::new();
    let mut current = WrappedLine {
        atom_ids: Vec::new(),
        display_width: 0,
    };
    for atom in buffer.atoms() {
        if matches!(atom.kind, AtomKind::Newline) {
            current.atom_ids.push(atom.id);
            lines.push(current);
            current = WrappedLine {
                atom_ids: Vec::new(),
                display_width: 0,
            };
        } else if current.display_width > 0
            && current.display_width.saturating_add(atom.display_width) > width
        {
            lines.push(current);
            current = WrappedLine {
                atom_ids: vec![atom.id],
                display_width: atom.display_width,
            };
        } else {
            current.atom_ids.push(atom.id);
            current.display_width = current.display_width.saturating_add(atom.display_width);
        }
    }
    lines.push(current);
    lines
}

mod legacy_height {
    use unicode_segmentation::UnicodeSegmentation;
    const COMPOSER_VISIBLE_TEXT_CHROME: u16 = 6;
    const MIN_COMPOSER_LINES: u16 = 1;
    const MAX_COMPOSER_LINES: u16 = 6;
    const PROMPT_MIN_MAX_HEIGHT: u16 = 6;
    pub(crate) fn composer_input_height(text: &str, width: u16) -> u16 {
        composer_input_height_with_max_lines(text, width, MAX_COMPOSER_LINES)
    }

    pub(crate) fn startup_composer_input_height(
        text: &str,
        width: u16,
        terminal_height: u16,
    ) -> u16 {
        composer_input_height_with_max_lines(text, width, prompt_max_height(terminal_height))
    }

    fn composer_input_height_with_max_lines(text: &str, width: u16, max_lines: u16) -> u16 {
        let inner_width = usize::from(width.saturating_sub(COMPOSER_VISIBLE_TEXT_CHROME).max(1));
        let wrapped_lines = if text.is_empty() {
            1
        } else {
            text.split('\n')
                .map(|line| word_wrapped_line_count(line, inner_width))
                .sum()
        };

        let clamped_lines =
            wrapped_lines.clamp(usize::from(MIN_COMPOSER_LINES), usize::from(max_lines));
        u16::try_from(clamped_lines).unwrap_or(max_lines)
    }

    fn prompt_max_height(terminal_height: u16) -> u16 {
        PROMPT_MIN_MAX_HEIGHT.max(terminal_height / 3)
    }

    fn display_width(text: &str) -> usize {
        text.lines()
            .map(unicode_width::UnicodeWidthStr::width)
            .sum()
    }

    fn word_wrapped_line_count(line: &str, width: usize) -> usize {
        if line.is_empty() {
            return 1;
        }

        let chars = line
            .graphemes(true)
            .map(|grapheme| (grapheme, display_width(grapheme).max(1)))
            .collect::<Vec<_>>();
        let mut count = 0usize;
        let mut start = 0usize;
        while start < chars.len() {
            count += 1;
            let fit_end = word_wrap_fit_end(&chars, start, width.max(1));
            if fit_end >= chars.len() {
                break;
            }

            if let Some(break_at) = chars[start..fit_end]
                .iter()
                .rposition(|(ch, _)| ch.chars().all(char::is_whitespace))
                .map(|offset| start + offset)
                .filter(|break_at| *break_at > start)
            {
                start = break_at + 1;
            } else if chars[fit_end].0.chars().all(char::is_whitespace) {
                start = fit_end + 1;
            } else {
                start = fit_end.max(start + 1);
            }
        }

        count.max(1)
    }

    fn word_wrap_fit_end(chars: &[(&str, usize)], start: usize, width: usize) -> usize {
        let mut used = 0usize;
        for (position, (_, char_width)) in chars.iter().enumerate().skip(start) {
            if position > start && used.saturating_add(*char_width) > width {
                return position;
            }
            used = used.saturating_add(*char_width);
        }
        chars.len()
    }
}
mod candidate_height {
    use unicode_segmentation::UnicodeSegmentation;
    const COMPOSER_VISIBLE_TEXT_CHROME: u16 = 6;
    const MIN_COMPOSER_LINES: u16 = 1;
    const MAX_COMPOSER_LINES: u16 = 6;
    const PROMPT_MIN_MAX_HEIGHT: u16 = 6;
    pub(crate) fn composer_input_height(text: &str, width: u16) -> u16 {
        composer_input_height_with_max_lines(text, width, MAX_COMPOSER_LINES)
    }

    pub(crate) fn startup_composer_input_height(
        text: &str,
        width: u16,
        terminal_height: u16,
    ) -> u16 {
        composer_input_height_with_max_lines(text, width, prompt_max_height(terminal_height))
    }

    fn composer_input_height_with_max_lines(text: &str, width: u16, max_lines: u16) -> u16 {
        let inner_width = usize::from(width.saturating_sub(COMPOSER_VISIBLE_TEXT_CHROME).max(1));
        let mut rows = 0;
        for mut line in text.split('\n') {
            loop {
                rows += 1;
                if rows == max_lines {
                    return rows;
                }
                let (_, next) = crate::text::composer_row_end(line, inner_width);
                line = &line[next..];
                if line.is_empty() {
                    break;
                }
            }
        }
        rows
    }

    fn prompt_max_height(terminal_height: u16) -> u16 {
        PROMPT_MIN_MAX_HEIGHT.max(terminal_height / 3)
    }
}
#[test]
fn recorded_atom_wrap_contract() {
    let mut cases = 0;
    for a in 0..7 {
        for b in 0..7 {
            for c in 0..7 {
                let atoms = [a, b, c]
                    .into_iter()
                    .enumerate()
                    .map(|(index, kind)| {
                        let id = index as u64;
                        match kind {
                            0 => ComposerAtom::text(id, GraphemeCluster::new("x")),
                            1 => ComposerAtom::text(id, GraphemeCluster::new("界")),
                            2 => ComposerAtom::attachment(id, AttachmentId::new(1)),
                            3 => ComposerAtom::file_mention(id, FileMentionId::new(1)),
                            4 => ComposerAtom::newline(id),
                            5 => {
                                let mut atom = ComposerAtom::text(id, GraphemeCluster::new("x"));
                                atom.display_width = u16::MAX;
                                atom
                            }
                            _ => {
                                let mut atom = ComposerAtom::newline(id);
                                atom.display_width = 5;
                                atom
                            }
                        }
                    })
                    .collect();
                let buffer = AtomBuffer::from_atoms(atoms).expect("unique ids");
                for width in [0, 1, 2, 3, 10, u16::MAX - 1, u16::MAX] {
                    assert_eq!(
                        buffer.wrap(width),
                        legacy_wrap(&buffer, width),
                        "kinds {a}/{b}/{c}, width {width}"
                    );
                    cases += 1;
                }
            }
        }
    }
    for text in ["", "\n", "\n\n", "a\n", "e\u{301} 👩‍💻"] {
        let buffer = AtomBuffer::from_text(text);
        for width in [0, 1, 2, 3, 10, u16::MAX] {
            assert_eq!(buffer.wrap(width), legacy_wrap(&buffer, width));
            cases += 1;
        }
    }
    eprintln!("{cases} exact atom-row comparisons");
}
#[test]
fn recorded_row_height_contract() {
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
    let mut texts = vec![String::new(), "line\n".repeat(100), "x".repeat(1000)];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                texts.push(format!("{a}{b}{c}"));
            }
        }
    }
    let mut cases = 0;
    for text in texts {
        for width in [0, 1, 6, 7, 8, 10, 16, 40, 120] {
            assert_eq!(
                candidate_height::composer_input_height(&text, width),
                legacy_height::composer_input_height(&text, width),
                "live {text:?}, width {width}"
            );
            cases += 1;
            for height in [0, 18, 48, u16::MAX] {
                assert_eq!(
                    candidate_height::startup_composer_input_height(&text, width, height),
                    legacy_height::startup_composer_input_height(&text, width, height),
                    "startup {text:?}, width {width}, height {height}"
                );
                cases += 1;
            }
        }
    }
    eprintln!("{cases} exact frame-height comparisons");
}
