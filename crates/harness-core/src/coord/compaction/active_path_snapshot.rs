use crate::session::{select_provider_active_path, ProviderViewError};

use super::snapshot::{
    ActiveCompactionBranch, ActivePathCompactionSnapshot, ActivePathCompactionSnapshotInput,
    CompactionSnapshotEntry, CompactionSnapshotError, PriorActiveCompactionSummary,
};

/// Projects the canonical active branch into one protocol-safe compaction snapshot.
///
/// Branch summaries and historical compaction entries are excluded from `entries`; the latest
/// active compaction summary is retained separately. Only unique complete tool pairs survive.
///
/// # Errors
/// Returns [`CompactionSnapshotError`] for malformed ancestry or owner identity mismatch.
pub fn build_active_path_compaction_snapshot(
    input: ActivePathCompactionSnapshotInput<'_>,
) -> Result<ActivePathCompactionSnapshot, CompactionSnapshotError> {
    let selected = select_provider_active_path(input.session, &input.owner, None).map_err(
        |error| match error {
            ProviderViewError::InvalidSession(error) => {
                CompactionSnapshotError::InvalidSession(error)
            }
            ProviderViewError::OwnerSessionMismatch { expected, actual } => {
                CompactionSnapshotError::OwnerSessionMismatch { expected, actual }
            }
            other => CompactionSnapshotError::InvalidProviderView(other),
        },
    )?;
    let prior_active_summary =
        selected
            .latest_compaction_summary
            .map(|summary| PriorActiveCompactionSummary {
                legacy_source_sequence: input
                    .legacy_source_sequences
                    .sequence_for(&summary.entry_id),
                entry_id: summary.entry_id,
                summary: summary.summary,
                first_kept_entry_id: summary.first_kept_entry_id,
            });
    let entries = selected
        .entries
        .into_iter()
        .map(|entry| CompactionSnapshotEntry {
            legacy_source_sequence: input.legacy_source_sequences.sequence_for(&entry.id),
            tool_pairs: selected
                .tool_pairs
                .iter()
                .filter(|pair| {
                    pair.assistant_entry_id == entry.id || pair.result_entry_id == entry.id
                })
                .cloned()
                .collect(),
            entry,
        })
        .collect();
    Ok(ActivePathCompactionSnapshot {
        owner: input.owner,
        active_branch: ActiveCompactionBranch {
            leaf_entry_id: Some(selected.selected_leaf),
            entry_ids: selected.active_entry_ids,
        },
        entries,
        pending_prompt: input.pending_prompt,
        prior_active_summary,
        current_model: input.current_model,
    })
}
