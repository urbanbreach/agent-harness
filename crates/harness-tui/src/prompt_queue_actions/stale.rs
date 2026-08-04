use std::fmt;

use super::state::QueueState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleError {
    MissingQueuedId(String),
}

impl fmt::Display for StaleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingQueuedId(id) => write!(formatter, "queued prompt id is stale: {id}"),
        }
    }
}

impl std::error::Error for StaleError {}

pub fn reject_stale(state: &QueueState, queued_id: &str) -> Result<(), StaleError> {
    if state.queued.iter().any(|item| item.id == queued_id) {
        Ok(())
    } else {
        Err(StaleError::MissingQueuedId(queued_id.to_owned()))
    }
}
