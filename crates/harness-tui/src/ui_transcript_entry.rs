use std::borrow::Borrow;
use std::ops::{Deref, DerefMut};

use super::ui_transcript_block_grammar::{
    TranscriptBlockContent, TranscriptBlockRole, TranscriptBlockSpec, TranscriptLifecycleState,
    TranscriptPromptState, TranscriptToolFamily, TranscriptToolStatus,
};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum TranscriptVisualEntryId {
    User {
        activity_first_seq: u64,
    },
    Part {
        activity_first_seq: u64,
        semantic_key: u64,
    },
    ToolGroup {
        activity_first_seq: u64,
        semantic_key: u64,
    },
    Footer {
        activity_first_seq: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum TranscriptVisualEntryGroup {
    Standalone,
    ToolRun(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptVisualEntryDisplayMode {
    Flow,
    Compact,
    StickyPrompt,
    PinnedFooter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptVisualEntryLifecycle {
    Settled,
    Active,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptVisualEntryAccent {
    Hidden,
    Active,
    Selected,
    Animated(ToolRailMotion),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptVisualEntryMetadata {
    pub(in crate::ui) id: TranscriptVisualEntryId,
    pub(in crate::ui) kind: TranscriptRenderSurfaceKind,
    pub(in crate::ui) group: TranscriptVisualEntryGroup,
    pub(in crate::ui) display_mode: TranscriptVisualEntryDisplayMode,
    pub(in crate::ui) lifecycle: TranscriptVisualEntryLifecycle,
    pub(in crate::ui) accent: TranscriptVisualEntryAccent,
}

impl TranscriptVisualEntryMetadata {
    #[cfg(test)]
    pub(in crate::ui) fn settled(
        activity_first_seq: u64,
        ordinal: usize,
        kind: TranscriptRenderSurfaceKind,
        display_mode: TranscriptVisualEntryDisplayMode,
    ) -> Self {
        Self {
            id: TranscriptVisualEntryId::Part {
                activity_first_seq,
                semantic_key: u64::try_from(ordinal).unwrap_or(u64::MAX),
            },
            kind,
            group: TranscriptVisualEntryGroup::Standalone,
            display_mode,
            lifecycle: TranscriptVisualEntryLifecycle::Settled,
            accent: TranscriptVisualEntryAccent::Hidden,
        }
    }

    pub(in crate::ui) fn from_spec(
        activity_first_seq: u64,
        spec: &TranscriptBlockSpec,
        draft: &TranscriptVisualEntryDraft,
    ) -> Self {
        let tool_key = match &spec.content {
            TranscriptBlockContent::Tool { ids, .. } => {
                Some(semantic_key(ids.iter().map(String::as_str)))
            }
            TranscriptBlockContent::UserMessage { .. }
            | TranscriptBlockContent::AssistantBody { .. }
            | TranscriptBlockContent::Reasoning { .. }
            | TranscriptBlockContent::Footer { .. }
            | TranscriptBlockContent::Error { .. }
            | TranscriptBlockContent::Compaction { .. } => None,
            #[cfg(test)]
            TranscriptBlockContent::Synthetic { .. } => None,
        };
        let id = match spec.role {
            TranscriptBlockRole::UserPrompt => TranscriptVisualEntryId::User { activity_first_seq },
            TranscriptBlockRole::Footer => TranscriptVisualEntryId::Footer { activity_first_seq },
            TranscriptBlockRole::Tool
                if matches!(
                    &spec.content,
                    TranscriptBlockContent::Tool {
                        family: TranscriptToolFamily::Group,
                        ..
                    }
                ) =>
            {
                TranscriptVisualEntryId::ToolGroup {
                    activity_first_seq,
                    semantic_key: tool_key.unwrap_or_else(|| semantic_key([spec.id.0.as_str()])),
                }
            }
            TranscriptBlockRole::AssistantBody
            | TranscriptBlockRole::Reasoning
            | TranscriptBlockRole::Tool
            | TranscriptBlockRole::Error
            | TranscriptBlockRole::Compaction => TranscriptVisualEntryId::Part {
                activity_first_seq,
                semantic_key: tool_key.unwrap_or_else(|| semantic_key([spec.id.0.as_str()])),
            },
            #[cfg(test)]
            TranscriptBlockRole::Synthetic => TranscriptVisualEntryId::Part {
                activity_first_seq,
                semantic_key: semantic_key([spec.id.0.as_str()]),
            },
        };
        let group = match tool_key {
            Some(key) => TranscriptVisualEntryGroup::ToolRun(key),
            None => TranscriptVisualEntryGroup::Standalone,
        };
        let display_mode = match draft.placement {
            TranscriptBlockPlacement::StickyPromptCandidate => {
                TranscriptVisualEntryDisplayMode::StickyPrompt
            }
            TranscriptBlockPlacement::PinnedFooter { .. } => {
                TranscriptVisualEntryDisplayMode::PinnedFooter
            }
            TranscriptBlockPlacement::Flow
                if matches!(
                    draft.kind,
                    TranscriptRenderSurfaceKind::AssistantTool
                        | TranscriptRenderSurfaceKind::AssistantCommandTool
                ) =>
            {
                TranscriptVisualEntryDisplayMode::Compact
            }
            TranscriptBlockPlacement::Flow => TranscriptVisualEntryDisplayMode::Flow,
        };
        let lifecycle = match &spec.content {
            TranscriptBlockContent::UserMessage { queued, state, .. } => {
                if *queued || matches!(state, TranscriptPromptState::ActiveThinking) {
                    TranscriptVisualEntryLifecycle::Active
                } else {
                    TranscriptVisualEntryLifecycle::Settled
                }
            }
            TranscriptBlockContent::AssistantBody { streaming, .. }
            | TranscriptBlockContent::Reasoning {
                active: streaming, ..
            } => {
                if *streaming {
                    TranscriptVisualEntryLifecycle::Active
                } else {
                    TranscriptVisualEntryLifecycle::Settled
                }
            }
            TranscriptBlockContent::Tool { policy, .. } => match policy.status {
                TranscriptToolStatus::Queued
                | TranscriptToolStatus::Running
                | TranscriptToolStatus::Waiting => TranscriptVisualEntryLifecycle::Active,
                TranscriptToolStatus::Failed | TranscriptToolStatus::Cancelled => {
                    TranscriptVisualEntryLifecycle::Failed
                }
                TranscriptToolStatus::Succeeded => TranscriptVisualEntryLifecycle::Settled,
            },
            TranscriptBlockContent::Footer { state, .. } => match state {
                TranscriptLifecycleState::Queued
                | TranscriptLifecycleState::Responding
                | TranscriptLifecycleState::Retrying { .. } => {
                    TranscriptVisualEntryLifecycle::Active
                }
                TranscriptLifecycleState::Cancelled | TranscriptLifecycleState::Failed => {
                    TranscriptVisualEntryLifecycle::Failed
                }
                TranscriptLifecycleState::Recovered | TranscriptLifecycleState::Completed => {
                    TranscriptVisualEntryLifecycle::Settled
                }
            },
            TranscriptBlockContent::Error { .. } => TranscriptVisualEntryLifecycle::Failed,
            TranscriptBlockContent::Compaction { .. } => TranscriptVisualEntryLifecycle::Settled,
            #[cfg(test)]
            TranscriptBlockContent::Synthetic { .. } => TranscriptVisualEntryLifecycle::Settled,
        };
        let accent = if draft.selected_rail {
            TranscriptVisualEntryAccent::Selected
        } else if let Some(motion) = draft.tool_rail_motion {
            TranscriptVisualEntryAccent::Animated(motion)
        } else if draft.show_outer_rail {
            TranscriptVisualEntryAccent::Active
        } else {
            TranscriptVisualEntryAccent::Hidden
        };
        Self {
            id,
            kind: draft.kind,
            group,
            display_mode,
            lifecycle,
            accent,
        }
    }
}

pub(in crate::ui) fn semantic_key<'a>(values: impl IntoIterator<Item = &'a str>) -> u64 {
    values
        .into_iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, value| {
            value.as_bytes().iter().fold(hash, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        })
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct ResolvedTranscriptVisualEntryDraft {
    pub(in crate::ui) metadata: TranscriptVisualEntryMetadata,
    pub(in crate::ui) draft: TranscriptVisualEntryDraft,
}

impl Deref for ResolvedTranscriptVisualEntryDraft {
    type Target = TranscriptVisualEntryDraft;

    fn deref(&self) -> &Self::Target {
        &self.draft
    }
}

impl DerefMut for ResolvedTranscriptVisualEntryDraft {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.draft
    }
}

impl Borrow<TranscriptVisualEntryDraft> for ResolvedTranscriptVisualEntryDraft {
    fn borrow(&self) -> &TranscriptVisualEntryDraft {
        &self.draft
    }
}

pub(in crate::ui) trait IntoResolvedTranscriptVisualEntryDraft {
    fn into_resolved(
        self,
        activity_first_seq: u64,
        ordinal: usize,
    ) -> ResolvedTranscriptVisualEntryDraft;
}

impl IntoResolvedTranscriptVisualEntryDraft for ResolvedTranscriptVisualEntryDraft {
    fn into_resolved(
        self,
        _activity_first_seq: u64,
        _ordinal: usize,
    ) -> ResolvedTranscriptVisualEntryDraft {
        self
    }
}

#[cfg(test)]
impl IntoResolvedTranscriptVisualEntryDraft for TranscriptVisualEntryDraft {
    fn into_resolved(
        self,
        activity_first_seq: u64,
        ordinal: usize,
    ) -> ResolvedTranscriptVisualEntryDraft {
        let display_mode = match self.placement {
            TranscriptBlockPlacement::Flow => TranscriptVisualEntryDisplayMode::Flow,
            TranscriptBlockPlacement::StickyPromptCandidate => {
                TranscriptVisualEntryDisplayMode::StickyPrompt
            }
            TranscriptBlockPlacement::PinnedFooter { .. } => {
                TranscriptVisualEntryDisplayMode::PinnedFooter
            }
        };
        let mut metadata = TranscriptVisualEntryMetadata::settled(
            activity_first_seq,
            ordinal,
            self.kind,
            display_mode,
        );
        metadata.lifecycle = if self.kind == TranscriptRenderSurfaceKind::AssistantError {
            TranscriptVisualEntryLifecycle::Failed
        } else if self.show_outer_rail || self.tool_rail_motion.is_some() {
            TranscriptVisualEntryLifecycle::Active
        } else {
            TranscriptVisualEntryLifecycle::Settled
        };
        metadata.accent = if self.selected_rail {
            TranscriptVisualEntryAccent::Selected
        } else if let Some(motion) = self.tool_rail_motion {
            TranscriptVisualEntryAccent::Animated(motion)
        } else if self.show_outer_rail {
            TranscriptVisualEntryAccent::Active
        } else {
            TranscriptVisualEntryAccent::Hidden
        };
        ResolvedTranscriptVisualEntryDraft {
            metadata,
            draft: self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptVisualEntryHitRegion {
    pub(in crate::ui) top_row: usize,
    pub(in crate::ui) left_column: u16,
    pub(in crate::ui) width: u16,
    pub(in crate::ui) height: usize,
}

impl TranscriptVisualEntryHitRegion {
    #[cfg(test)]
    pub(in crate::ui) const fn new(top_row: usize, width: u16, height: usize) -> Self {
        Self {
            top_row,
            left_column: 0,
            width,
            height,
        }
    }
}
