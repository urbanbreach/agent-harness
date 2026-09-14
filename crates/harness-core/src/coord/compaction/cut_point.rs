//! Typed compaction cut planning plus event-level turn boundaries.

#[path = "cut_point/safe.rs"]
mod safe;
#[path = "cut_point/turn_boundary.rs"]
mod turn_boundary;

pub(crate) use safe::SafeCutError;
pub use turn_boundary::{find_cut_point, CutPointResult};

use crate::ids::EntryId;
use crate::session::{AssistantPart, SessionEntryPayload};

use super::snapshot::{ActivePathCompactionSnapshot, CompactionSnapshotEntry};
use super::tokens::estimate_text_tokens;
use safe::{plan_safe_cut, SafeCutCandidate};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedCutPointPlan {
    pub(crate) first_kept_entry_id: EntryId,
    pub(crate) retained_tokens: u32,
    pub(crate) summarized_tokens: u32,
}

pub(crate) fn find_safe_cut_point(
    snapshot: &ActivePathCompactionSnapshot,
    keep_recent_tokens: u32,
    force_progress: bool,
) -> Result<TypedCutPointPlan, SafeCutError> {
    let candidates = snapshot
        .entries
        .iter()
        .map(|entry| {
            let joins_previous = entry
                .tool_pairs
                .iter()
                .any(|pair| pair.result_entry_id == entry.entry.id);
            let joins_next = entry
                .tool_pairs
                .iter()
                .any(|pair| pair.assistant_entry_id == entry.entry.id);
            candidate_for_payload(&entry.entry.payload, joins_previous, joins_next)
        })
        .collect::<Vec<_>>();
    let mut plan = plan_safe_cut(&candidates, keep_recent_tokens, estimate_text_tokens)?;
    if force_progress && plan.summarized_tokens == 0 {
        if let Some(next) =
            snapshot
                .entries
                .iter()
                .enumerate()
                .skip(1)
                .find_map(|(index, entry)| {
                    matches!(
                        entry.entry.payload,
                        SessionEntryPayload::UserMessage { .. }
                            | SessionEntryPayload::AssistantMessage { .. }
                    )
                    .then_some(index)
                })
        {
            plan.first_kept_index = next;
            plan.retained_tokens = estimate_typed_entries_tokens(&snapshot.entries[next..]);
            plan.summarized_tokens = estimate_typed_entries_tokens(&snapshot.entries[..next]);
        }
    }
    let Some(first_kept) = snapshot.entries.get(plan.first_kept_index) else {
        return Err(SafeCutError::NoSafeCut);
    };
    Ok(TypedCutPointPlan {
        first_kept_entry_id: first_kept.entry.id.clone(),
        retained_tokens: plan.retained_tokens,
        summarized_tokens: plan.summarized_tokens,
    })
}

pub(crate) fn estimate_typed_entries_tokens(entries: &[CompactionSnapshotEntry]) -> u32 {
    entries.iter().fold(0_u32, |total, entry| {
        let candidate = candidate_for_payload(&entry.entry.payload, false, false);
        total.saturating_add(candidate.tokens(estimate_text_tokens))
    })
}

fn candidate_for_payload(
    payload: &SessionEntryPayload,
    joins_previous: bool,
    joins_next: bool,
) -> SafeCutCandidate<'_> {
    match payload {
        SessionEntryPayload::UserMessage { text, attachments } if attachments.is_empty() => {
            SafeCutCandidate::text(text)
        }
        SessionEntryPayload::UserMessage { text, .. } => {
            SafeCutCandidate::atomic(estimate_text_tokens(text), joins_previous, joins_next)
        }
        SessionEntryPayload::AssistantMessage { parts, .. } => match parts.as_slice() {
            [AssistantPart::Text { text }] | [AssistantPart::Reasoning { text }] => {
                SafeCutCandidate::text(text)
            }
            _ => SafeCutCandidate::atomic(
                parts.iter().fold(0_u32, |tokens, part| {
                    let part_tokens = match part {
                        AssistantPart::Text { text } | AssistantPart::Reasoning { text } => {
                            estimate_text_tokens(text)
                        }
                        AssistantPart::ToolCall(call) => estimate_text_tokens(&call.tool_id)
                            .saturating_add(estimate_text_tokens(&call.args_summary)),
                    };
                    tokens.saturating_add(part_tokens)
                }),
                joins_previous,
                joins_next,
            ),
        },
        SessionEntryPayload::ToolResult {
            output_summary,
            output_json,
            ..
        } => SafeCutCandidate::atomic(
            output_summary.as_deref().map_or_else(
                || {
                    output_json
                        .as_ref()
                        .map_or(0, |value| estimate_text_tokens(&value.to_string()))
                },
                estimate_text_tokens,
            ),
            true,
            joins_next,
        ),
        SessionEntryPayload::SystemContextUpdate { context }
        | SessionEntryPayload::CustomModelVisibleContext { context, .. } => {
            SafeCutCandidate::atomic(estimate_text_tokens(context), joins_previous, joins_next)
        }
        SessionEntryPayload::CompactionSummary { summary, .. }
        | SessionEntryPayload::BranchSummary { summary } => {
            SafeCutCandidate::atomic(estimate_text_tokens(summary), joins_previous, joins_next)
        }
        SessionEntryPayload::ModelChange { .. }
        | SessionEntryPayload::ReasoningSettingChange { .. }
        | SessionEntryPayload::CustomPersistedState { .. }
        | SessionEntryPayload::SessionMetadata { .. } => {
            SafeCutCandidate::atomic(0, joins_previous, joins_next)
        }
    }
}
