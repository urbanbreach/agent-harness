//! The turn pump (senpi `session-registry-pump.ts`, `session-turn-claim.ts`).
use super::*;

// ---- the pump ----

fn is_replay_for(message: &Value, uuid: &str) -> bool {
    message["type"] == "user" && message["isReplay"] == true && message["uuid"] == uuid
}

fn is_autonomous_result(message: &Value) -> bool {
    if message["origin"].is_object() && message["origin"]["kind"] != "human" {
        return true;
    }
    !message["parent_tool_use_id"].is_null()
        || !message["subagent_type"].is_null()
        || message["isSynthetic"] == true
}

fn is_foreign_result(message: &Value, turn: &ActiveTurn) -> bool {
    match message["user_message_uuid"].as_str() {
        Some(uuid) => uuid != turn.uuid,
        None => is_autonomous_result(message),
    }
}

fn result_matches_turn(message: &Value, turn: &ActiveTurn) -> bool {
    match message["user_message_uuid"].as_str() {
        Some(uuid) => uuid == turn.uuid,
        None => turn.claimed && !is_autonomous_result(message),
    }
}

fn deliver(state: &mut EntryState, message: Value) -> Result<(), LaneError> {
    if state.state == SessionState::TurnClaimed {
        transition(&mut state.state, SessionState::TurnStreaming)?;
    }
    let Some(turn) = state.active_turn.as_mut() else {
        return Ok(());
    };
    turn.messages.push(message.clone());
    match turn.sink.try_send(message) {
        Err(mpsc::error::TrySendError::Full(_)) => Err(LaneError::Message(format!(
            "Anthropic Subscription session stream queue exceeded {SESSION_STREAM_QUEUE_CAPACITY} messages"
        ))),
        _ => Ok(()),
    }
}

fn claim_turn(state: &mut EntryState) -> Result<Vec<Value>, LaneError> {
    let Some(turn) = state.active_turn.as_mut() else {
        return Ok(Vec::new());
    };
    turn.claimed = true;
    let buffered = std::mem::take(&mut turn.pre_replay);
    turn.pre_replay_bytes = 0;
    transition(&mut state.state, SessionState::TurnClaimed)?;
    Ok(buffered)
}

fn fail_turn(entry: &Arc<SessionEntry>, error: LaneError) {
    let turn = entry.lock().active_turn.take();
    if let Some(mut turn) = turn
        && let Some(completion) = turn.completion.take()
    {
        let _ = completion.send(Err(error.clone()));
    }
    record_use(entry, false);
    if is_current_generation(&entry.session_id, entry.generation) {
        close_session(&entry.session_id, &error.message());
    }
}

/// An interrupt that never produces a terminal result still ends the turn as aborted;
/// only the query closes, so the binding survives and the next turn reattaches.
fn abort_turn(entry: &Arc<SessionEntry>, uuid: &str) {
    if !is_current_generation(&entry.session_id, entry.generation) {
        return;
    }
    let turn = {
        let mut state = entry.lock();
        match &state.active_turn {
            Some(turn) if turn.uuid == uuid => state.active_turn.take(),
            _ => None,
        }
    };
    let Some(mut turn) = turn else {
        return;
    };
    record_use(entry, false);
    close_session(&entry.session_id, "abort_uncertain");
    if let Some(completion) = turn.completion.take() {
        let _ = completion.send(Ok(TurnResult {
            uuid: turn.uuid,
            messages: turn.messages,
            aborted: true,
        }));
    }
}

/// `evaluateAbortOutcome`: keep the query only when Claude Code reports an empty queue.
fn abort_outcome_keep(receipt: Option<&Value>) -> bool {
    receipt
        .and_then(|r| r["still_queued"].as_array())
        .is_some_and(Vec::is_empty)
}

