use super::CompletionError;

/// The four completion providers understood by the composer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CompletionSource {
    Slash,
    File,
    Shell,
    History,
}

/// A half-open atom-index range in the composer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionRange {
    pub start: usize,
    pub end: usize,
}

impl CompletionRange {
    /// Creates a range that never splits an atom.
    pub fn new(start: usize, end: usize) -> Result<Self, CompletionError> {
        if start > end {
            return Err(CompletionError::InvalidRange { start, end });
        }
        Ok(Self { start, end })
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// A parsed completion trigger and its current query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionTrigger {
    pub range: CompletionRange,
    pub query: String,
    pub source: CompletionSource,
}

impl CompletionTrigger {
    pub fn new(range: CompletionRange, query: impl Into<String>, source: CompletionSource) -> Self {
        Self {
            range,
            query: query.into(),
            source,
        }
    }
}
