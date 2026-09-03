use std::fmt::{Display, Formatter};

use super::osc52::{wrap_tmux, TmuxSequence};
use super::selection_types::CellPoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkRange {
    pub row: usize,
    pub start_cell: usize,
    pub end_cell: usize,
}

impl LinkRange {
    pub const fn new(row: usize, start_cell: usize, end_cell: usize) -> Self {
        Self {
            row,
            start_cell,
            end_cell,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hyperlink {
    label: String,
    url: String,
    range: LinkRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HyperlinkError {
    EmptyUrl,
    ControlCharacter,
    LabelControlCharacter,
    UnsafeScheme,
    ReversedRange,
}

impl Display for HyperlinkError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyUrl => "hyperlink URL is empty",
            Self::ControlCharacter => "hyperlink URL contains a control character",
            Self::LabelControlCharacter => "hyperlink label contains a control character",
            Self::UnsafeScheme => "hyperlink URL scheme is not allowed",
            Self::ReversedRange => "hyperlink range is reversed",
        };
        formatter.write_str(message)
    }
}
impl std::error::Error for HyperlinkError {}

impl Hyperlink {
    pub fn new(
        label: impl Into<String>,
        url: &str,
        range: LinkRange,
    ) -> Result<Self, HyperlinkError> {
        let label = label.into();
        if label.chars().any(char::is_control) {
            return Err(HyperlinkError::LabelControlCharacter);
        }
        if url.is_empty() {
            return Err(HyperlinkError::EmptyUrl);
        }
        if url.chars().any(char::is_control) {
            return Err(HyperlinkError::ControlCharacter);
        }
        if !safe_external_url(url) {
            return Err(HyperlinkError::UnsafeScheme);
        }
        if range.start_cell > range.end_cell {
            return Err(HyperlinkError::ReversedRange);
        }
        Ok(Self {
            label,
            url: url.to_string(),
            range,
        })
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    fn contains(&self, point: CellPoint) -> bool {
        self.range.row == point.row
            && (self.range.start_cell..=self.range.end_cell).contains(&point.cell)
    }
}

#[derive(Debug, Clone, Default)]
pub struct HyperlinkMap {
    links: Vec<Hyperlink>,
}

impl HyperlinkMap {
    pub fn new(links: Vec<Hyperlink>) -> Self {
        Self { links }
    }
    pub fn hover(&self, point: CellPoint) -> Option<&Hyperlink> {
        self.links.iter().find(|link| link.contains(point))
    }
    pub fn click(&self, point: CellPoint) -> Option<&Hyperlink> {
        self.hover(point)
    }
}

pub(crate) fn safe_external_url(url: &str) -> bool {
    !url.chars().any(char::is_control)
        && ["http://", "https://"].into_iter().any(|scheme| {
            url.len() > scheme.len()
                && url
                    .get(..scheme.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

pub fn hyperlink_sequence(link: &Hyperlink, route: TmuxSequence) -> String {
    let sequence = format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", link.url, link.label);
    match route {
        TmuxSequence::Direct => sequence,
        TmuxSequence::Tmux => wrap_tmux(&sequence),
    }
}
