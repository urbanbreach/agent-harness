//! Choosing the compaction that defines a restored context, and restoring its entry.
use super::{fold::Recorded, *};
use crate::coord::compaction::native_store::read_native_payload;
use crate::coord::context::{
    archive::{archive_frame_attachments, import_archive},
    summary_entry, CompactionState, RenderCaches,
};
use crate::event::{CompactionPatch, CompactionPatchedEvent, NativeCompactionMeta};
use harness_providers::{CompletionRequest, ProviderContinuation};

/// The compaction restored context starts from.
pub(super) struct Boundary {
    pub index: usize,
    /// Replay state imported for the live provider, model and protocol.
    pub payload: Option<ProviderContinuation>,
    /// Originals with a sequence at or after this one stay in the context.
    pub start: u64,
}

/// The newest compaction whose summary the live reader can use. A provider-native
/// replacement-history compaction counts only when its stored payload loads for `request`;
/// otherwise the previous readable boundary applies and the originals it kept return.
pub(super) fn select(
    compactions: &[Recorded<'_>],
    request: Option<&CompletionRequest>,
    run_dir: &std::path::Path,
) -> Option<Boundary> {
    for (index, recorded) in compactions.iter().enumerate().rev() {
        let native = recorded
            .event
            .native
            .as_ref()
            .filter(|n| n.lane != "custom");
        let payload = native.and_then(|meta| native_payload(meta, request, run_dir));
        if native.is_some_and(replaces_history) && payload.is_none() {
            continue;
        }
        let start = retained_from(recorded, payload.is_some());
        return Some(Boundary {
            index,
            payload,
            start,
        });
    }
    None
}

fn replaces_history(meta: &NativeCompactionMeta) -> bool {
    meta.lane.starts_with("openai_")
}

/// The stored native payload of a compaction when it belongs to `request`'s provider,
/// model and protocol; anything else is unreadable.
pub(in crate::coord) fn native_payload(
    meta: &NativeCompactionMeta,
    request: Option<&CompletionRequest>,
    run_dir: &std::path::Path,
) -> Option<ProviderContinuation> {
    let payload = read_native_payload(run_dir, meta.payload.as_ref()?, request?)?;
    meta.api
        .as_deref()
        .is_none_or(|api| api == payload.protocol().as_str())
        .then_some(payload)
}

/// First original sequence that is not already inside the compaction.
fn retained_from(recorded: &Recorded<'_>, replayed: bool) -> u64 {
    let event = recorded.event;
    let kept = event.first_kept_event_seq;
    let through = event
        .provider_replay_through_seq
        .filter(|through| *through < recorded.seq);
    let replacement = event.native.as_ref().is_some_and(replaces_history);
    match through {
        // Replacement history carries every turn up to the snapshot.
        Some(through) if replacement && replayed => through.saturating_add(1),
        // A native summary that kept nothing leaves the entries after its snapshot.
        Some(through) if event.native.is_some() && kept >= recorded.seq => {
            through.saturating_add(1)
        }
        _ => kept,
    }
}

/// The entry that stands for a compaction in the restored context.
pub(super) fn summary(
    recorded: &Recorded<'_>,
    payload: Option<ProviderContinuation>,
    history_rewrite_at_ms: Option<u64>,
) -> Entry {
    compaction_entry(
        recorded.seq,
        recorded.at_ms,
        recorded.event,
        &recorded.patches,
        payload,
        history_rewrite_at_ms,
    )
}

/// Rebuilds the entry of a recorded compaction: its commit time and sequence, the typed
/// details, hook data and amendments, and metadata-only archive frames.
pub(in crate::coord) fn compaction_entry(
    seq: u64,
    at_ms: Option<u64>,
    event: &crate::event::SessionCompactionEvent,
    patches: &[CompactionPatchedEvent],
    payload: Option<ProviderContinuation>,
    history_rewrite_at_ms: Option<u64>,
) -> Entry {
    let archive = event.archive.clone().and_then(import_archive);
    let mut state = CompactionState {
        summary: event.summary.clone(),
        short_summary: event.short_summary.clone(),
        method: event
            .method
            .unwrap_or(harness_compaction::CompactionMethod::Soft),
        tokens_before: event.tokens_before,
        tokens_after: event.tokens_after,
        read_files: event.read_files.clone(),
        modified_files: event.modified_files.clone(),
        warning: event.warning.clone(),
        archive: archive.clone(),
        native: event.native.clone().filter(|n| n.lane != "custom"),
        from_hook: event.from_hook,
        details: event.details.clone(),
        preserve_data: event.preserve_data.clone(),
        commit_seq: seq,
        history_rewrite_at_ms,
    };
    materialize_patches(&mut state, patches);
    let mut entry = summary_entry(state);
    if let Some(archive) = &archive {
        entry.attachments = archive_frame_attachments(archive);
    }
    entry.seq = seq;
    entry.at_ms = at_ms;
    entry.origin = harness_compaction::Origin::Host;
    entry.message.continuation = payload.map(Box::new);
    entry
}

/// Applies amendments recorded after a compaction committed. A later warning replaces an
/// earlier one. Reader views are not part of the state; see [`seed_reader_views`].
pub(in crate::coord) fn materialize_patches(
    state: &mut CompactionState,
    patches: &[CompactionPatchedEvent],
) {
    if let Some(text) = crate::transcript_projection::warning_of(patches) {
        state.warning = Some(text.to_owned());
    }
}

/// Hands the persisted reasoning-free views of a compaction to the request caches.
pub(super) fn seed_reader_views(
    caches: &RenderCaches,
    compaction_seq: u64,
    patches: &[CompactionPatchedEvent],
) {
    for patch in patches {
        if let CompactionPatch::ReaderView {
            reader_key,
            summary,
            archive,
        } = &patch.patch
        {
            caches.seed_reader_view(compaction_seq, reader_key, summary.clone(), archive.clone());
        }
    }
}

/// The time just before the first retained entry, else the first entry after the
/// compaction: native Anthropic replay keeps retained signed thinking by placing its rewrite
/// marker there while the summary keeps its real commit time.
pub(in crate::coord) fn history_rewrite_at_ms(entries: &[Entry], first_kept: usize) -> Option<u64> {
    entries
        .get(first_kept)?
        .at_ms
        .map(|at_ms| at_ms.saturating_sub(1))
}