fn finish_turn(entry: &Arc<SessionEntry>, message: &Value) -> Result<(), LaneError> {
    let (turn, close) = {
        let mut state = entry.lock();
        let Some(turn) = state.active_turn.as_ref() else {
            return Ok(());
        };
        if !result_matches_turn(message, turn) {
            return Err(LaneError::Message(
                "Anthropic Subscription result user_message_uuid did not match the active turn"
                    .into(),
            ));
        }
        let aborted = turn.aborted;
        let keep = abort_outcome_keep(turn.interrupt_receipt.as_ref());
        if state.state == SessionState::TurnClaimed {
            transition(&mut state.state, SessionState::TurnStreaming)?;
        }
        if !aborted {
            deliver(&mut state, message.clone())?;
        }
        transition(&mut state.state, SessionState::TurnResultSeen)?;
        let turn = state.active_turn.take();
        let close = aborted && !keep;
        if !close {
            transition(&mut state.state, SessionState::IdleSynced)?;
        }
        (turn, close)
    };
    let idle = !close;
    record_use(entry, idle);
    if close {
        close_session(&entry.session_id, "abort_uncertain");
    }
    if let Some(mut turn) = turn
        && let Some(completion) = turn.completion.take()
    {
        let _ = completion.send(Ok(TurnResult {
            uuid: turn.uuid,
            messages: turn.messages,
            aborted: turn.aborted,
        }));
    }
    Ok(())
}

/// One stdout message; `Ok(true)` stops the pump.
fn handle_message(entry: &Arc<SessionEntry>, message: Value) -> Result<bool, LaneError> {
    // A forked query mints a NEW session id; persist it before any turn-state guard.
    if message["type"] == "system"
        && message["subtype"] == "init"
        && let Some(id) = message["session_id"].as_str()
    {
        let mut state = entry.lock();
        if id != state.sdk_session_id {
            state.sdk_session_id = id.into();
        }
        state.sdk_session_id_confirmed = true;
    }
    let current = is_current_generation(&entry.session_id, entry.generation);
    let mut state = entry.lock();
    let Some(turn) = state.active_turn.as_ref() else {
        return Ok(false);
    };
    if !current || turn.generation != entry.generation {
        return Ok(false);
    }
    if !turn.claimed {
        let uuid = turn.uuid.clone();
        if is_replay_for(&message, &uuid) {
            state.sdk_session_id_confirmed = true;
            for buffered in claim_turn(&mut state)? {
                if buffered["type"] == "result" {
                    drop(state);
                    finish_turn(entry, &buffered)?;
                    state = entry.lock();
                } else {
                    deliver(&mut state, buffered)?;
                }
            }
        } else if message["type"] == "stream_event" {
            buffer_before_replay(&mut state, message);
        } else if message["type"] == "result" && result_matches_turn(&message, turn) {
            if let Some(failure) = sdk_result_failure(&message) {
                return Err(failure);
            }
            for buffered in claim_turn(&mut state)? {
                deliver(&mut state, buffered)?;
            }
            drop(state);
            finish_turn(entry, &message)?;
        } else if message["type"] == "result" && is_foreign_result(&message, turn) {
            if let Some(turn) = state.active_turn.as_mut() {
                turn.pre_replay.clear();
                turn.pre_replay_bytes = 0;
                turn.pre_replay_overflowed = false;
            }
        } else if message["type"] == "result" {
            // A failure before the replay echo (a version floor, a session limit) must
            // surface as that failure so failover can classify and rotate.
            return Err(sdk_result_failure(&message).unwrap_or_else(|| {
                LaneError::Message(
                    "Anthropic Subscription result arrived before replay claim".into(),
                )
            }));
        }
        return Ok(false);
    }
    if message["type"] == "user" && message["isReplay"] == true {
        return Ok(false);
    }
    if let Some(refusal) = refusal_error(&message) {
        drop(state);
        fail_turn(entry, refusal);
        return Ok(true);
    }
    if message["type"] == "result" {
        if let Some(failure) = sdk_result_failure(&message) {
            drop(state);
            fail_turn(entry, failure);
            return Ok(true);
        }
        drop(state);
        finish_turn(entry, &message)?;
    } else {
        deliver(&mut state, message)?;
    }
    Ok(false)
}

/// Events ahead of our replay belong to a turn Claude Code is already running; only
/// main-thread events are held, bounded, and an outgrown segment is dropped.
fn buffer_before_replay(state: &mut EntryState, message: Value) {
    let Some(turn) = state.active_turn.as_mut() else {
        return;
    };
    if !message["parent_tool_use_id"].is_null() || turn.pre_replay_overflowed {
        return;
    }
    turn.pre_replay_bytes += message.to_string().len();
    turn.pre_replay.push(message);
    if turn.pre_replay.len() > DEFAULT_PRE_REPLAY_MAX_MESSAGES
        || turn.pre_replay_bytes > DEFAULT_PRE_REPLAY_MAX_BYTES
    {
        turn.pre_replay.clear();
        turn.pre_replay_bytes = 0;
        turn.pre_replay_overflowed = true;
    }
}

