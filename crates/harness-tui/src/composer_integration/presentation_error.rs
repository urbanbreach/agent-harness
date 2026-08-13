#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerPresentationError {
    InvalidAtoms(crate::composer_atoms::AtomBufferError),
    ZeroWrapWidth,
    ZeroViewportLines,
    ZeroAvailableRows,
}

impl std::fmt::Display for ComposerPresentationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAtoms(error) => std::fmt::Display::fmt(error, formatter),
            Self::ZeroWrapWidth => formatter.write_str("composer wrap width must be positive"),
            Self::ZeroViewportLines => {
                formatter.write_str("composer viewport line limit must be positive")
            }
            Self::ZeroAvailableRows => {
                formatter.write_str("composer presentation requires at least one row")
            }
        }
    }
}

impl std::error::Error for ComposerPresentationError {}

impl From<crate::composer_atoms::AtomBufferError> for ComposerPresentationError {
    fn from(error: crate::composer_atoms::AtomBufferError) -> Self {
        Self::InvalidAtoms(error)
    }
}
