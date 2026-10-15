use super::{
    context::{Context, Entry},
    *,
};
use harness_providers::{AssistantToolCall, CompletionMessage, CompletionRequest, MessageRole};
use std::collections::{HashMap, HashSet};
pub(super) const INTERRUPTED_TOOL_RESULT: &str =
    "Execution was interrupted. Its outcome is unknown; inspect current state before retrying.";
mod boundary;
mod edits;
mod fold;
mod provenance;
mod restore;
pub(in crate::coord) use crate::transcript_projection::{
    latest_compaction, reader_view, ActiveCompaction,
};
pub(in crate::coord) use boundary::{
    compaction_entry, history_rewrite_at_ms, materialize_patches, native_payload,
};
pub(in crate::coord) use edits::apply_edits;
pub(in crate::coord) use restore::derive_images;
use restore::*;

/// Provider context from settled records. Provider-native replay state is not attached, so a
/// native replacement-history compaction falls back to the previous readable boundary.
pub(super) fn messages(
    events: &[EventEnvelopeV1],
    agent: &str,
    primary: bool,
    system_prompt: &str,
    run_dir: &std::path::Path,
) -> Result<Context, CoordinatorError> {
    rebuild(events, agent, primary, system_prompt, run_dir, None)
}

/// Provider context from settled records, grouped by turn rather than arrival time. With a
/// `request` naming the live provider and model, a stored native payload that belongs to
/// them is imported and replays instead of the originals it already covers.
pub(super) fn rebuild(
    events: &[EventEnvelopeV1],
    agent: &str,
    primary: bool,
    system_prompt: &str,
    run_dir: &std::path::Path,
    request: Option<&CompletionRequest>,
) -> Result<Context, CoordinatorError> {
    let events = crate::conversation_rewind::active_events(events);
    let fold::Folded {
        mut entries,
        compactions,
        tool_ids,
        contexts,
    } = fold::fold(&events, agent, primary);
    let mut output = Context::new(system_prompt);
    output.unavailable = Some(crate::subagent::FinalizedStateUnavailable::LegacySummaryOnly);
    let mut recovered = None;
    let mut summary = None;
    if let Some(selected) = boundary::select(&compactions, request, run_dir) {
        let recorded = &compactions[selected.index];
        recovered = recover_request(&entries, recorded, selected.start);
        entries.retain(|entry| entry.seq >= selected.start);
        summary = Some((recorded, selected.payload));
    }
    apply_pruned_events(&mut entries, &events, agent, &tool_ids);
    if let Some((recorded, payload)) = summary {
        let rewrite_at = history_rewrite_at_ms(&entries, 0);
        output
            .entries
            .push(boundary::summary(recorded, payload, rewrite_at));
        boundary::seed_reader_views(&output.render_caches, recorded.seq, &recorded.patches);
    }
    output.entries.extend(entries);
    derive_images(&mut output.entries);
    super::prompt::restore_content(&mut output, run_dir, &contexts)?;
    if let Some(mut request) = recovered {
        super::prompt::restore_entries(std::slice::from_mut(&mut request), run_dir, &contexts)?;
        super::compaction::notes::insert_recovered_request(&mut output, request);
    }
    if let Some(revision) = super::compaction::notes::latest_notebook(&events, agent) {
        super::compaction::notes::inject_notebook(&mut output, &revision);
    }
    Ok(output)
}

/// The newest authoritative request before a notes rollover that the kept suffix does not
/// already hold; it is appended after the summary.
fn recover_request(
    originals: &[Entry],
    recorded: &fold::Recorded<'_>,
    start: u64,
) -> Option<Entry> {
    if !super::compaction::notes::is_rollover(recorded.event.details.as_ref()) {
        return None;
    }
    let before: Vec<Entry> = originals
        .iter()
        .filter(|entry| entry.seq < recorded.seq && entry.message.role == MessageRole::User)
        .cloned()
        .collect();
    let first_kept = before
        .iter()
        .position(|entry| entry.seq >= start)
        .unwrap_or(before.len());
    let index = super::compaction::notes::recover_authoritative_request(&before, first_kept)?;
    before.into_iter().nth(index)
}

/// Every settled entry of the agent in journal order, ignoring all compactions. Attachment
/// bytes are not loaded; see [`hydrate_original`].
pub(in crate::coord) fn original_entries(
    events: &[EventEnvelopeV1],
    agent: &str,
    primary: bool,
) -> Vec<Entry> {
    let events = crate::conversation_rewind::active_events(events);
    let fold::Folded {
        mut entries,
        tool_ids,
        ..
    } = fold::fold(&events, agent, primary);
    apply_pruned_events(&mut entries, &events, agent, &tool_ids);
    derive_images(&mut entries);
    entries
}

/// Loads the attachment bytes and selected context of one entry from [`original_entries`].
pub(in crate::coord) fn hydrate_original(
    entry: &mut Entry,
    events: &[EventEnvelopeV1],
    run_dir: &std::path::Path,
) -> Result<(), CoordinatorError> {
    let contexts: HashMap<String, &ArtifactWrittenEvent> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::ArtifactWritten(written) => written
                .metadata
                .get("prompt_context")
                .map(|request| (request.clone(), written)),
            _ => None,
        })
        .collect();
    super::prompt::restore_entries(std::slice::from_mut(entry), run_dir, &contexts)
}
