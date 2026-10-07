//! Retainable turn attempts (senpi `session-turn-attempt.ts`).
use super::*;

#[derive(Default)]
struct AttemptState {
    uuid: Option<String>,
    completion: Option<
        tokio::sync::oneshot::Receiver<
            Result<crate::anthropic_subscription::session::registry::TurnResult, LaneError>,
        >,
    >,
    finished: bool,
    discarded: bool,
    resume_target_missing: bool,
    staged: Option<StagedDecision>,
}

fn publish_binding(entry: &SessionEntry, mut binding: BindingSnapshot) {
    binding.sdk_session_id_confirmed = Some(entry.lock().sdk_session_id_confirmed);
    remember_binding(&entry.session_id, binding);
}

/// An attempt that pushed its payload and then aborted or failed leaves that message
/// un-answered on the lineage; the same turn's retry forks past it.
fn remember_retry_checkpoint(entry: &SessionEntry, hashes: &[String]) {
    let sent = entry.lock().sent_count;
    if sent > hashes.len() {
        return;
    }
    let mut binding = binding_from_entry(entry, &hashes[..sent]);
    binding.unanswered_turn_digest = Some(sent_hash_prefix_digest(hashes, hashes.len()));
    publish_binding(entry, binding);
}

/// Drops only the boundary Claude Code rejected; earlier ones stay mapped.
fn forget_missing_assistant_uuid(entry: &SessionEntry, message: &str) {
    let Some(missing) = RESUME_MESSAGE_MISSING
        .as_ref()
        .and_then(|re| re.captures(message))
        .and_then(|c| c.get(1).map(|m| m.as_str().to_owned()))
    else {
        return;
    };
    entry
        .lock()
        .assistant_uuid_by_index
        .retain(|_, uuid| *uuid != missing);
}

fn successful_turn(messages: &[Value]) -> bool {
    messages.iter().any(|m| {
        m["type"] == "result" && m["subtype"] == "success" && sdk_result_failure(m).is_none()
    })
}

struct AbortGuard {
    entry: Arc<SessionEntry>,
    hashes: Vec<String>,
    state: Arc<Mutex<AttemptState>>,
}
impl Drop for AbortGuard {
    fn drop(&mut self) {
        let (uuid, completion, staged) = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.finished || state.discarded {
                return;
            }
            (
                state.uuid.clone(),
                state.completion.take(),
                state.staged.take(),
            )
        };
        let Some(uuid) = uuid else {
            return;
        };
        // The caller abandoned the turn: interrupt it, then record the retry checkpoint
        // once Claude Code settles it, exactly as an aborted turn's consumer would.
        abort_session_turn(&self.entry, &uuid);
        let (entry, hashes) = (Arc::clone(&self.entry), self.hashes.clone());
        if let (Some(completion), Ok(handle)) = (completion, tokio::runtime::Handle::try_current())
        {
            handle.spawn(async move {
                let _ = completion.await;
                remember_retry_checkpoint(&entry, &hashes);
                if let Some(mut staged) = staged {
                    staged.emit();
                }
            });
        }
    }
}

pub(super) fn session_turn_attempt(
    entry: Arc<SessionEntry>,
    message: Value,
    hashes: Vec<String>,
    staged: StagedDecision,
    cancel: CancellationToken,
) -> Attempt {
    let state = Arc::new(Mutex::new(AttemptState {
        staged: Some(staged),
        ..AttemptState::default()
    }));
    let guard = AbortGuard {
        entry: Arc::clone(&entry),
        hashes: hashes.clone(),
        state: Arc::clone(&state),
    };
    let generation = entry.generation;
    let discard_entry = Arc::clone(&entry);
    let discard_hashes = hashes.clone();
    let messages: MessageStream = Box::pin(async_stream::stream! {
        let guard = guard;
        let submitted = match submit_session_turn(&entry, message) {
            Ok(submitted) => submitted,
            Err(error) => {
                remember_retry_checkpoint(&entry, &hashes);
                yield Err(error);
                return;
            }
        };
        if let Ok(mut state) = guard.state.lock() {
            state.uuid = Some(submitted.uuid.clone());
            state.completion = Some(submitted.completion);
        }
        let mut receiver = submitted.messages;
        let mut aborting = false;
        loop {
            let received = tokio::select! {
                biased;
                // The pump keeps delivering until Claude Code settles the interrupted turn.
                () = cancel.cancelled(), if !aborting => {
                    aborting = true;
                    abort_session_turn(&entry, &submitted.uuid);
                    continue;
                }
                received = receiver.recv() => received,
            };
            let Some(sdk_message) = received else { break };
            if sdk_message["type"] == "assistant" && sdk_message["parent_tool_use_id"].is_null()
                && let Some(uuid) = sdk_message["uuid"].as_str()
            {
                entry.lock().assistant_uuid_by_index.insert(hashes.len(), uuid.to_owned());
            }
            yield Ok(sdk_message);
        }
        let completion = guard.state.lock().ok().and_then(|mut s| s.completion.take());
        let result = match completion {
            Some(completion) => completion.await.unwrap_or_else(|_| Err(LaneError::Message("Anthropic Subscription query ended before the active turn completed".into()))),
            None => Err(LaneError::Message("Anthropic Subscription query ended before the active turn completed".into())),
        };
        if let Ok(mut state) = guard.state.lock() {
            state.finished = true;
        }
        match result {
            Ok(turn) => {
                if !turn.aborted && successful_turn(&turn.messages) {
                    record_synced_stream(&entry, &hashes);
                    publish_binding(&entry, binding_from_entry(&entry, &hashes));
                } else {
                    remember_retry_checkpoint(&entry, &hashes);
                }
                let staged = guard.state.lock().ok().and_then(|mut s| s.staged.take());
                if let Some(mut staged) = staged {
                    staged.emit();
                }
            }
            Err(error) => {
                if RESUME_TARGET_MISSING.as_ref().is_some_and(|re| re.is_match(&error.message())) {
                    if let Ok(mut state) = guard.state.lock() {
                        state.resume_target_missing = true;
                    }
                    forget_binding(&entry.session_id);
                } else {
                    forget_missing_assistant_uuid(&entry, &error.message());
                    remember_retry_checkpoint(&entry, &hashes);
                }
                yield Err(error);
            }
        }
    });
    let discard: Box<dyn FnOnce() + Send> = Box::new(move || {
        let missing = state.lock().map(|mut s| {
            s.discarded = true;
            s.resume_target_missing
        });
        if missing.unwrap_or(false) {
            forget_binding(&discard_entry.session_id);
        } else {
            remember_retry_checkpoint(&discard_entry, &discard_hashes);
        }
        if is_current_generation(&discard_entry.session_id, generation) {
            close_session(&discard_entry.session_id, "attempt_discarded");
        }
    });
    Attempt {
        messages,
        discard: Some(discard),
    }
}
