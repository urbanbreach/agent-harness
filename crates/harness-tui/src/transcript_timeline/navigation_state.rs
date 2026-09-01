use std::fmt::{Display, Formatter};

use crate::transcript_identity::{IdentityError, TranscriptIdentity, TurnId};

use super::markers::TimelineTurn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollAnchor {
    pub turn_id: TurnId,
    pub row_offset: usize,
}

impl ScrollAnchor {
    pub fn capture(turn: &TimelineTurn, scroll_top: usize) -> Self {
        Self {
            turn_id: turn.turn_id(),
            row_offset: turn.row.saturating_sub(scroll_top),
        }
    }

    pub fn resolve(
        self,
        identity: &TranscriptIdentity,
        turns: &[TimelineTurn],
    ) -> Result<usize, TimelineNavigationError> {
        if !identity
            .turns()
            .iter()
            .any(|turn| turn.id() == self.turn_id)
        {
            return Err(TimelineNavigationError::AnchorIdentityMismatch(
                self.turn_id,
            ));
        }
        let Some(turn) = turns.iter().find(|turn| turn.turn_id() == self.turn_id) else {
            return Err(TimelineNavigationError::AnchorTurnMissing(self.turn_id));
        };
        Ok(turn.row.saturating_sub(self.row_offset))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponsePosition {
    pub index: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineNavigationSnapshot {
    pub selected_turn_id: Option<TurnId>,
    pub scroll_top: usize,
    pub anchor: Option<ScrollAnchor>,
    pub response_position: Option<ResponsePosition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineNavigationError {
    Identity(IdentityError),
    TurnIdentityMismatch(TurnId),
    SelectedTurnMissing(TurnId),
    AnchorIdentityMismatch(TurnId),
    AnchorTurnMissing(TurnId),
}

impl Display for TimelineNavigationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Identity(error) => write!(formatter, "timeline identity is invalid: {error}"),
            Self::TurnIdentityMismatch(turn_id) => {
                write!(
                    formatter,
                    "timeline turn is absent from identity: {turn_id}"
                )
            }
            Self::SelectedTurnMissing(turn_id) => {
                write!(formatter, "selected timeline turn is missing: {turn_id}")
            }
            Self::AnchorIdentityMismatch(turn_id) => {
                write!(formatter, "timeline anchor identity is missing: {turn_id}")
            }
            Self::AnchorTurnMissing(turn_id) => {
                write!(formatter, "timeline anchor turn is missing: {turn_id}")
            }
        }
    }
}

impl std::error::Error for TimelineNavigationError {}

impl From<IdentityError> for TimelineNavigationError {
    fn from(error: IdentityError) -> Self {
        Self::Identity(error)
    }
}
