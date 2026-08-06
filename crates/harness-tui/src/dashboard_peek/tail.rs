use crate::transcript_blocks::BlockSnapshot;
use crate::transcript_identity::BlockId;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailError {
    ZeroCapacity,
    MissingBlock(BlockId),
}

impl Display for TailError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("peek tail capacity must be positive"),
            Self::MissingBlock(id) => write!(formatter, "peek tail block is missing: {id}"),
        }
    }
}

impl Error for TailError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailUpdateKind {
    Replaced,
    Incremental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailUpdate {
    pub kind: TailUpdateKind,
    pub appended: usize,
    pub replaced: usize,
    pub evicted: usize,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeekTail {
    blocks: VecDeque<BlockSnapshot>,
    capacity: usize,
    revision: u64,
}

impl PeekTail {
    pub fn new(capacity: usize) -> Result<Self, TailError> {
        if capacity == 0 {
            return Err(TailError::ZeroCapacity);
        }
        Ok(Self {
            blocks: VecDeque::new(),
            capacity,
            revision: 0,
        })
    }

    pub fn replace(&mut self, blocks: &[BlockSnapshot]) -> TailUpdate {
        self.blocks.clear();
        self.blocks
            .extend(blocks.iter().cloned().rev().take(self.capacity).rev());
        self.revision = self.revision.saturating_add(1);
        TailUpdate {
            kind: TailUpdateKind::Replaced,
            appended: self.blocks.len(),
            replaced: 0,
            evicted: blocks.len().saturating_sub(self.blocks.len()),
            revision: self.revision,
        }
    }

    pub fn append_block(&mut self, block: BlockSnapshot) -> TailUpdate {
        if let Some(existing) = self.blocks.iter_mut().find(|item| item.id == block.id) {
            *existing = block;
            self.revision = self.revision.saturating_add(1);
            return TailUpdate {
                kind: TailUpdateKind::Incremental,
                appended: 0,
                replaced: 1,
                evicted: 0,
                revision: self.revision,
            };
        }

        self.blocks.push_back(block);
        let mut evicted = 0;
        while self.blocks.len() > self.capacity {
            let _ = self.blocks.pop_front();
            evicted += 1;
        }
        self.revision = self.revision.saturating_add(1);
        TailUpdate {
            kind: TailUpdateKind::Incremental,
            appended: 1,
            replaced: 0,
            evicted,
            revision: self.revision,
        }
    }

    pub fn append_content(&mut self, id: BlockId, content: &str) -> Result<TailUpdate, TailError> {
        let Some(block) = self.blocks.iter_mut().find(|block| block.id == id) else {
            return Err(TailError::MissingBlock(id));
        };
        block.content.push_str(content);
        self.revision = self.revision.saturating_add(1);
        Ok(TailUpdate {
            kind: TailUpdateKind::Incremental,
            appended: 0,
            replaced: 1,
            evicted: 0,
            revision: self.revision,
        })
    }

    pub fn snapshot(&self) -> Vec<BlockSnapshot> {
        self.blocks.iter().cloned().collect()
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }
}
