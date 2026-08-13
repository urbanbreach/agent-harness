use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockSpec {
    pub(in crate::ui) id: TranscriptBlockId,
    pub(in crate::ui) role: TranscriptBlockRole,
    pub(in crate::ui) content: TranscriptBlockContent,
    pub(in crate::ui) chrome: TranscriptBlockChrome,
    pub(in crate::ui) spacing: TranscriptBlockSpacing,
    pub(in crate::ui) grouping: TranscriptBlockGrouping,
    pub(in crate::ui) fold: TranscriptBlockFold,
    pub(in crate::ui) interaction: TranscriptBlockInteraction,
    pub(in crate::ui) disclosure: TranscriptBlockDisclosure,
    pub(in crate::ui) compact: TranscriptBlockCompactPolicy,
    pub(in crate::ui) placement: TranscriptBlockPlacement,
    pub(in crate::ui) motion: TranscriptBlockMotionDemand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(in crate::ui) enum TranscriptGrammarError {
    #[error("selected transcript block is not selectable")]
    InvalidInteraction,
    #[error("transcript motion requires accent chrome")]
    InvalidMotion,
    #[error("transcript placement does not match its role")]
    InvalidPlacement,
    #[error("transcript disclosure state is contradictory")]
    InvalidDisclosure,
    #[error("transcript group members require a stable group id")]
    InvalidGrouping,
    #[error("resolved transcript row arrays are not aligned")]
    RowMismatch,
}
