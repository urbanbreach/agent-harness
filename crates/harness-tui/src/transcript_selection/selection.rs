use super::{
    CellPoint, DragResult, Grapheme, NavigationKey, SelectionError, SelectionMode, SelectionRange,
    TextLayout, Viewport,
};

/// Owned grapheme inspection remains available to public callers. Application
/// surfaces use the same layout directly without materializing these strings.
#[derive(Debug, Clone)]
pub struct WrappedText {
    layout: TextLayout,
    graphemes: Vec<Vec<Grapheme>>,
}

impl WrappedText {
    pub fn new(text: &str, width: usize) -> Result<Self, SelectionError> {
        let layout = TextLayout::new(text.to_owned(), width)?;
        let graphemes = layout.owned_graphemes();
        Ok(Self { layout, graphemes })
    }

    pub fn row_count(&self) -> usize {
        self.layout.row_count()
    }

    pub fn row_text(&self, row: usize) -> String {
        self.layout.row_text(row).to_owned()
    }

    pub fn point_for_byte(&self, byte: usize) -> CellPoint {
        self.layout.point_for_byte(byte)
    }

    pub fn grapheme_at(&self, point: CellPoint) -> Option<&Grapheme> {
        self.graphemes
            .get(point.row)?
            .iter()
            .find(|cluster| cluster.range.cell_range.contains(&point.cell))
    }

    pub const fn drag(&self, anchor: CellPoint, focus: CellPoint) -> SelectionRange {
        SelectionRange::new(anchor, focus)
    }

    pub fn drag_with_autoscroll(
        &self,
        _anchor: CellPoint,
        focus: CellPoint,
        viewport: Viewport,
    ) -> DragResult {
        self.layout.drag_with_autoscroll(focus, viewport)
    }

    pub fn select(&self, point: CellPoint, mode: SelectionMode) -> SelectionRange {
        self.layout.select(point, mode)
    }

    pub fn copy(&self, selection: SelectionRange) -> Result<String, SelectionError> {
        self.layout.copy(selection)
    }

    pub fn move_focus(&self, point: CellPoint, key: NavigationKey) -> CellPoint {
        self.layout.move_focus(point, key)
    }
}
