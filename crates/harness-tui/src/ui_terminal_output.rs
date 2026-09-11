//! Interpret a recorded tool stream into styled cells. Terminal commands are
//! consumed here; only printable text and Ratatui styles leave this boundary.
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vte::{Params, Perform};

pub(super) fn render(text: &str, base: Style, theme: &crate::theme::Theme) -> Vec<Line<'static>> {
    let mut output = Output {
        rows: vec![Vec::new()],
        column: 0,
        base,
        style: base,
    };
    vte::Parser::new().advance(&mut output, text.as_bytes());
    let lines: Vec<Line<'static>> = output
        .rows
        .into_iter()
        .map(|row| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (text, mut style) in row {
                style.fg = style
                    .fg
                    .map(|color| crate::theme::quantize_color(color, theme.color_level()));
                style.bg = style
                    .bg
                    .map(|color| crate::theme::quantize_color(color, theme.color_level()));
                if let Some(last) = spans.last_mut().filter(|last| last.style == style) {
                    last.content.to_mut().push_str(&text);
                } else {
                    spans.push(Span::styled(text, style));
                }
            }
            Line::from(spans)
        })
        .collect();
    let plain = lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    if let crate::transcript_blocks::RawPayload::Text(redacted) =
        crate::transcript_blocks::RawDisclosure::from_text(&plain).payload
    {
        if redacted != plain {
            return redacted
                .lines()
                .map(|line| Line::from(Span::styled(line.to_string(), base)))
                .collect();
        }
    }
    lines
}

struct Output {
    rows: Vec<Vec<(String, Style)>>,
    column: usize,
    base: Style,
    style: Style,
}

impl Perform for Output {
    fn print(&mut self, c: char) {
        if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            return;
        }
        let mut text = c.to_string();
        let Some(row) = self.rows.last_mut() else {
            return;
        };
        // VTE emits scalars; the terminal cursor advances in grapheme cells.
        // Keep ZWJ emoji, flags, modifiers, and variation selectors in one cell
        // owner so a subsequent overwrite clears the whole glyph.
        if let Some(index) = row
            .iter()
            .take(self.column)
            .rposition(|(text, _)| !text.is_empty())
        {
            let previous = &row[index].0;
            let joined = format!("{previous}{c}");
            if index + previous.width() == self.column && joined.graphemes(true).nth(1).is_none() {
                text = joined;
                self.column = index;
            }
        }
        let width = text.width();
        if width == 0 {
            if let Some((text, _)) = row
                .iter_mut()
                .take(self.column)
                .rev()
                .find(|(text, _)| !text.is_empty())
            {
                text.push(c);
            }
            return;
        }
        // CSI cursor movement cannot allocate arbitrary amounts of memory.
        self.column = self.column.min(65_535);
        row.resize(
            row.len().max(self.column + width),
            (" ".to_string(), self.base),
        );
        // Erase both halves of any wide glyph touched by this write.
        for index in self.column..self.column + width {
            if row[index].0.is_empty() && index > 0 {
                row[index - 1] = (" ".into(), self.base);
            }
            if row.get(index + 1).is_some_and(|(text, _)| text.is_empty()) {
                row[index + 1] = (" ".into(), self.base);
            }
        }
        row[self.column] = (text, self.style);
        if width == 2 {
            row[self.column + 1] = (String::new(), self.style);
        }
        self.column += width;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.rows.push(Vec::new());
                self.column = 0;
            }
            b'\r' => self.column = 0,
            8 => self.column = self.column.saturating_sub(1),
            b'\t' => {
                let stop = (self.column / 4 + 1) * 4;
                while self.column < stop {
                    self.print(' ');
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || !intermediates.is_empty() {
            return;
        }
        let values: Vec<u16> = params
            .iter()
            .flat_map(|part| part.iter().copied())
            .collect();
        let first = values.first().copied().unwrap_or_default();
        match action {
            'K' => self.erase_line(first),
            'G' => self.column = usize::from(first.max(1) - 1),
            'C' => {
                self.column = self
                    .column
                    .saturating_add(usize::from(first.max(1)))
                    .min(65_535)
            }
            'D' => self.column = self.column.saturating_sub(usize::from(first.max(1))),
            'm' => self.set_graphics(&values),
            _ => {}
        }
    }
}

