//! Resident Claude Code sessions and the turn pump.
//!
//! One streaming query
//! per harness session, a strict per-turn state machine, replay-claimed turn attribution, and
//! idle reaping.
use super::observability::record_pending_close_cause;
use crate::anthropic_subscription::{
    errors::{refusal_error, sdk_result_failure, LaneError},
    protocol::{ClaudeQuery, Prompt, QueryOptions},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
mod pump;
pub use pump::*;

pub const SESSION_REGISTRY_IDLE_TTL_MS: i64 = 30 * 60_000;
pub const SESSION_REGISTRY_MAX_ENTRIES: usize = 32;
pub const SESSION_STREAM_QUEUE_CAPACITY: usize = 256;
pub const DEFAULT_PRE_REPLAY_MAX_MESSAGES: usize = 64;
pub const DEFAULT_PRE_REPLAY_MAX_BYTES: usize = 256 * 1024;
pub const SESSION_TURN_ABORT_GRACE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    IdleSynced,
    TurnWaiting,
    TurnSent,
    TurnClaimed,
    TurnStreaming,
    TurnResultSeen,
    Tainted,
    Closing,
    Closed,
    Broken,
}

fn allowed(from: SessionState, to: SessionState) -> bool {
    use SessionState::{
        Broken, Closed, Closing, IdleSynced, Starting, Tainted, TurnClaimed, TurnResultSeen,
        TurnSent, TurnStreaming, TurnWaiting,
    };
    match from {
        Starting => matches!(to, IdleSynced | Tainted | Closing | Broken),
        IdleSynced => matches!(to, TurnWaiting | Tainted | Closing | Broken),
        TurnWaiting => matches!(to, TurnSent | Tainted | Closing | Broken),
        TurnSent => matches!(to, TurnClaimed | Tainted | Closing | Broken),
        TurnClaimed => matches!(to, TurnStreaming | Tainted | Closing | Broken),
        TurnStreaming => matches!(to, TurnResultSeen | Tainted | Closing | Broken),
        TurnResultSeen => matches!(to, IdleSynced | Tainted | Closing | Broken),
        Tainted => matches!(to, Closing | Broken),
        Closing => matches!(to, Closed | Broken),
        Closed => false,
        Broken => matches!(to, Closing | Closed),
    }
}

pub fn transition(state: &mut SessionState, next: SessionState) -> Result<(), LaneError> {
    if !allowed(*state, next) {
        return Err(LaneError::Message(format!(
            "Illegal session state transition: {state:?} -> {next:?}"
        )));
    }
    *state = next;
    Ok(())
}

pub fn now_ms() -> i64 {
    crate::anthropic_subscription::auth_lane::now_ms()
}

