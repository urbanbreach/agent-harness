use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SuggestionGeneration(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalidation {
    Edit,
    FocusChange,
    StateChange,
    Cancellation,
    PartialAcceptance,
    FullAcceptance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationError {
    Exhausted,
}

impl SuggestionGeneration {
    pub const fn new() -> Self {
        Self(0)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn bump(&mut self, _reason: Invalidation) -> Result<Self, GenerationError> {
        self.0 = self.0.checked_add(1).ok_or(GenerationError::Exhausted)?;
        Ok(*self)
    }
}

impl Display for GenerationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exhausted => formatter.write_str("suggestion generation counter exhausted"),
        }
    }
}

impl std::error::Error for GenerationError {}
