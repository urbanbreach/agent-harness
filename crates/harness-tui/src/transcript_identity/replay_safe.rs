use std::fmt::{Display, Formatter};

use super::ids::{BlockId, ReplayTurnSource, TurnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReplayIdentityKey {
    pub source: ReplayTurnSource,
    pub turn_index: u64,
    pub block_index: Option<u64>,
}

impl ReplayIdentityKey {
    pub const fn event(event_seq: u64, turn_index: u64) -> Self {
        Self {
            source: ReplayTurnSource::Event { event_seq },
            turn_index,
            block_index: None,
        }
    }

    pub const fn checkpoint(checkpoint_seq: u64, turn_index: u64) -> Self {
        Self {
            source: ReplayTurnSource::Checkpoint { checkpoint_seq },
            turn_index,
            block_index: None,
        }
    }

    pub const fn block_event(event_seq: u64, turn_index: u64, block_index: u64) -> Self {
        Self {
            block_index: Some(block_index),
            ..Self::event(event_seq, turn_index)
        }
    }

    pub const fn block_checkpoint(checkpoint_seq: u64, turn_index: u64, block_index: u64) -> Self {
        Self {
            block_index: Some(block_index),
            ..Self::checkpoint(checkpoint_seq, turn_index)
        }
    }
}

pub fn assert_turn_id(key: ReplayIdentityKey, actual: TurnId) -> Result<(), ReplaySafetyError> {
    let expected = match key.source {
        ReplayTurnSource::Event { event_seq } => TurnId::from_replay(event_seq, key.turn_index),
        ReplayTurnSource::Checkpoint { checkpoint_seq } => {
            TurnId::from_checkpoint(checkpoint_seq, key.turn_index)
        }
    };
    if expected == actual {
        Ok(())
    } else {
        Err(ReplaySafetyError::TurnMismatch { expected, actual })
    }
}

pub fn assert_block_id(key: ReplayIdentityKey, actual: BlockId) -> Result<(), ReplaySafetyError> {
    let Some(block_index) = key.block_index else {
        return Err(ReplaySafetyError::MissingBlockIndex);
    };
    let expected = match key.source {
        ReplayTurnSource::Event { event_seq } => {
            BlockId::from_replay(event_seq, key.turn_index, block_index)
        }
        ReplayTurnSource::Checkpoint { checkpoint_seq } => {
            BlockId::from_checkpoint(checkpoint_seq, key.turn_index, block_index)
        }
    };
    if expected == actual {
        Ok(())
    } else {
        Err(ReplaySafetyError::BlockMismatch { expected, actual })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaySafetyError {
    MissingBlockIndex,
    TurnMismatch { expected: TurnId, actual: TurnId },
    BlockMismatch { expected: BlockId, actual: BlockId },
}

impl Display for ReplaySafetyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBlockIndex => formatter.write_str("block identity key has no block index"),
            Self::TurnMismatch { expected, actual } => {
                write!(
                    formatter,
                    "turn identity mismatch: expected {expected}, got {actual}"
                )
            }
            Self::BlockMismatch { expected, actual } => {
                write!(
                    formatter,
                    "block identity mismatch: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for ReplaySafetyError {}
