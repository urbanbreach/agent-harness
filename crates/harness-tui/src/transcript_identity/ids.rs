use std::collections::HashSet;
use std::fmt::{Display, Formatter};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const EVENT_DOMAIN: u64 = 0x4556_454e_5400_0001;
const CHECKPOINT_DOMAIN: u64 = 0x4348_4b50_5400_0001;
const TURN_DOMAIN: u64 = 0x5455_524e_0000_0001;
const BLOCK_DOMAIN: u64 = 0x424c_4f43_4b00_0001;

const fn mix(mut hash: u64, value: u64) -> u64 {
    let mut byte = 0;
    while byte < 8 {
        hash ^= (value >> (byte * 8)) & 0xff;
        hash = hash.wrapping_mul(FNV_PRIME);
        byte += 1;
    }
    hash
}

const fn identity_hash(
    domain: u64,
    source: u64,
    event_seq: u64,
    turn_index: u64,
    block_index: u64,
) -> u64 {
    let hash = mix(FNV_OFFSET, domain);
    let hash = mix(hash, source);
    let hash = mix(hash, event_seq);
    let hash = mix(hash, turn_index);
    mix(hash, block_index)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TurnId(u64);

impl TurnId {
    pub const fn from_replay(event_seq: u64, turn_index: u64) -> Self {
        Self(identity_hash(
            TURN_DOMAIN,
            EVENT_DOMAIN,
            event_seq,
            turn_index,
            u64::MAX,
        ))
    }

    pub const fn from_checkpoint(checkpoint_seq: u64, turn_index: u64) -> Self {
        Self(identity_hash(
            TURN_DOMAIN,
            CHECKPOINT_DOMAIN,
            checkpoint_seq,
            turn_index,
            u64::MAX,
        ))
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl Display for TurnId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "turn-{value:016x}", value = self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u64);

impl BlockId {
    pub const fn from_replay(event_seq: u64, turn_index: u64, block_index: u64) -> Self {
        Self(identity_hash(
            BLOCK_DOMAIN,
            EVENT_DOMAIN,
            event_seq,
            turn_index,
            block_index,
        ))
    }

    pub const fn from_checkpoint(checkpoint_seq: u64, turn_index: u64, block_index: u64) -> Self {
        Self(identity_hash(
            BLOCK_DOMAIN,
            CHECKPOINT_DOMAIN,
            checkpoint_seq,
            turn_index,
            block_index,
        ))
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl Display for BlockId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "block-{value:016x}", value = self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplayTurnSource {
    Event { event_seq: u64 },
    Checkpoint { checkpoint_seq: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReplayTurn {
    pub source: ReplayTurnSource,
    pub turn_index: u64,
    pub block_count: u64,
}

impl ReplayTurn {
    pub const fn event(event_seq: u64, turn_index: u64, block_count: u64) -> Self {
        Self {
            source: ReplayTurnSource::Event { event_seq },
            turn_index,
            block_count,
        }
    }

    pub const fn checkpoint(checkpoint_seq: u64, turn_index: u64, block_count: u64) -> Self {
        Self {
            source: ReplayTurnSource::Checkpoint { checkpoint_seq },
            turn_index,
            block_count,
        }
    }

    pub const fn turn_id(self) -> TurnId {
        match self.source {
            ReplayTurnSource::Event { event_seq } => {
                TurnId::from_replay(event_seq, self.turn_index)
            }
            ReplayTurnSource::Checkpoint { checkpoint_seq } => {
                TurnId::from_checkpoint(checkpoint_seq, self.turn_index)
            }
        }
    }

    pub const fn block_id(self, block_index: u64) -> BlockId {
        match self.source {
            ReplayTurnSource::Event { event_seq } => {
                BlockId::from_replay(event_seq, self.turn_index, block_index)
            }
            ReplayTurnSource::Checkpoint { checkpoint_seq } => {
                BlockId::from_checkpoint(checkpoint_seq, self.turn_index, block_index)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderableBlockIdentity {
    pub id: BlockId,
    pub turn_id: TurnId,
    pub block_index: u64,
}

impl RenderableBlockIdentity {
    pub const fn id(self) -> BlockId {
        self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnIdentity {
    id: TurnId,
    blocks: Vec<RenderableBlockIdentity>,
}

impl TurnIdentity {
    pub fn id(&self) -> TurnId {
        self.id
    }

    pub fn blocks(&self) -> &[RenderableBlockIdentity] {
        &self.blocks
    }

    fn from_replay(descriptor: ReplayTurn) -> Result<Self, IdentityError> {
        let block_count = usize::try_from(descriptor.block_count)
            .map_err(|_| IdentityError::BlockCountTooLarge(descriptor.block_count))?;
        let id = descriptor.turn_id();
        let mut blocks = Vec::with_capacity(block_count);
        for block_index in 0..descriptor.block_count {
            blocks.push(RenderableBlockIdentity {
                id: descriptor.block_id(block_index),
                turn_id: id,
                block_index,
            });
        }
        Ok(Self { id, blocks })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptIdentity {
    turns: Vec<TurnIdentity>,
}

impl TranscriptIdentity {
    pub fn from_replay(
        descriptors: impl IntoIterator<Item = ReplayTurn>,
    ) -> Result<Self, IdentityError> {
        let mut turn_ids = HashSet::new();
        let mut block_ids = HashSet::new();
        let mut turns = Vec::new();
        for descriptor in descriptors {
            let turn = TurnIdentity::from_replay(descriptor)?;
            if !turn_ids.insert(turn.id()) {
                return Err(IdentityError::DuplicateTurn(turn.id()));
            }
            for block in turn.blocks() {
                if !block_ids.insert(block.id) {
                    return Err(IdentityError::DuplicateBlock(block.id));
                }
            }
            turns.push(turn);
        }
        Ok(Self { turns })
    }

    pub fn turns(&self) -> &[TurnIdentity] {
        &self.turns
    }

    pub fn contains(&self, turn_id: TurnId, block_id: BlockId) -> bool {
        self.turns.iter().any(|turn| {
            turn.id() == turn_id && turn.blocks().iter().any(|block| block.id == block_id)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    BlockCountTooLarge(u64),
    DuplicateTurn(TurnId),
    DuplicateBlock(BlockId),
}

impl Display for IdentityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlockCountTooLarge(count) => {
                write!(formatter, "block count is too large: {count}")
            }
            Self::DuplicateTurn(id) => write!(formatter, "duplicate turn identity: {id}"),
            Self::DuplicateBlock(id) => write!(formatter, "duplicate block identity: {id}"),
        }
    }
}

impl std::error::Error for IdentityError {}