impl Output {
    fn erase_line(&mut self, mode: u16) {
        let Some(row) = self.rows.last_mut() else {
            return;
        };
        match mode {
            0 => {
                if row
                    .get(self.column)
                    .is_some_and(|(text, _)| text.is_empty())
                    && self.column > 0
                {
                    row[self.column - 1] = (" ".into(), self.base);
                }
                row.truncate(self.column.min(row.len()));
            }
            1 => {
                if row
                    .get(self.column + 1)
                    .is_some_and(|(text, _)| text.is_empty())
                {
                    row[self.column + 1] = (" ".into(), self.base);
                }
                for cell in row.iter_mut().take(self.column + 1) {
                    *cell = (" ".to_string(), self.base);
                }
            }
            2 => row.clear(),
            _ => {}
        }
    }

    fn set_graphics(&mut self, values: &[u16]) {
        let mut index = 0;
        while index < values.len().max(1) {
            let value = values.get(index).copied().unwrap_or_default();
            match value {
                0 => self.style = self.base,
                1 => self.style = self.style.add_modifier(Modifier::BOLD),
                2 => self.style = self.style.add_modifier(Modifier::DIM),
                3 => self.style = self.style.add_modifier(Modifier::ITALIC),
                4 => self.style = self.style.add_modifier(Modifier::UNDERLINED),
                22 => self.style = self.style.remove_modifier(Modifier::BOLD | Modifier::DIM),
                23 => self.style = self.style.remove_modifier(Modifier::ITALIC),
                24 => self.style = self.style.remove_modifier(Modifier::UNDERLINED),
                30..=37 => {
                    self.style.fg =
                        Some(Color::Indexed(u8::try_from(value - 30).unwrap_or_default()))
                }
                90..=97 => {
                    self.style.fg = Some(Color::Indexed(
                        u8::try_from(value - 90 + 8).unwrap_or_default(),
                    ))
                }
                40..=47 => {
                    self.style.bg =
                        Some(Color::Indexed(u8::try_from(value - 40).unwrap_or_default()))
                }
                100..=107 => {
                    self.style.bg = Some(Color::Indexed(
                        u8::try_from(value - 100 + 8).unwrap_or_default(),
                    ))
                }
                39 => self.style.fg = self.base.fg,
                49 => self.style.bg = self.base.bg,
                38 | 48 => {
                    let (color, consumed) = extended_color(&values[index + 1..]);
                    index += consumed;
                    match (value, color) {
                        (38, Some(color)) => self.style.fg = Some(color),
                        (48, Some(color)) => self.style.bg = Some(color),
                        _ => {}
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }
}

fn extended_color(values: &[u16]) -> (Option<Color>, usize) {
    match values {
        [5, color, ..] => (u8::try_from(*color).ok().map(Color::Indexed), 2),
        [2, r, g, b, ..] => {
            let color = match (u8::try_from(*r), u8::try_from(*g), u8::try_from(*b)) {
                (Ok(r), Ok(g), Ok(b)) => Some(Color::Rgb(r, g, b)),
                _ => None,
            };
            (color, 4)
        }
        _ => (None, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorded_output_interprets_style_overwrite_and_discards_terminal_commands() {
        let lines = render(
            "\x1b[31mred\x1b[0m\nprogress 1\rprogress 2\x1b[K\x1b]52;c;secret\x07",
            Style::default(),
            &crate::theme::Theme::default(),
        );
        assert_eq!(lines[0].to_string(), "red");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Indexed(1)));
        assert_eq!(lines[1].to_string(), "progress 2");
        let wide = render(
            "界\u{301}\rX",
            Style::default(),
            &crate::theme::Theme::default(),
        );
        assert_eq!(wide[0].to_string(), "X ");
        for text in ["👩\u{200d}💻", "🇫🇮", "✈️", "👍🏽"] {
            let overwritten = render(
                &format!("{text}!\rX"),
                Style::default(),
                &crate::theme::Theme::default(),
            );
            assert_eq!(overwritten[0].to_string(), "X !", "{text}");
        }
        for (input, expected) in [("界!\x1b[2G\x1b[K", " "), ("界!\r\x1b[1K", "  !")] {
            assert_eq!(
                render(input, Style::default(), &crate::theme::Theme::default())[0].to_string(),
                expected
            );
        }
        assert!(lines
            .iter()
            .all(|line| !line.to_string().chars().any(char::is_control)));
    }
}
