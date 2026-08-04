use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AtomBoundary {
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomCursor {
    pub atom_index: usize,
    pub boundary: AtomBoundary,
}

impl AtomCursor {
    pub const fn before(atom_index: usize) -> Self {
        Self {
            atom_index,
            boundary: AtomBoundary::Before,
        }
    }

    pub const fn after(atom_index: usize) -> Self {
        Self {
            atom_index,
            boundary: AtomBoundary::After,
        }
    }

    pub const fn start() -> Self {
        Self::before(0)
    }

    pub const fn insertion_index(self) -> usize {
        match self.boundary {
            AtomBoundary::Before => self.atom_index,
            AtomBoundary::After => self.atom_index + 1,
        }
    }
}

impl std::fmt::Display for AtomCursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "atom {} {}", self.atom_index, self.boundary)
    }
}

impl std::fmt::Display for AtomBoundary {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Before => formatter.write_str("before"),
            Self::After => formatter.write_str("after"),
        }
    }
}