/// A UUIDv7-shaped id: 48-bit millisecond timestamp, then random bits.
pub fn create_session_uuid(now: i64) -> String {
    let ts = u64::try_from(now.max(0)).unwrap_or(0);
    let mut random = [0u8; 10];
    let _ = getrandom::fill(&mut random);
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().take(6).enumerate() {
        *byte = u8::try_from((ts >> (40 - 8 * i)) & 0xff).unwrap_or(0);
    }
    bytes[6] = 0x70 | (random[0] & 0x0f);
    bytes[7] = random[1];
    bytes[8] = 0x80 | (random[2] & 0x3f);
    bytes[9] = random[3];
    bytes[10..].copy_from_slice(&random[4..]);
    let hex = crate::anthropic_subscription::prompt::hex(&bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub struct TurnResult {
    pub uuid: String,
    pub messages: Vec<Value>,
    pub aborted: bool,
}

struct ActiveTurn {
    uuid: String,
    generation: u64,
    messages: Vec<Value>,
    pre_replay: Vec<Value>,
    pre_replay_bytes: usize,
    pre_replay_overflowed: bool,
    claimed: bool,
    aborted: bool,
    interrupt_receipt: Option<Value>,
    sink: mpsc::Sender<Value>,
    completion: Option<oneshot::Sender<Result<TurnResult, LaneError>>>,
}

pub struct EntryState {
    pub sdk_session_id: String,
    pub sdk_session_id_confirmed: bool,
    pub account_name: String,
    pub model_id: String,
    pub toolset_hash: String,
    pub system_prompt_hash: String,
    pub state: SessionState,
    active_turn: Option<ActiveTurn>,
    pump_started: bool,
    pub synced_prefix_hash: Option<String>,
    pub sent_count: usize,
    /// The hashes `recordSyncedStream` last recorded (senpi's `sentHashesByEntry`).
    pub sent_hashes: Option<Vec<String>>,
    pub assistant_uuid_by_index: BTreeMap<usize, String>,
    pub tainted_reason: Option<String>,
    pub pending_fork_reason: Option<String>,
    pub last_used_at: i64,
    pub credential_digest: Option<String>,
    /// Semantic hash of the assistant this lane produced at each sent count.
    pub provider_final: BTreeMap<usize, String>,
    reap_token: u64,
}
impl EntryState {
    pub fn has_active_turn(&self) -> bool {
        self.active_turn.is_some()
    }
}

pub struct SessionEntry {
    pub session_id: String,
    pub generation: u64,
    pub query: Arc<ClaudeQuery>,
    pub state: Mutex<EntryState>,
}

impl SessionEntry {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, EntryState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn evictable(state: &EntryState) -> bool {
    matches!(
        state.state,
        SessionState::IdleSynced | SessionState::Tainted
    ) && state.active_turn.is_none()
}

pub struct CreateEntryInput {
    pub session_id: String,
    pub account_name: String,
    pub model_id: String,
    pub toolset_hash: String,
    pub system_prompt_hash: String,
    pub options: QueryOptions,
    pub resume: Option<(String, Option<String>)>,
}

#[derive(Default)]
struct Registry {
    entries: HashMap<String, Arc<SessionEntry>>,
    generation_counter: u64,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(Mutex::default);

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn get_session(session_id: &str) -> Option<Arc<SessionEntry>> {
    registry().entries.get(session_id).cloned()
}

pub fn is_current_generation(session_id: &str, generation: u64) -> bool {
    registry()
        .entries
        .get(session_id)
        .is_some_and(|e| e.generation == generation)
}

pub fn is_idle_expired(entry: &SessionEntry) -> bool {
    now_ms() - entry.lock().last_used_at >= SESSION_REGISTRY_IDLE_TTL_MS
}

pub fn close_session(session_id: &str, reason: &str) {
    let Some(entry) = registry().entries.get(session_id).cloned() else {
        return;
    };
    record_pending_close_cause(session_id, reason);
    {
        let mut state = entry.lock();
        state.reap_token += 1;
        let _ = transition(&mut state.state, SessionState::Closing);
    }
    entry.query.close();
    {
        let mut state = entry.lock();
        let _ = transition(&mut state.state, SessionState::Closed);
    }
    let mut registry = registry();
    if registry
        .entries
        .get(session_id)
        .is_some_and(|e| Arc::ptr_eq(e, &entry))
    {
        registry.entries.remove(session_id);
    }
}

/// Updates `lastUsedAt` and re-arms the idle reaper when the entry just went idle.
pub fn record_use(entry: &Arc<SessionEntry>, schedule_reap: bool) {
    if !is_current_generation(&entry.session_id, entry.generation) {
        return;
    }
    let token = {
        let mut state = entry.lock();
        state.last_used_at = now_ms();
        state.reap_token += 1;
        state.reap_token
    };
    if schedule_reap {
        arm_reap(
            Arc::clone(entry),
            token,
            Duration::from_millis(u64::try_from(SESSION_REGISTRY_IDLE_TTL_MS).unwrap_or(0)),
        );
    }
}

pub fn touch(entry: &Arc<SessionEntry>) {
    let idle = {
        let state = entry.lock();
        state.active_turn.is_none() && evictable(&state)
    };
    record_use(entry, idle);
}

fn arm_reap(entry: Arc<SessionEntry>, token: u64, delay: Duration) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        tokio::time::sleep(delay).await;
        let (current, idle, last_used) = {
            let state = entry.lock();
            (
                state.reap_token == token,
                evictable(&state),
                state.last_used_at,
            )
        };
        if !current || !idle || !is_current_generation(&entry.session_id, entry.generation) {
            return;
        }
        let remaining = last_used + SESSION_REGISTRY_IDLE_TTL_MS - now_ms();
        if remaining <= 0 {
            close_session(&entry.session_id, "idle_ttl");
        } else {
            arm_reap(
                Arc::clone(&entry),
                token,
                Duration::from_millis(u64::try_from(remaining).unwrap_or(0)),
            );
        }
    });
}

fn entries() -> Vec<Arc<SessionEntry>> {
    registry().entries.values().cloned().collect()
}

fn evict_expired() {
    let expired: Vec<String> = entries()
        .iter()
        .filter(|entry| {
            let state = entry.lock();
            evictable(&state) && now_ms() - state.last_used_at >= SESSION_REGISTRY_IDLE_TTL_MS
        })
        .map(|entry| entry.session_id.clone())
        .collect();
    for id in expired {
        close_session(&id, "idle_ttl");
    }
}

fn ensure_capacity() -> Result<(), LaneError> {
    let oldest = {
        let entries = entries();
        if entries.len() < SESSION_REGISTRY_MAX_ENTRIES {
            return Ok(());
        }
        entries
            .iter()
            .filter_map(|entry| {
                let state = entry.lock();
                evictable(&state).then(|| (state.last_used_at, entry.session_id.clone()))
            })
            .min()
    };
    let Some((_, oldest)) = oldest else {
        return Err(LaneError::Message(format!(
            "Anthropic Subscription session registry is at its {SESSION_REGISTRY_MAX_ENTRIES}-entry limit with no idle session to evict"
        )));
    };
    close_session(&oldest, "capacity");
    Ok(())
}

pub fn get_or_create_session(input: CreateEntryInput) -> Result<Arc<SessionEntry>, LaneError> {
    if let Some(existing) = get_session(&input.session_id) {
        let keep = {
            let state = existing.lock();
            !evictable(&state) || now_ms() - state.last_used_at < SESSION_REGISTRY_IDLE_TTL_MS
        };
        if keep {
            touch(&existing);
            return Ok(existing);
        }
        close_session(&existing.session_id, "idle_ttl");
    }
    evict_expired();
    ensure_capacity()?;
    let now = now_ms();
    let sdk_session_id = input
        .resume
        .as_ref()
        .map_or_else(|| create_session_uuid(now), |(id, _)| id.clone());
    let mut options = input.options.clone();
    match &input.resume {
        Some((id, at)) => {
            options.resume = Some(id.clone());
            if let Some(at) = at {
                options.resume_session_at = Some(at.clone());
                options.fork_session = true;
            }
        }
        None => options.session_id = Some(sdk_session_id.clone()),
    }
    options.set_extra_arg("replay-user-messages", Some(""));
    let query = Arc::new(
        ClaudeQuery::spawn(&options, Prompt::Streaming).map_err(|e| LaneError::Message(e.0))?,
    );
    let mut registry = registry();
    registry.generation_counter += 1;
    let entry = Arc::new(SessionEntry {
        session_id: input.session_id.clone(),
        generation: registry.generation_counter,
        query,
        state: Mutex::new(EntryState {
            sdk_session_id,
            sdk_session_id_confirmed: input.resume.is_some(),
            account_name: input.account_name,
            model_id: input.model_id,
            toolset_hash: input.toolset_hash,
            system_prompt_hash: input.system_prompt_hash,
            state: SessionState::Starting,
            active_turn: None,
            pump_started: false,
            synced_prefix_hash: None,
            sent_count: 0,
            sent_hashes: None,
            assistant_uuid_by_index: BTreeMap::new(),
            tainted_reason: None,
            pending_fork_reason: None,
            last_used_at: now,
            credential_digest: None,
            provider_final: BTreeMap::new(),
            reap_token: 0,
        }),
    });
    registry
        .entries
        .insert(input.session_id, Arc::clone(&entry));
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_uuids_are_v7_shaped_and_transitions_are_strict() {
        let uuid = create_session_uuid(1_791_374_400_000);
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[14..15], "7");
        assert!(matches!(&uuid[19..20], "8" | "9" | "a" | "b"));
        assert!(uuid.starts_with("01a1163c-1a00-7"));
        let mut state = SessionState::Starting;
        assert!(transition(&mut state, SessionState::TurnSent).is_err());
        assert!(transition(&mut state, SessionState::IdleSynced).is_ok());
        assert!(transition(&mut state, SessionState::TurnWaiting).is_ok());
    }
}
