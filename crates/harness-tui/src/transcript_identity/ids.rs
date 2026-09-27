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
