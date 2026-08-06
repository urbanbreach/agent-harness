use std::fmt::{Display, Formatter};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellPoint {
    pub row: usize,
    pub cell: usize,
}

impl CellPoint {
    pub const fn new(row: usize, cell: usize) -> Self {
        Self { row, cell }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub top: usize,
    pub height: usize,
}

impl Viewport {
    pub const fn new(top: usize, height: usize) -> Self {
        Self { top, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Autoscroll {
    pub lines: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragResult {
    pub focus: CellPoint,
    pub autoscroll: Autoscroll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphemeRange {
    pub byte_range: Range<usize>,
    pub cell_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grapheme {
    pub text: String,
    pub range: GraphemeRange,
    pub end: CellPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Character,
    Word,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRange {
    pub anchor: CellPoint,
    pub focus: CellPoint,
}

impl SelectionRange {
    pub const fn new(anchor: CellPoint, focus: CellPoint) -> Self {
        Self { anchor, focus }
    }

    pub(crate) fn normalized(self) -> (CellPoint, CellPoint) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionError {
    ZeroWidth,
    EmptyText,
    EmptySelection,
    InvalidPoint,
}

impl Display for SelectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ZeroWidth => "selection width must be greater than zero",
            Self::EmptyText => "selection text is empty",
            Self::EmptySelection => "selection contains no complete grapheme",
            Self::InvalidPoint => "selection point is outside the wrapped text",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SelectionError {}
