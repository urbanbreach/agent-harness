//! Resident-lane attempts (senpi `session-stream.ts`, `session-turn-attempt.ts`).
use super::binding::{committed_assistant, message_content_hash, restore_binding, BindingStore};
use super::continuity::{
    decide_native_continuity, BindingSnapshot, Decision, DecisionInput, EntrySnapshot,
};
use super::observability::{
    emit_continuity_observation, observe_session_sync_decision, sanitize_terminal_failure,
    ContinuityObservation, ObservedKind, StagedDecision, SyncDecision,
};
use super::reattach::{
    admit_restored_binding, binding_from_entry, binding_invalidation_reason, forget_binding,
    get_binding, reattach_recovering_checkpoint, record_synced_stream, remember_binding,
    remember_binding_invalidation, RESUME_MESSAGE_MISSING, RESUME_TARGET_MISSING,
};
use super::registry::{
    abort_session_turn, close_session, get_or_create_session, get_session, is_current_generation,
    is_idle_expired, record_pending_fork, submit_session_turn, CreateEntryInput, SessionEntry,
};
use super::sync::{
    config_fingerprint, sent_hash_prefix_digest, sent_message_hashes, sent_messages,
};
use crate::anthropic_subscription::{
    auth_lane::{
        query_with_auth_lane, Attempt, AttemptFactory, AuthLaneInput, AuthenticatedAttempt,
        MessageStream,
    },
    cold_seed::{cold_seed_calibration, cold_seed_overflow, estimate_cold_seed_tokens},
    errors::{sdk_result_failure, LaneError},
    options::TokenInjection,
    prompt::{
        build_delta_prompt_blocks, build_prompt_blocks, dedupe_ultrawork_blocks,
        serialized_payload_bytes, LaneContext, LaneMessage,
    },
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, LazyLock, Mutex},
};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
mod attempt;
pub use attempt::*;

/// What the provider layer learns about the dispatched attempt.
#[derive(Debug, Default, Clone)]
pub struct DispatchShape {
    pub cold_seed: bool,
    pub estimated_tokens: Option<u64>,
    pub resume_fallback: Option<String>,
    pub observation: Option<ContinuityObservation>,
}

pub struct ResidentInput {
    pub session_id: String,
    pub model: String,
    pub context: Arc<LaneContext>,
    pub custom_to_sdk: BTreeMap<String, String>,
    pub tool_watch_note: Option<String>,
    pub context_window: Option<u64>,
    pub cancel: CancellationToken,
    pub environment: Arc<BTreeMap<String, String>>,
    pub binding_store: Option<Arc<BindingStore>>,
    pub shape: Arc<Mutex<DispatchShape>>,
}

/// Sessions whose persisted binding was already consulted this process (senpi `session_start`).
static RESTORED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

/// Drops the persisted and in-memory binding with a recorded cause (`invalidateBinding`).
pub fn invalidate_binding(session_id: &str, store: Option<&BindingStore>, reason: &str) {
    forget_binding(session_id);
    if let Some(store) = store {
        store.delete(session_id);
    }
    remember_binding_invalidation(session_id, Some(reason));
}

fn restore_persisted_binding(input: &ResidentInput, hashes: &[String]) {
    let first = RESTORED
        .lock()
        .is_ok_and(|mut seen| seen.insert(input.session_id.clone()));
    if !first || get_binding(&input.session_id).is_some() {
        return;
    }
    let Some(store) = &input.binding_store else {
        return;
    };
    let Some(stored) = store.read(&input.session_id) else {
        store.delete(&input.session_id);
        return;
    };
    match restore_binding(&stored, &input.context, hashes) {
        Some(binding) => remember_binding(&input.session_id, binding),
        None => store.delete(&input.session_id),
    }
}

/// `message_end` commit boundary: an assistant the next request carries differently than
/// this lane streamed it was rewritten, and the lineage must fork before it.
fn detect_rewritten_assistant(input: &ResidentInput, entry: &SessionEntry) {
    let (sent_count, produced) = {
        let state = entry.lock();
        (
            state.sent_count,
            state.provider_final.get(&state.sent_count).cloned(),
        )
    };
    let Some(produced) = produced else {
        return;
    };
    let committed =
        committed_assistant(&input.context, sent_count).and_then(|(m, _)| message_content_hash(m));
    if committed.is_some_and(|hash| hash != produced) {
        record_pending_fork(&input.session_id, "assistant_rewritten");
        invalidate_binding(
            &input.session_id,
            input.binding_store.as_deref(),
            "assistant_rewritten",
        );
    }
}