async fn run_pump(entry: Arc<SessionEntry>) {
    loop {
        match entry.query.next().await {
            None => {
                fail_turn(
                    &entry,
                    LaneError::Message(
                        "Anthropic Subscription query ended before the active turn completed"
                            .into(),
                    ),
                );
                return;
            }
            Some(Err(error)) => {
                fail_turn(&entry, LaneError::Message(error.0));
                return;
            }
            Some(Ok(message)) => match handle_message(&entry, message) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    fail_turn(&entry, error);
                    return;
                }
            },
        }
    }
}

pub struct SubmittedTurn {
    pub uuid: String,
    pub messages: mpsc::Receiver<Value>,
    pub completion: oneshot::Receiver<Result<TurnResult, LaneError>>,
}

/// Admits one turn: pushes its user message and starts the pump on first use.
pub fn submit_session_turn(
    entry: &Arc<SessionEntry>,
    message: Value,
) -> Result<SubmittedTurn, LaneError> {
    let uuid = create_session_uuid(now_ms());
    let (sink, messages) = mpsc::channel(SESSION_STREAM_QUEUE_CAPACITY);
    let (completion_tx, completion) = oneshot::channel();
    let (start_pump, sdk_session_id) = {
        let mut state = entry.lock();
        if state.active_turn.is_some() {
            return Err(LaneError::Message(format!(
                "Concurrent Anthropic Subscription turn admission for session {}",
                entry.session_id
            )));
        }
        if state.state == SessionState::Starting {
            transition(&mut state.state, SessionState::IdleSynced)?;
        }
        transition(&mut state.state, SessionState::TurnWaiting)?;
        state.active_turn = Some(ActiveTurn {
            uuid: uuid.clone(),
            generation: entry.generation,
            messages: Vec::new(),
            pre_replay: Vec::new(),
            pre_replay_bytes: 0,
            pre_replay_overflowed: false,
            claimed: false,
            aborted: false,
            interrupt_receipt: None,
            sink,
            completion: Some(completion_tx),
        });
        let start = !state.pump_started;
        state.pump_started = true;
        (start, state.sdk_session_id.clone())
    };
    record_use(entry, false);
    if start_pump {
        tokio::spawn(run_pump(Arc::clone(entry)));
    }
    entry
        .query
        .push_user(json!({
            "type": "user",
            "message": message,
            "parent_tool_use_id": null,
            "uuid": uuid,
            "session_id": sdk_session_id,
        }))
        .map_err(|e| LaneError::Message(e.0))?;
    transition(&mut entry.lock().state, SessionState::TurnSent)?;
    Ok(SubmittedTurn {
        uuid,
        messages,
        completion,
    })
}

/// The caller abandoned the turn: interrupt, and settle it as aborted after a grace period
/// when Claude Code never answers with a terminal result.
pub fn abort_session_turn(entry: &Arc<SessionEntry>, uuid: &str) {
    {
        let mut state = entry.lock();
        match state.active_turn.as_mut() {
            Some(turn) if turn.uuid == uuid && !turn.aborted => turn.aborted = true,
            _ => return,
        }
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let (timer_entry, timer_uuid) = (Arc::clone(entry), uuid.to_owned());
    handle.spawn(async move {
        tokio::time::sleep(SESSION_TURN_ABORT_GRACE).await;
        abort_turn(&timer_entry, &timer_uuid);
    });
    let (entry, uuid) = (Arc::clone(entry), uuid.to_owned());
    handle.spawn(async move {
        match entry.query.interrupt().await {
            Ok(receipt) => {
                if let Some(turn) = entry.lock().active_turn.as_mut()
                    && turn.uuid == uuid
                {
                    turn.interrupt_receipt = receipt;
                }
            }
            Err(_) => abort_turn(&entry, &uuid),
        }
    });
}

pub fn mark_tainted(session_id: &str, reason: &str) {
    if let Some(entry) = get_session(session_id) {
        {
            let mut state = entry.lock();
            state.tainted_reason = Some(reason.into());
            if state.state != SessionState::Tainted {
                let _ = transition(&mut state.state, SessionState::Tainted);
            }
        }
        touch(&entry);
    }
}

pub fn record_pending_fork(session_id: &str, reason: &str) {
    if let Some(entry) = get_session(session_id) {
        entry.lock().pending_fork_reason = Some(reason.into());
        touch(&entry);
    }
}
