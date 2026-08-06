use crate::parity::{compare_frames, IdentityMaskRegistry, SemanticFrame};

use super::error::ComparatorError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FocusState {
    pub row: u16,
    pub col: u16,
    pub active: bool,
}

impl FocusState {
    pub const fn new(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            active: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ZOrderEntry {
    pub layer: String,
    pub depth: u32,
}

impl ZOrderEntry {
    pub fn new(layer: impl Into<String>, depth: u32) -> Self {
        Self {
            layer: layer.into(),
            depth,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellSnapshot {
    pub frame: SemanticFrame,
    pub focus: Option<FocusState>,
    pub z_order: Vec<ZOrderEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellDiffRecord {
    pub path: String,
    pub expected: String,
    pub observed: String,
}

pub fn compare_cells(
    expected: &CellSnapshot,
    actual: &CellSnapshot,
    masks: &IdentityMaskRegistry,
) -> Result<(), ComparatorError> {
    let mut diffs = compare_frames(&expected.frame, &actual.frame, masks)
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|diff| CellDiffRecord {
            path: diff.path,
            expected: diff.expected,
            observed: diff.observed,
        })
        .collect::<Vec<_>>();
    if expected.focus != actual.focus {
        diffs.push(CellDiffRecord {
            path: "focus".to_owned(),
            expected: format!("{:?}", expected.focus),
            observed: format!("{:?}", actual.focus),
        });
    }
    if expected.z_order != actual.z_order {
        diffs.push(CellDiffRecord {
            path: "z_order".to_owned(),
            expected: format!("{:?}", expected.z_order),
            observed: format!("{:?}", actual.z_order),
        });
    }
    if diffs.is_empty() {
        Ok(())
    } else {
        let diffs_len = diffs.len();
        Err(ComparatorError::Cells { diffs, diffs_len })
    }
}
