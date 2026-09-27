#[cfg(test)]
use std::borrow::Borrow;
use std::ops::{Deref, DerefMut};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TranscriptVisualEntryId {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptVisualEntryDisplayMode {
    Flow,
    Compact,
    Expanded,
    StickyPrompt,
    PinnedFooter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptVisualEntryMetadata {
    pub(in crate::ui) foldable: bool,
    pub(in crate::ui) context_group: bool,
    pub(in crate::ui) id: TranscriptVisualEntryId,
    pub(in crate::ui) display_mode: TranscriptVisualEntryDisplayMode,
}

impl TranscriptVisualEntryMetadata {
    #[cfg(test)]
    pub(in crate::ui) fn settled(
        activity_first_seq: u64,
        ordinal: usize,
        display_mode: TranscriptVisualEntryDisplayMode,
    ) -> Self {
        Self {
            foldable: false,
            context_group: false,
            id: TranscriptVisualEntryId::Part {
                activity_first_seq,
                semantic_key: u64::try_from(ordinal).unwrap_or(u64::MAX),
            },
            display_mode,
        }
    }
}

impl ResolvedTranscriptVisualEntryDraft {
    pub(super) fn new(
        id: TranscriptVisualEntryId,
        draft: TranscriptVisualEntryDraft,
        source: Option<&str>,
    ) -> Self {
        let display_mode = match draft.placement {
            TranscriptBlockPlacement::Flow => TranscriptVisualEntryDisplayMode::Flow,
            TranscriptBlockPlacement::StickyPromptCandidate => {
                TranscriptVisualEntryDisplayMode::StickyPrompt
            }
            TranscriptBlockPlacement::PinnedFooter { .. } => {
                TranscriptVisualEntryDisplayMode::PinnedFooter
            }
        };
        Self {
            metadata: TranscriptVisualEntryMetadata {
                id,
                foldable: false,
                context_group: false,
                display_mode,
            },
            source_text: source.map(std::sync::Arc::from),
            draft,
        }
    }

    pub(super) fn part(
        turn: &TranscriptTurnSection,
        index: usize,
        draft: TranscriptVisualEntryDraft,
    ) -> Self {
        let part = &turn.assistant_parts[index];
        let (role, source, foldable, expanded) = match part {
            TranscriptAssistantPart::Reasoning(reasoning) => (
                "reasoning",
                Some(reasoning.text.as_str()),
                true,
                turn.reasoning_expanded,
            ),
            TranscriptAssistantPart::Body(
                TranscriptBodyBlock::RichText(text) | TranscriptBodyBlock::StreamingRichText(text),
            ) => ("body", Some(text.as_str()), false, false),
            TranscriptAssistantPart::ToolCall(tool) => (
                "tool",
                None,
                tool.header.disclosure_state.is_some(),
                tool.header.disclosure_state == Some(TranscriptToolCallDisclosureState::Expanded)
                    || (tool_family(tool) == TranscriptToolFamily::Task
                        && tool.header.presentation.status == ToolCallPresentationStatus::Running),
            ),
            TranscriptAssistantPart::Error(error) => {
                ("error", Some(error.text.as_str()), false, false)
            }
            TranscriptAssistantPart::Compaction(compaction) => {
                ("compaction", Some(compaction.summary.as_str()), true, false)
            }
        };
        let key = if let TranscriptAssistantPart::ToolCall(tool) = part {
            semantic_key([tool.tool_call_id.as_str()])
        } else {
            let source_seq = (turn.assistant_part_source_ids.len() == turn.assistant_parts.len())
                .then(|| turn.assistant_part_source_ids[index].0);
            let id = source_seq.map_or_else(
                || {
                    format!(
                        "{}:{role}:fixture:{:016x}",
                        turn.request_id,
                        semantic_key([source.unwrap_or("")])
                    )
                },
                |seq| format!("{}:{role}:event:{seq}", turn.request_id),
            );
            semantic_key([id.as_str()])
        };
        let mut entry = Self::new(
            TranscriptVisualEntryId::Part {
                activity_first_seq: turn.activity_first_seq,
                semantic_key: key,
            },
            draft,
            source,
        );
        entry.metadata.foldable = foldable;
        if matches!(
            part,
            TranscriptAssistantPart::ToolCall(_) | TranscriptAssistantPart::Reasoning(_)
        ) {
            entry.metadata.display_mode = if expanded {
                TranscriptVisualEntryDisplayMode::Expanded
            } else {
                TranscriptVisualEntryDisplayMode::Compact
            };
        }
        entry
    }

    pub(super) fn group(
        turn: &TranscriptTurnSection,
        group: &super::ui_transcript_groups::TranscriptToolGroup,
        draft: TranscriptVisualEntryDraft,
    ) -> Self {
        let mut entry = Self::new(
            TranscriptVisualEntryId::ToolGroup {
                activity_first_seq: turn.activity_first_seq,
                semantic_key: semantic_key(group.target_ids.iter().take(1).map(String::as_str)),
            },
            draft,
            None,
        );
        entry.metadata.foldable =
            turn.assistant_parts[group.start]
                .tool_call()
                .is_none_or(|tool| {
                    tool.header.visual_style != TranscriptToolCallVisualStyle::TaskInline
                });
        entry.metadata.context_group = group.summary.kind == TranscriptToolGroupKind::Context;
        entry.metadata.display_mode =
            if group.summary.disclosure == TranscriptToolDisclosureMode::Expanded {
                TranscriptVisualEntryDisplayMode::Expanded
            } else {
                TranscriptVisualEntryDisplayMode::Compact
            };
        entry
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
    pub(in crate::ui) source_text: Option<std::sync::Arc<str>>,
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

#[cfg(test)]
impl Borrow<TranscriptVisualEntryDraft> for ResolvedTranscriptVisualEntryDraft {
    fn borrow(&self) -> &TranscriptVisualEntryDraft {
        &self.draft
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
