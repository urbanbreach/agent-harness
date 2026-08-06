mod folds;
mod kind;
mod raw;
mod styling;

pub use folds::{default_fold, BlockLifecycle, FoldController, FoldState};
pub use kind::BlockKind;
pub use raw::{RawDisclosure, RawPayload, Redaction, RedactionReason};
pub use styling::{style_for, BlockStyle};

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
pub struct TranscriptBlock {
    id: BlockId,
    kind: BlockKind,
    lifecycle: BlockLifecycle,
    content: String,
    fold: FoldController,
    raw: Option<RawDisclosure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSnapshot {
    pub id: BlockId,
    pub kind: BlockKind,
    pub lifecycle: BlockLifecycle,
    pub content: String,
    pub fold_state: FoldState,
    pub raw: Option<RawDisclosure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockEvent {
    Create {
        id: BlockId,
        kind: BlockKind,
        lifecycle: BlockLifecycle,
        content: String,
    },
    Append {
        id: BlockId,
        content: String,
    },
    Lifecycle {
        id: BlockId,
        lifecycle: BlockLifecycle,
    },
    ToggleFold {
        id: BlockId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockStoreError {
    MissingBlock(BlockId),
    DuplicateBlock(BlockId),
}

impl Display for BlockStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBlock(id) => write!(formatter, "missing transcript block {id}"),
            Self::DuplicateBlock(id) => write!(formatter, "duplicate transcript block {id}"),
        }
    }
}

impl Error for BlockStoreError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptBlocks {
    blocks: Vec<TranscriptBlock>,
}

impl TranscriptBlocks {
    pub const fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    pub fn insert(
        &mut self,
        id: BlockId,
        kind: BlockKind,
        lifecycle: BlockLifecycle,
        content: impl Into<String>,
        raw: Option<RawDisclosure>,
    ) {
        self.blocks.push(TranscriptBlock {
            id,
            kind,
            lifecycle,
            content: content.into(),
            fold: FoldController::new(kind, lifecycle),
            raw,
        });
    }

    pub fn replay(events: &[BlockEvent]) -> Result<Self, BlockStoreError> {
        let mut blocks = Self::new();
        blocks.apply_all(events)?;
        Ok(blocks)
    }

    pub fn apply_all(&mut self, events: &[BlockEvent]) -> Result<(), BlockStoreError> {
        for event in events {
            self.apply(event.clone())?;
        }
        Ok(())
    }

    pub fn append(&mut self, id: BlockId, content: &str) -> Result<(), BlockStoreError> {
        self.get_mut(id)?.content.push_str(content);
        Ok(())
    }

    pub fn toggle_fold(&mut self, id: BlockId) -> Result<(), BlockStoreError> {
        let block = self.get_mut(id)?;
        block.fold = block.fold.toggle();
        Ok(())
    }

    pub fn set_lifecycle(
        &mut self,
        id: BlockId,
        lifecycle: BlockLifecycle,
    ) -> Result<(), BlockStoreError> {
        let block = self.get_mut(id)?;
        block.lifecycle = lifecycle;
        block.fold = block.fold.lifecycle_changed(block.kind, lifecycle);
        Ok(())
    }

    pub fn get(&self, id: BlockId) -> Option<&TranscriptBlock> {
        self.blocks.iter().find(|block| block.id == id)
    }

    pub fn snapshot(&self) -> Vec<BlockSnapshot> {
        self.blocks.iter().map(TranscriptBlock::snapshot).collect()
    }

    fn apply(&mut self, event: BlockEvent) -> Result<(), BlockStoreError> {
        match event {
            BlockEvent::Create {
                id,
                kind,
                lifecycle,
                content,
            } => {
                if self.get(id).is_some() {
                    return Err(BlockStoreError::DuplicateBlock(id));
                }
                self.insert(id, kind, lifecycle, content, None);
                Ok(())
            }
            BlockEvent::Append { id, content } => self.append(id, &content),
            BlockEvent::Lifecycle { id, lifecycle } => self.set_lifecycle(id, lifecycle),
            BlockEvent::ToggleFold { id } => self.toggle_fold(id),
        }
    }

    fn get_mut(&mut self, id: BlockId) -> Result<&mut TranscriptBlock, BlockStoreError> {
        self.blocks
            .iter_mut()
            .find(|block| block.id == id)
            .ok_or(BlockStoreError::MissingBlock(id))
    }
}

impl TranscriptBlock {
    pub const fn id(&self) -> BlockId {
        self.id
    }

    pub const fn kind(&self) -> BlockKind {
        self.kind
    }

    pub const fn lifecycle(&self) -> BlockLifecycle {
        self.lifecycle
    }

    pub const fn fold_state(&self) -> FoldState {
        self.fold.state()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub const fn raw(&self) -> Option<&RawDisclosure> {
        self.raw.as_ref()
    }

    fn snapshot(&self) -> BlockSnapshot {
        BlockSnapshot {
            id: self.id,
            kind: self.kind,
            lifecycle: self.lifecycle,
            content: self.content.clone(),
            fold_state: self.fold_state(),
            raw: self.raw.clone(),
        }
    }
}
