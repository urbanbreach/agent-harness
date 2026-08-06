use std::fmt::{Display, Formatter};
use std::ops::Range;

use super::ids::{BlockId, TranscriptIdentity, TurnId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSelection {
    pub turn_id: TurnId,
    pub block_id: BlockId,
    pub range: Range<usize>,
}

impl TranscriptSelection {
    pub fn new(
        turn_id: TurnId,
        block_id: BlockId,
        range: Range<usize>,
    ) -> Result<Self, SelectionError> {
        if range.start > range.end {
            return Err(SelectionError::ReversedRange {
                start: range.start,
                end: range.end,
            });
        }
        Ok(Self {
            turn_id,
            block_id,
            range,
        })
    }

    pub fn is_present_in(&self, identity: &TranscriptIdentity) -> bool {
        identity.contains(self.turn_id, self.block_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionError {
    ReversedRange { start: usize, end: usize },
}

impl Display for SelectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReversedRange { start, end } => {
                write!(formatter, "selection range is reversed: {start}..{end}")
            }
        }
    }
}

impl std::error::Error for SelectionError {}
