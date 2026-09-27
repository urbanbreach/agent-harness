mod folds;
mod kind;
mod raw;

pub use folds::{default_fold, BlockLifecycle, FoldState};
pub use kind::BlockKind;
pub use raw::{RawDisclosure, RawPayload, Redaction, RedactionReason};

use crate::transcript_identity::BlockId;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawDisclosureError {
    InvalidJson(String),
}

impl Display for RawDisclosureError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(message) => write!(formatter, "invalid raw JSON: {message}"),
        }
    }
}

impl Error for RawDisclosureError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSnapshot {
    pub id: BlockId,
    pub kind: BlockKind,
    pub lifecycle: BlockLifecycle,
    pub content: String,
    pub fold_state: FoldState,
    pub raw: Option<RawDisclosure>,
}
