//! Committing a compaction: the replacement context, the durable event, hooks and resets.
use super::super::{
    context::{summary_entry, CompactionState, Context, Entry},
    turn::Worker,
    *,
};
use super::maintain::Request;
use super::progress::Progress;
use crate::attachment_transport::AttachmentMetadata;
use crate::config::HookLifecycleEvent as Hook;
use crate::event::{ArchiveFrameRecord, CompactionPhase};
use harness_compaction::CompactionMethod;
use serde_json::{json, Value};

/// Preserve-data keys earlier builds used for provider-native state. Native payloads now live
/// in private artifacts, so a new compaction never carries these forward.
const NATIVE_PRESERVE_KEYS: [&str; 2] = ["openaiRemoteCompaction", "anthropicCompaction"];
/// Serialized cap for hook-supplied details and preserved data.
const PRESERVE_BYTE_LIMIT: usize = 64 * 1024;

/// A compaction ready to commit.
pub(in crate::coord::compaction) struct Draft {
    pub state: CompactionState,
    /// Provider-native replay state for the summary message. The commit stores it in a
    /// private artifact; the event only references it.
    pub continuation: Option<Box<harness_providers::ProviderContinuation>>,
    /// Archive frames with bytes, persisted as private attachment artifacts on commit.
    pub frames: Vec<AttachmentMetadata>,
    /// Index into the context of the first kept entry; `entries.len()` keeps nothing.
    pub cut: usize,
    pub usage: Option<harness_providers::CompletionUsage>,
    pub model: Option<(String, String)>,
    pub replay_through_seq: Option<u64>,
    /// The record names no method (a notes rollover).
    pub omit_method: bool,
    /// The earlier compaction this commit replaces (a rescue's second boundary).
    pub supersedes_seq: Option<u64>,
    /// Where a handoff document was saved.
    pub saved_path: Option<String>,
    /// Entries placed between the summary and the kept tail (a recovered request).
    pub after_summary: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct Committed {
    pub method: CompactionMethod,
    /// The record names no method; `method` is a placeholder.
    pub method_omitted: bool,
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub preview: String,
    /// Journal sequence of the compaction event.
    pub seq: u64,
    pub first_kept_seq: Option<u64>,
    pub short_summary: Option<String>,
    pub warning: Option<String>,
    pub saved_path: Option<String>,
}

/// Previous preserved data without native slots, then hook keys, then engine keys (engine wins).
pub(in crate::coord::compaction) fn merge_preserve(
    previous: &BTreeMap<String, Value>,
    hook: BTreeMap<String, Value>,
    engine: BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut merged: BTreeMap<String, Value> = previous
        .iter()
        .filter(|(key, _)| !NATIVE_PRESERVE_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    merged.extend(hook);
    merged.extend(engine);
    merged.retain(|key, _| !NATIVE_PRESERVE_KEYS.contains(&key.as_str()));
    merged
}

fn oversized(details: &Option<Value>, preserve: &BTreeMap<String, Value>) -> bool {
    let size = details.as_ref().map_or(0, |v| v.to_string().len())
        + preserve
            .iter()
            .map(|(key, value)| key.len() + value.to_string().len())
            .sum::<usize>();
    size > PRESERVE_BYTE_LIMIT
}

impl Draft {
    fn native_payload_present(&self) -> bool {
        self.continuation.is_some()
            && self
                .state
                .native
                .as_ref()
                .is_some_and(|native| native.lane.starts_with("openai"))
    }
}

impl Worker {
    /// The context after a draft commits: system prompt, summary, then the kept tail. Usage
    /// of kept originals stays; measurement discounts it because the summary entry is newer.
    pub(in crate::coord::compaction) fn replacement(
        &self,
        context: &Context,
        draft: &Draft,
    ) -> Context {
        let body = context.body_start();
        let mut replacement = Context {
            entries: context.entries[..body].to_vec(),
            ..context.clone()
        };
        // A draft is "now": every kept original is older than it.
        let mut state = draft.state.clone();
        state.commit_seq = u64::MAX;
        let mut summary = summary_entry(state);
        summary.attachments.clone_from(&draft.frames);
        summary.message.continuation.clone_from(&draft.continuation);
        summary.at_ms = Some(u64::MAX);
        replacement.entries.push(summary);
        replacement
            .entries
            .extend(draft.after_summary.iter().cloned());
        replacement.entries.extend(
            context.entries[draft.cut.min(context.entries.len())..]
                .iter()
                .cloned(),
        );
        replacement
    }

    /// Appends the durable compaction and swaps the live context. Everything after the
    /// append is bookkeeping: its failures are returned to the caller as a late error through
    /// the notice channel, never as a failed commit.
    pub(in crate::coord::compaction) async fn commit(
        &mut self,
        context: &mut Context,
        draft: Draft,
        request: &Request,
        progress: &Progress,
    ) -> Result<Committed, CoordinatorError> {
        let mut replacement = self.replacement(context, &draft);
        let tokens_before = draft.state.tokens_before;
        let tokens_after = self.measure(&replacement)?.tokens;
        let agent = self
            .actor
            .agent_id
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("compaction needs an agent".into()))?;
        let kept = context
            .entries
            .get(draft.cut)
            .map(|e| (e.seq, e.turn.clone()));
        let first_kept_seq = kept.as_ref().map(|(seq, _)| *seq);
        let rewrite_at = super::super::history::history_rewrite_at_ms(&context.entries, draft.cut);
        let (task, owner, cancel, label, operation_id) = (
            self.turn.id.clone(),
            self.actor.clone(),
            progress.cancel.clone(),
            request.label.clone(),
            progress.operation_id.clone(),
        );
        let frames = draft.frames.clone();
        let mut state = draft.state.clone();
        let continuation = draft.continuation.clone();
        let (usage, model) = (draft.usage.clone(), draft.model.clone());
        // A native replacement history stands in for everything summarized: replay covers
        // history up to the last summarized entry and the kept tail stays live.
        let replay_through_seq = draft.replay_through_seq.or_else(|| {
            draft
                .native_payload_present()
                .then(|| {
                    draft
                        .cut
                        .checked_sub(1)
                        .and_then(|last| context.entries.get(last))
                        .map(|entry| entry.seq)
                })
                .flatten()
        });
        let (omit_method, supersedes_seq) = (draft.omit_method, draft.supersedes_seq);
        let (state, seq, at_ms, dropped) = self
            .handle
            .call(move |s| {
                s.check_task(&task)?;
                if cancel.is_cancelled() {
                    return Err(CoordinatorError::Cancelled(task));
                }
                state.summary = s.redactor.redact_text(&state.summary);
                state.short_summary = state.short_summary.map(|t| s.redactor.redact_text(&t));
                state.tokens_after = Some(tokens_after);
                state.details = state
                    .details
                    .take()
                    .map(|v| crate::redact::redact_value(s.redactor.as_ref(), &v));
                state.preserve_data = std::mem::take(&mut state.preserve_data)
                    .into_iter()
                    .map(|(k, v)| (k, crate::redact::redact_value(s.redactor.as_ref(), &v)))
                    .collect();
                let dropped = oversized(&state.details, &state.preserve_data);
                if dropped {
                    state.details = None;
                    state.preserve_data.clear();
                }
                if let Some(archive) = &mut state.archive {
                    archive.text = s.redactor.redact_text(&archive.text);
                    archive.text_head = s.redactor.redact_text(&archive.text_head);
                    archive.text_tail = s.redactor.redact_text(&archive.text_tail);
                    let mut records = Vec::with_capacity(frames.len());
                    for (frame, record) in frames.iter().zip(&archive.frames) {
                        let bytes = frame
                            .bytes()
                            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
                        let artifact =
                            s.write_blob(&owner, None, "attachment", bytes, BTreeMap::new())?;
                        records.push(ArchiveFrameRecord {
                            path: artifact.path.clone(),
                            digest: artifact.digest.clone(),
                            ..record.clone()
                        });
                    }
                    archive.frames = records;
                }
                // The payload is written before the event that references it.
                if let (Some(meta), Some(payload)) =
                    (state.native.as_mut(), continuation.as_deref())
                {
                    meta.api = Some(payload.protocol().as_str().to_owned());
                    meta.payload = Some(s.write_native_payload(&owner, payload)?);
                }
                let hook = json!({
                    "outcome": "compacted",
                    "output_summary": state.summary,
                    "method": state.method.as_str(),
                    "tokens_before": tokens_before,
                    "tokens_after": tokens_after,
                });
                // The written gate runs before the append so a failing hook vetoes the commit.
                s.hook(Hook::CompactionWritten, &owner, Some(&task), hook)?;
                // Nothing kept: the boundary is the compaction event itself, which is the next
                // event appended.
                let (first_kept_event_seq, first_kept_request_id) =
                    kept.clone().unwrap_or((s.last_seq.saturating_add(1), None));
                // The session projection names entries `entry-<seq>`. With nothing kept the
                // boundary is the compaction's own entry; otherwise the projection resolves
                // the first kept sequence to the first entry at or after it, because
                // harness-authored notes have no entry of their own.
                let first_kept_entry_id = kept
                    .is_none()
                    .then(|| crate::ids::EntryId::new(format!("entry-{first_kept_event_seq}")));
                let event = s.emit(
                    owner.clone(),
                    Some(task),
                    EventV1::SessionCompaction(SessionCompactionEvent {
                        agent_id: agent.clone(),
                        summary: state.summary.clone(),
                        first_kept_event_seq,
                        first_kept_request_id,
                        first_kept_entry_id,
                        tokens_before,
                        tokens_after: Some(tokens_after),
                        summary_usage: usage,
                        summary_provider_id: model.as_ref().map(|m| m.0.clone()),
                        summary_model_id: model.map(|m| m.1),
                        read_files: state.read_files.clone(),
                        modified_files: state.modified_files.clone(),
                        task_intent: None,
                        current_intent: None,
                        trigger_reason: label,
                        from_hook: state.from_hook,
                        short_summary: state.short_summary.clone(),
                        method: (!omit_method).then_some(state.method),
                        warning: state.warning.clone(),
                        provider_replay_through_seq: replay_through_seq,
                        native: state.native.clone(),
                        archive: state.archive.clone(),
                        details: state.details.clone(),
                        preserve_data: state.preserve_data.clone(),
                        supersedes_seq,
                        operation_id: Some(operation_id),
                    }),
                )?;
                state.commit_seq = event.seq;
                let at_ms = event
                    .ts
                    .as_deref()
                    .and_then(crate::clock::rfc3339_unix_ms)
                    .and_then(|ms| u64::try_from(ms).ok());
                // Directory instructions re-arm; the speculative snapshot and parked dead
                // ends no longer apply; a manual pass records that it committed before any
                // later work can fail.
                s.instructions_seen.remove(&agent);
                super::speculate::committed(s, &agent);
                s.parked.remove(&agent);
                s.manual_claims.mark_committed(&agent);
                Ok((state, event.seq, at_ms, dropped))
            })
            .await?;
        let body = replacement.body_start();
        if let Some(summary) = replacement.entries.get_mut(body) {
            let mut state = state.clone();
            state.history_rewrite_at_ms = rewrite_at;
            let mut entry = summary_entry(state);
            entry.seq = seq;
            entry.at_ms = at_ms;
            entry.attachments = std::mem::take(&mut summary.attachments);
            entry.message.continuation = summary.message.continuation.take();
            *summary = entry;
        }
        let preview: String = state.summary.chars().take(512).collect();
        super::reduce::rebase_native_usage(&mut replacement, &self.counter);
        *context = replacement;
        self.parked.clear();
        // From here on the compaction is durable: late failures are surfaced, never retried
        // as another method.
        if let Err(error) = self
            .after_commit(
                &state,
                seq,
                tokens_after,
                first_kept_seq,
                request,
                progress,
                dropped,
            )
            .await
        {
            if progress.cancel.is_cancelled() {
                return Err(error);
            }
            let _ = self
                .warn(format!(
                    "Compaction finished, but follow-up work failed: {error}"
                ))
                .await;
        }
        Ok(Committed {
            method: state.method,
            method_omitted: omit_method,
            tokens_before,
            tokens_after,
            preview,
            seq,
            first_kept_seq,
            short_summary: state.short_summary.clone(),
            warning: state.warning.clone(),
            saved_path: draft.saved_path,
        })
    }

    /// Bookkeeping after the append: provider reset descriptor, completion hook, progress.
    #[expect(clippy::too_many_arguments, reason = "One commit's facts")]
    async fn after_commit(
        &self,
        state: &CompactionState,
        seq: u64,
        tokens_after: u32,
        first_kept_seq: Option<u64>,
        request: &Request,
        progress: &Progress,
        dropped: bool,
    ) -> Result<(), CoordinatorError> {
        self.after_rewrite(super::reduce::RewriteReason::Summary(Some(
            harness_providers::CompactionDescriptor {
                operation_id: progress.operation_id.clone(),
                trigger: if request.reason.automatic() {
                    "auto"
                } else {
                    "manual"
                }
                .to_owned(),
                reason: if request.reason.automatic() {
                    "context_limit"
                } else {
                    "user_requested"
                }
                .to_owned(),
                phase: if request.reason.automatic() {
                    request.phase.as_str()
                } else {
                    "standalone_turn"
                }
                .to_owned(),
            },
        )));
        progress
            .update(|stage| {
                stage.method = Some(state.method);
                stage.phase = CompactionPhase::Done;
                stage.tokens_before = Some(state.tokens_before);
                stage.tokens_after = Some(tokens_after);
            })
            .await?;
        if dropped {
            self.warn(
                "Hook data attached to this compaction exceeded 64 KiB and was not kept."
                    .to_owned(),
            )
            .await?;
        }
        let saved = super::hooks::SavedCompaction {
            seq,
            first_kept_seq: first_kept_seq.unwrap_or(seq),
            summary: state.summary.clone(),
            short_summary: state.short_summary.clone(),
            method: Some(state.method),
            tokens_before: state.tokens_before,
            tokens_after,
            from_hook: state.from_hook,
        };
        self.fire_after_hook(&saved).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_data_merges_engine_over_hook_over_previous_without_native_slots() {
        let map = |pairs: &[(&str, i64)]| -> BTreeMap<String, Value> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), json!(v)))
                .collect()
        };
        let merged = merge_preserve(
            &map(&[("kept", 1), ("shared", 1), ("openaiRemoteCompaction", 9)]),
            map(&[("shared", 2), ("hook", 2), ("anthropicCompaction", 9)]),
            map(&[("shared", 3)]),
        );
        assert_eq!(merged, map(&[("kept", 1), ("shared", 3), ("hook", 2)]));
        assert!(merge_preserve(&BTreeMap::new(), BTreeMap::new(), BTreeMap::new()).is_empty());
    }

    #[test]
    fn hook_data_over_the_cap_is_detected() {
        let big = BTreeMap::from([("k".to_owned(), json!("x".repeat(PRESERVE_BYTE_LIMIT)))]);
        assert!(oversized(&None, &big));
        assert!(!oversized(&Some(json!({"kind": "x"})), &BTreeMap::new()));
    }
}