fn entry_snapshot(entry: &SessionEntry, hashes: &[String]) -> EntrySnapshot {
    let state = entry.lock();
    EntrySnapshot {
        sdk_session_id: state.sdk_session_id.clone(),
        account_name: state.account_name.clone(),
        model_id: state.model_id.clone(),
        system_prompt_hash: state.system_prompt_hash.clone(),
        toolset_hash: state.toolset_hash.clone(),
        sent_count: state.sent_count,
        sent_hashes: hashes[..state.sent_count.min(hashes.len())].to_vec(),
        last_assistant_uuid: state
            .assistant_uuid_by_index
            .get(&state.sent_count)
            .cloned(),
        assistant_uuid_by_index: state.assistant_uuid_by_index.clone(),
        pending_fork_reason: state.pending_fork_reason.clone(),
        tainted_reason: state.tainted_reason.clone(),
        credential_digest: state.credential_digest.clone(),
    }
}

fn observed_kind(decision: &Decision) -> ObservedKind {
    match decision {
        Decision::Delta { .. } => ObservedKind::Incremental,
        Decision::Reattach { .. } | Decision::Fork { .. } => ObservedKind::Resume,
        Decision::Flatten { .. } | Decision::Bootstrap { .. } => ObservedKind::ColdSeed,
    }
}

