use crate::design_contract::LifecycleState;
use crate::transcript_blocks::{
    BlockEvent, BlockKind, BlockLifecycle, BlockStoreError, RawDisclosure, TranscriptBlocks,
};
use crate::transcript_identity::{BlockId, TurnId};
use crate::transcript_timeline::TimelineStatus;

use super::cache::BoundedLayoutCache;
use super::composite::TranscriptIntegrationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSeed {
    pub replay: crate::transcript_identity::ReplayTurn,
    pub status: TimelineStatus,
    pub lifecycle: LifecycleState,
    pub label: String,
}

impl TurnSeed {
    pub fn new(
        replay: crate::transcript_identity::ReplayTurn,
        status: TimelineStatus,
        lifecycle: LifecycleState,
    ) -> Self {
        Self {
            replay,
            status,
            lifecycle,
            label: String::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub const fn turn_id(&self) -> TurnId {
        self.replay.turn_id()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSeed {
    pub id: BlockId,
    pub turn_id: TurnId,
    pub kind: BlockKind,
    pub lifecycle: BlockLifecycle,
    pub content: String,
    pub raw: Option<RawDisclosure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEvent {
    TurnStarted(TurnSeed),
    TurnStatus {
        turn_id: TurnId,
        status: TimelineStatus,
        lifecycle: LifecycleState,
    },
    BlockCreated(BlockSeed),
    BlockAppended {
        id: BlockId,
        content: String,
    },
    BlockLifecycle {
        id: BlockId,
        lifecycle: BlockLifecycle,
    },
    FoldToggled {
        id: BlockId,
    },
    BlockRemoved {
        id: BlockId,
    },
    TurnRemoved {
        turn_id: TurnId,
    },
    Compacted {
        previous_turn_id: TurnId,
        replacement: TurnSeed,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInvalidation {
    WidthChanged {
        from: u16,
        to: u16,
    },
    FoldToggled(BlockId),
    LifecycleTransition {
        block_id: BlockId,
        from: BlockLifecycle,
        to: BlockLifecycle,
    },
    BlockAppended(BlockId),
    BlockRemoved(BlockId),
    Compaction,
    ReplayReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidationReport {
    pub removed_entries: usize,
    pub cleared_all: bool,
}

impl InvalidationReport {
    pub const fn partial(removed_entries: usize) -> Self {
        Self {
            removed_entries,
            cleared_all: false,
        }
    }
    pub const fn cleared(removed_entries: usize) -> Self {
        Self {
            removed_entries,
            cleared_all: true,
        }
    }
}

impl CacheInvalidation {
    pub fn apply(self, cache: &mut BoundedLayoutCache) -> InvalidationReport {
        match self {
            Self::WidthChanged { to, .. } => InvalidationReport::cleared(cache.set_width(to)),
            Self::FoldToggled(id)
            | Self::LifecycleTransition { block_id: id, .. }
            | Self::BlockAppended(id)
            | Self::BlockRemoved(id) => InvalidationReport::partial(cache.invalidate_block(id)),
            Self::Compaction | Self::ReplayReset => InvalidationReport::cleared(cache.clear()),
        }
    }
}

#[derive(Debug)]
struct ProjectedBlock {
    seed: BlockSeed,
    changes: Vec<BlockEvent>,
    removed: bool,
}

#[derive(Debug)]
pub(crate) struct Projection {
    pub(crate) turns: Vec<TurnSeed>,
    pub(crate) blocks: TranscriptBlocks,
}

pub(crate) fn project(
    events: &[TranscriptEvent],
) -> Result<Projection, TranscriptIntegrationError> {
    let mut turns = Vec::new();
    let mut blocks = Vec::new();
    for event in events {
        match event {
            TranscriptEvent::TurnStarted(turn) => turns.push(turn.clone()),
            TranscriptEvent::TurnStatus {
                turn_id,
                status,
                lifecycle,
            } => {
                let turn = turns
                    .iter_mut()
                    .find(|turn| turn.turn_id() == *turn_id)
                    .ok_or(TranscriptIntegrationError::MissingTurn(*turn_id))?;
                turn.status = *status;
                turn.lifecycle = *lifecycle;
            }
            TranscriptEvent::BlockCreated(seed) => {
                if blocks
                    .iter()
                    .any(|block: &ProjectedBlock| block.seed.id == seed.id)
                {
                    return Err(TranscriptIntegrationError::Blocks(
                        BlockStoreError::DuplicateBlock(seed.id),
                    ));
                }
                blocks.push(ProjectedBlock {
                    seed: seed.clone(),
                    changes: Vec::new(),
                    removed: false,
                });
            }
            TranscriptEvent::BlockAppended { id, content } => {
                block_change(
                    &mut blocks,
                    *id,
                    BlockEvent::Append {
                        id: *id,
                        content: content.clone(),
                    },
                )?;
            }
            TranscriptEvent::BlockLifecycle { id, lifecycle } => {
                block_change(
                    &mut blocks,
                    *id,
                    BlockEvent::Lifecycle {
                        id: *id,
                        lifecycle: *lifecycle,
                    },
                )?;
            }
            TranscriptEvent::FoldToggled { id } => {
                block_change(&mut blocks, *id, BlockEvent::ToggleFold { id: *id })?;
            }
            TranscriptEvent::BlockRemoved { id } => {
                blocks
                    .iter_mut()
                    .find(|block| block.seed.id == *id)
                    .ok_or(TranscriptIntegrationError::MissingBlock(*id))?
                    .removed = true;
            }
            TranscriptEvent::TurnRemoved { turn_id } => {
                turns.retain(|turn| turn.turn_id() != *turn_id)
            }
            TranscriptEvent::Compacted {
                previous_turn_id,
                replacement,
            } => {
                let turn = turns
                    .iter_mut()
                    .find(|turn| turn.turn_id() == *previous_turn_id)
                    .ok_or(TranscriptIntegrationError::MissingTurn(*previous_turn_id))?;
                *turn = replacement.clone();
            }
        }
    }
    let mut projected = TranscriptBlocks::new();
    for block in blocks.into_iter().filter(|block| {
        !block.removed
            && turns
                .iter()
                .any(|turn| turn.turn_id() == block.seed.turn_id)
    }) {
        projected.insert(
            block.seed.id,
            block.seed.kind,
            block.seed.lifecycle,
            block.seed.content,
            block.seed.raw,
        );
        for change in block.changes {
            match change {
                BlockEvent::Append { id, content } => projected.append(id, &content)?,
                BlockEvent::Lifecycle { id, lifecycle } => {
                    projected.set_lifecycle(id, lifecycle)?
                }
                BlockEvent::ToggleFold { id } => projected.toggle_fold(id)?,
                BlockEvent::Create { .. } => {}
            }
        }
    }
    Ok(Projection {
        turns,
        blocks: projected,
    })
}

fn block_change(
    blocks: &mut [ProjectedBlock],
    id: BlockId,
    change: BlockEvent,
) -> Result<(), TranscriptIntegrationError> {
    let block = blocks
        .iter_mut()
        .find(|block| block.seed.id == id && !block.removed)
        .ok_or(TranscriptIntegrationError::MissingBlock(id))?;
    block.changes.push(change);
    Ok(())
}
