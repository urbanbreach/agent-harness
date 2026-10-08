//! Lineage bindings, reattach and recovery.
//!
//! A new query is not a new session: `--resume` re-attaches
//! to the existing lineage; a fork adds `--resume-session-at` and `--fork-session`.
use super::continuity::BindingSnapshot;
use super::registry::{close_session, get_or_create_session, CreateEntryInput, SessionEntry};
use crate::anthropic_subscription::{
    errors::LaneError, options::TokenInjection, protocol::QueryOptions,
    transcript::get_session_messages,
};
use regex::Regex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

static BINDINGS: LazyLock<Mutex<HashMap<String, BindingSnapshot>>> = LazyLock::new(Mutex::default);
static INVALIDATIONS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

pub fn remember_binding(session_id: &str, binding: BindingSnapshot) {
    if let Ok(mut bindings) = BINDINGS.lock() {
        bindings.insert(session_id.into(), binding);
    }
}
pub fn get_binding(session_id: &str) -> Option<BindingSnapshot> {
    BINDINGS.lock().ok()?.get(session_id).cloned()
}
pub fn forget_binding(session_id: &str) {
    if let Ok(mut bindings) = BINDINGS.lock() {
        bindings.remove(session_id);
    }
}
/// The reason the newest record invalidated this session's binding, when one is pending.
pub fn remember_binding_invalidation(session_id: &str, reason: Option<&str>) {
    if let Ok(mut invalidations) = INVALIDATIONS.lock() {
        match reason {
            Some(reason) => invalidations.insert(session_id.into(), reason.into()),
            None => invalidations.remove(session_id),
        };
    }
}
pub fn binding_invalidation_reason(session_id: &str) -> Option<String> {
    INVALIDATIONS.lock().ok()?.get(session_id).cloned()
}

pub fn binding_from_entry(entry: &SessionEntry, sent_hashes: &[String]) -> BindingSnapshot {
    let state = entry.lock();
    BindingSnapshot {
        sdk_session_id: state.sdk_session_id.clone(),
        sent_count: state.sent_count,
        sent_hashes: sent_hashes.to_vec(),
        sent_prefix_hash: None,
        last_assistant_uuid: state
            .assistant_uuid_by_index
            .get(&state.sent_count)
            .cloned(),
        assistant_uuid_by_index: state
            .assistant_uuid_by_index
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        account_name: state.account_name.clone(),
        model_id: state.model_id.clone(),
        system_prompt_hash: state.system_prompt_hash.clone(),
        toolset_hash: state.toolset_hash.clone(),
        unanswered_turn_digest: None,
        sdk_session_id_confirmed: Some(state.sdk_session_id_confirmed),
    }
}

pub fn record_synced_stream(entry: &SessionEntry, hashes: &[String]) {
    let mut state = entry.lock();
    state.sent_hashes = Some(hashes.to_vec());
    state.sent_count = hashes.len();
    state.synced_prefix_hash = Some(super::sync::sent_hash_prefix_digest(hashes, hashes.len()));
}

/// A restored binding verifies against the Claude Code transcript: same session, the anchor
/// assistant present, and no top-level user message after it (an un-committed orphan).
pub fn verify_restored_transcript(
    binding: &BindingSnapshot,
    cwd: &std::path::Path,
    auth_lane: TokenInjection,
    env: &dyn Fn(&str) -> Option<String>,
) -> bool {
    if auth_lane == TokenInjection::ConfigDir {
        return false;
    }
    let Ok(messages) = get_session_messages(&binding.sdk_session_id, Some(cwd), env) else {
        return false;
    };
    if messages.is_empty()
        || messages
            .iter()
            .any(|m| m.session_id != binding.sdk_session_id)
    {
        return false;
    }
    let Some(anchor) = &binding.last_assistant_uuid else {
        return true;
    };
    let Some(position) = messages
        .iter()
        .position(|m| m.kind == "assistant" && &m.uuid == anchor)
    else {
        return false;
    };
    let orphans = messages[position + 1..]
        .iter()
        .filter(|m| m.kind == "user")
        .count();
    if orphans > 0 {
        tracing::info!(
            orphan_user_messages = orphans,
            "claude_sdk_oauth_restored_transcript_orphan_tail"
        );
        return false;
    }
    true
}

pub struct ReattachInput<'a> {
    pub session_id: &'a str,
    pub binding: BindingSnapshot,
    pub options: QueryOptions,
    pub at_uuid: Option<String>,
    pub cancel: Option<&'a tokio_util::sync::CancellationToken>,
}

pub async fn reattach_session(input: ReattachInput<'_>) -> Result<Arc<SessionEntry>, LaneError> {
    let binding = input.binding;
    close_session(input.session_id, "reattach");
    let entry = get_or_create_session(CreateEntryInput {
        session_id: input.session_id.into(),
        account_name: binding.account_name.clone(),
        model_id: binding.model_id.clone(),
        system_prompt_hash: binding.system_prompt_hash.clone(),
        toolset_hash: binding.toolset_hash.clone(),
        options: input.options,
        resume: Some((binding.sdk_session_id.clone(), input.at_uuid.clone())),
    })?;
    let initialized = match input.cancel {
        Some(cancel) => tokio::select! {
            result = entry.query.initialization_result() => result.map_err(|e| LaneError::Message(e.0)),
            () = cancel.cancelled() => {
                close_session(input.session_id, "resume_initialization_aborted");
                return Err(LaneError::Message("Anthropic Subscription reattach aborted".into()));
            }
        },
        None => entry
            .query
            .initialization_result()
            .await
            .map_err(|e| LaneError::Message(e.0)),
    };
    if let Err(error) = initialized {
        close_session(input.session_id, "resume_initialization_failed");
        return Err(error);
    }
    record_synced_stream(&entry, &binding.sent_hashes);
    let sdk_session_id = {
        let mut state = entry.lock();
        for (index, uuid) in &binding.assistant_uuid_by_index {
            state.assistant_uuid_by_index.insert(*index, uuid.clone());
        }
        if let Some(uuid) = &binding.last_assistant_uuid {
            state
                .assistant_uuid_by_index
                .insert(binding.sent_count, uuid.clone());
        }
        state.sdk_session_id.clone()
    };
    remember_binding(
        input.session_id,
        BindingSnapshot {
            sdk_session_id,
            ..binding
        },
    );
    Ok(entry)
}