async fn create_resident_attempt(
    input: Arc<ResidentInput>,
    auth: AuthenticatedAttempt,
) -> Result<Attempt, LaneError> {
    let session_id = input.session_id.clone();
    let messages = sent_messages(&input.context);
    let hashes = sent_message_hashes(&messages);
    let env_map = Arc::clone(&input.environment);
    let env = move |name: &str| env_map.get(name).cloned();
    if let Some(entry) = get_session(&session_id) {
        detect_rewritten_assistant(&input, &entry);
    }
    let existing = get_session(&session_id);
    if existing.is_none() {
        restore_persisted_binding(&input, &hashes);
    }
    let fingerprint = config_fingerprint(
        &auth.options,
        &input.context,
        auth.auth_lane,
        &auth.account_name,
    );
    let resident_hashes = existing
        .as_ref()
        .and_then(|e| e.lock().sent_hashes.clone())
        .unwrap_or_else(|| hashes.clone());
    let (binding, transcript_available) =
        admit_restored_binding(&session_id, &auth.options.cwd, auth.auth_lane, &env);
    let snapshot = existing
        .as_ref()
        .map(|e| entry_snapshot(e, &resident_hashes));
    let invalidation = binding_invalidation_reason(&session_id);
    let decision = decide_native_continuity(&DecisionInput {
        entry: snapshot.as_ref(),
        binding: binding.as_ref(),
        current_hashes: &hashes,
        account_name: &auth.account_name,
        model_id: &input.model,
        system_prompt_hash: &fingerprint.system_prompt_hash,
        toolset_hash: &fingerprint.toolset_hash,
        transcript_available,
        cross_account_resume_supported: auth.auth_lane != TokenInjection::ConfigDir,
        idle_expired: existing.as_deref().is_some_and(is_idle_expired),
        invalidation_reason: invalidation.as_deref(),
        credential_digest: auth.credential_digest.as_deref(),
    });
    let first_turn = existing.is_none()
        && get_binding(&session_id).is_none()
        && !input
            .context
            .messages
            .iter()
            .any(|m| matches!(m, LaneMessage::Assistant { .. }));
    let mut observed_reason: Option<&'static str> = match &decision {
        Decision::Reattach { reason, .. }
        | Decision::Fork { reason, .. }
        | Decision::Flatten { reason } => Some(*reason),
        Decision::Bootstrap { reason } => Some(reason.unwrap_or("registry_miss")),
        Decision::Delta { .. } => None,
    };
    let mut kind = observed_kind(&decision);
    let mut flatten = matches!(
        decision,
        Decision::Flatten { .. } | Decision::Bootstrap { .. }
    );
    let new_entry = |fingerprint: &super::sync::SessionConfigFingerprint| {
        get_or_create_session(CreateEntryInput {
            session_id: session_id.clone(),
            account_name: auth.account_name.clone(),
            model_id: input.model.clone(),
            toolset_hash: fingerprint.toolset_hash.clone(),
            system_prompt_hash: fingerprint.system_prompt_hash.clone(),
            options: auth.options.clone(),
            resume: None,
        })
    };
    let (entry, from) = match (&decision, &existing) {
        (Decision::Delta { from }, Some(existing)) => (Arc::clone(existing), *from),
        (Decision::Reattach { from, .. } | Decision::Fork { from, .. }, _) => {
            let at_uuid = match &decision {
                Decision::Fork { at_uuid, .. } => Some(at_uuid.clone()),
                _ => None,
            };
            let source = get_binding(&session_id).or_else(|| {
                existing
                    .as_ref()
                    .map(|e| binding_from_entry(e, &resident_hashes))
            });
            let reattached = match source {
                None => Err(LaneError::Message(
                    "Anthropic Subscription continuity binding is unavailable".into(),
                )),
                Some(source) => {
                    let binding = BindingSnapshot {
                        sent_prefix_hash: None,
                        sent_count: *from,
                        sent_hashes: hashes[..(*from).min(hashes.len())].to_vec(),
                        assistant_uuid_by_index: source
                            .assistant_uuid_by_index
                            .iter()
                            .filter(|(index, _)| *index <= *from)
                            .cloned()
                            .collect(),
                        account_name: auth.account_name.clone(),
                        model_id: input.model.clone(),
                        system_prompt_hash: fingerprint.system_prompt_hash.clone(),
                        toolset_hash: fingerprint.toolset_hash.clone(),
                        last_assistant_uuid: at_uuid.clone().or(source.last_assistant_uuid.clone()),
                        ..source
                    };
                    reattach_recovering_checkpoint(
                        &session_id,
                        binding,
                        at_uuid,
                        auth.options.clone(),
                        Some(&input.cancel),
                        &hashes,
                        auth.auth_lane,
                        &env,
                    )
                    .await
                }
            };
            match reattached {
                Ok(reattached) => reattached,
                Err(error) => {
                    if input.cancel.is_cancelled() {
                        return Err(error);
                    }
                    if let Ok(mut shape) = input.shape.lock() {
                        shape.resume_fallback = Some(error.message());
                    }
                    kind = ObservedKind::ColdSeed;
                    observed_reason = Some("resume_initialization_failed");
                    flatten = true;
                    (new_entry(&fingerprint)?, 0)
                }
            }
        }
        _ => {
            if existing.is_some() {
                close_session(&session_id, observed_reason.unwrap_or("registry_miss"));
            }
            (new_entry(&fingerprint)?, 0)
        }
    };
    // Every branch above reused a subprocess whose token matched or spawned one with this token.
    entry
        .lock()
        .credential_digest
        .clone_from(&auth.credential_digest);

    let flattened = flatten.then(|| {
        dedupe_ultrawork_blocks(build_prompt_blocks(
            &input.context,
            Some(&input.custom_to_sdk),
            input.tool_watch_note.as_deref(),
        ))
    });
    let estimated = flattened
        .as_ref()
        .map(|f| estimate_cold_seed_tokens(&input.context, &f.blocks));
    if let Ok(mut shape) = input.shape.lock() {
        shape.cold_seed = flattened.is_some();
        shape.estimated_tokens = estimated;
    }
    if let Some(estimated) = estimated
        && let Some(overflow) = cold_seed_overflow(
            input.context_window,
            estimated,
            cold_seed_calibration(Some(&session_id)),
        )
    {
        // Never dispatch a re-send that cannot fit; the overflow goes to harness compaction.
        close_session(&session_id, "cold_seed_over_budget");
        return Err(overflow);
    }
    let payload_bytes = flattened
        .as_ref()
        .map(|f| serialized_payload_bytes(&f.blocks));
    let collapsed = flattened.as_ref().map(|f| f.collapsed_directives);
    let blocks = match flattened {
        Some(flattened) => flattened.blocks,
        None => build_delta_prompt_blocks(
            &messages[from.min(messages.len())..],
            Some(&input.custom_to_sdk),
        ),
    };
    let observation = observe_session_sync_decision(&SyncDecision {
        kind,
        reason: observed_reason,
        delta_messages: if flatten {
            hashes.len()
        } else {
            hashes.len() - from.min(hashes.len())
        },
        first_turn,
        session_id: &session_id,
        payload_bytes,
        collapsed_directives: collapsed,
    });
    if let Ok(mut shape) = input.shape.lock() {
        shape.observation = Some(observation.clone());
    }
    let staged = StagedDecision::new(observation, &session_id);
    Ok(session_turn_attempt(
        entry,
        json!({"role": "user", "content": blocks}),
        hashes,
        staged,
        input.cancel.clone(),
    ))
}

/// The resident lane: every attempt admits through the registry; a turn whose every
/// attempt failed yields exactly one terminal `failed` observation.
pub fn resident_session_messages(
    input: Arc<ResidentInput>,
    mut lane: AuthLaneInput,
) -> MessageStream {
    let factory_input = Arc::clone(&input);
    let factory: AttemptFactory = Arc::new(move |auth| {
        let input = Arc::clone(&factory_input);
        Box::pin(create_resident_attempt(input, auth))
    });
    lane.create_attempt = factory;
    let session_id = input.session_id.clone();
    Box::pin(async_stream::stream! {
        let mut messages = query_with_auth_lane(lane);
        while let Some(item) = messages.next().await {
            if let Err(error) = &item {
                emit_continuity_observation(
                    &ContinuityObservation {
                        kind: "failed",
                        reason: sanitize_terminal_failure(&error.message()),
                        delta_messages: None,
                        payload_bytes: None,
                        collapsed_directives: None,
                    },
                    Some(&session_id),
                );
            }
            yield item;
        }
    })
}
