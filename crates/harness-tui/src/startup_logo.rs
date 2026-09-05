use ratatui::style::{Color, Style};
use ratatui::text::Span;

const FULL_WIDTH: usize = 15;
const FULL_ROWS: [&str; 7] = [
    " ██╗  ██╗",
    " ██║  ██║",
    " ██║  ██║",
    " ███████║",
    " ██╔══██║",
    " ██║  ██║",
    " ╚═╝  ╚═╝",
];

#[derive(Clone, Copy)]
pub(crate) struct Logo {
    rows: &'static [&'static str],
    width: usize,
}

impl Logo {
    pub(crate) const fn rows(self) -> &'static [&'static str] {
        self.rows
    }

    pub(crate) const fn width(self) -> usize {
        self.width
    }

    pub(crate) const fn height(self) -> usize {
        self.rows.len()
    }
}

const FULL_LOGO: Logo = Logo {
    rows: &FULL_ROWS,
    width: FULL_WIDTH,
};

pub(crate) fn row_spans(logo: Logo, row_index: usize, color: Color) -> Vec<Span<'static>> {
    vec![Span::styled(
        padded_row(logo.rows()[row_index], logo.width()),
        Style::default().fg(color),
    )]
}

pub(crate) const fn full_logo(glyphs_supported: bool) -> Option<Logo> {
    if glyphs_supported {
        Some(FULL_LOGO)
    } else {
        None
    }
}

fn padded_row(row: &str, width: usize) -> String {
    let mut output = row.to_string();
    while output.chars().count() < width {
        output.push(' ');
    }
    output.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_logo_collapses_its_layout_width() {
        assert!(full_logo(false).is_none());
    }
}