/// Claude Code's wording for a fork point absent from its transcript.
pub static RESUME_MESSAGE_MISSING: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?i)no message found with message\.uuid(?:\s+of)?:\s*([0-9a-fA-F-]+)").ok()
});
/// Claude Code answered "No conversation found with session ID": never resume it again.
pub static RESUME_TARGET_MISSING: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)no conversation found with session id").ok());
const MAX_CHECKPOINT_RECOVERIES: usize = 3;

/// The newest mapped boundary strictly before `binding.sent_count`, inside the hash-proven
/// prefix, that exists in the same session's transcript as a top-level assistant.
pub fn earlier_verified_checkpoint(
    binding: &BindingSnapshot,
    current_hashes: &[String],
    cwd: &std::path::Path,
    auth_lane: TokenInjection,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<BindingSnapshot> {
    if auth_lane == TokenInjection::ConfigDir {
        return None;
    }
    let mut candidates: Vec<&(usize, String)> = binding
        .assistant_uuid_by_index
        .iter()
        .filter(|(index, _)| {
            *index >= 1
                && *index < binding.sent_count
                && *index <= binding.sent_hashes.len()
                && *index <= current_hashes.len()
                && binding.sent_hashes[..*index] == current_hashes[..*index]
        })
        .collect();
    candidates.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    if candidates.is_empty() {
        return None;
    }
    let messages = get_session_messages(&binding.sdk_session_id, Some(cwd), env).ok()?;
    if messages
        .iter()
        .any(|m| m.session_id != binding.sdk_session_id)
    {
        return None;
    }
    let available: std::collections::HashSet<&str> = messages
        .iter()
        .filter(|m| m.kind == "assistant")
        .map(|m| m.uuid.as_str())
        .collect();
    let (sent_count, uuid) = candidates
        .into_iter()
        .find(|(_, uuid)| available.contains(uuid.as_str()))?
        .clone();
    Some(BindingSnapshot {
        sent_count,
        sent_hashes: current_hashes[..sent_count].to_vec(),
        last_assistant_uuid: Some(uuid),
        assistant_uuid_by_index: binding
            .assistant_uuid_by_index
            .iter()
            .filter(|(index, _)| *index <= sent_count)
            .cloned()
            .collect(),
        ..binding.clone()
    })
}

/// Reattaches at the decided boundary; when Claude Code rejects the fork point, forks at the
/// next earlier verified boundary instead of re-sending the whole conversation.
pub async fn reattach_recovering_checkpoint(
    session_id: &str,
    mut binding: BindingSnapshot,
    mut at_uuid: Option<String>,
    options: QueryOptions,
    cancel: Option<&tokio_util::sync::CancellationToken>,
    current_hashes: &[String],
    auth_lane: TokenInjection,
    env: &(dyn Fn(&str) -> Option<String> + Sync),
) -> Result<(Arc<SessionEntry>, usize), LaneError> {
    let mut recovered = 0;
    loop {
        let from = binding.sent_count;
        match reattach_session(ReattachInput {
            session_id,
            binding: binding.clone(),
            options: options.clone(),
            at_uuid: at_uuid.clone(),
            cancel,
        })
        .await
        {
            Ok(entry) => return Ok((entry, from)),
            Err(error) => {
                if cancel.is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
                    || recovered >= MAX_CHECKPOINT_RECOVERIES
                    || !RESUME_MESSAGE_MISSING
                        .as_ref()
                        .is_some_and(|re| re.is_match(&error.message()))
                {
                    return Err(error);
                }
                let Some(earlier) = earlier_verified_checkpoint(
                    &binding,
                    current_hashes,
                    &options.cwd,
                    auth_lane,
                    env,
                ) else {
                    return Err(error);
                };
                let Some(uuid) = earlier.last_assistant_uuid.clone() else {
                    return Err(error);
                };
                tracing::info!(
                    rejected_index = binding.sent_count,
                    recovered_index = earlier.sent_count,
                    "claude_sdk_oauth_checkpoint_recovered"
                );
                binding = earlier;
                at_uuid = Some(uuid);
                recovered += 1;
            }
        }
    }
}

/// A persisted binding is admitted only when its transcript verifies; otherwise forgotten.
pub fn admit_restored_binding(
    session_id: &str,
    cwd: &std::path::Path,
    auth_lane: TokenInjection,
    env: &dyn Fn(&str) -> Option<String>,
) -> (Option<BindingSnapshot>, bool) {
    let binding = get_binding(session_id);
    let Some(restored) = binding.as_ref().filter(|b| b.sent_prefix_hash.is_some()) else {
        return (binding, true);
    };
    let available = verify_restored_transcript(restored, cwd, auth_lane, env);
    if !available {
        forget_binding(session_id);
    }
    (binding, available)
}
