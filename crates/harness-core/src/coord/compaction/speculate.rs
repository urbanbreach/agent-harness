//! Speculative compaction: a background summary prepared just below the threshold, committed
//! instantly once the threshold is crossed if its snapshot is still the start of the context.
use super::super::{context::Context, turn::Worker, *};
use super::commit::Draft;
use super::maintain::{Measure, Reason, Request};
use crate::config::HookLifecycleEvent as Hook;
use harness_compaction::{policy, CompactionMethod};
use harness_providers::ProviderErrorCategory;

mod claim;
mod run;
#[cfg(test)]
mod tests;

/// Room the context may still grow past the threshold while a speculative pass finishes.
const GRACE_FLOOR: u32 = 8_192;

/// One agent's speculative slot.
pub(in crate::coord) struct Speculation {
    generation: u64,
    cancel: CancellationToken,
    model: String,
    tokens_at_start: u32,
    /// Sequence numbers of the snapshot entries, in order.
    prefix: Vec<u64>,
    armed: Option<Box<Draft>>,
    /// Flips to true when the pass ends armed or cleared. Dropping the slot wakes waiters too.
    settled: tokio::sync::watch::Sender<bool>,
}

impl Speculation {
    fn running(&self) -> bool {
        self.armed.is_none()
    }

    pub(in crate::coord) fn is_running(&self) -> bool {
        self.running()
    }
}

/// The blacklist key of a native route: the session and the qualified `provider/model`.
fn native_key(session: &str, model: &str) -> String {
    format!("{session}\0{model}")
}

/// The session a runtime-side caller shares with the agent's worker: a subagent is its own
/// session, a root agent shares the run's.
fn session_of(s: &Runtime, agent: &str) -> Option<String> {
    if s.agents
        .get(agent)
        .is_some_and(|a| a.info.parent_agent_id.is_some())
    {
        return Some(agent.to_owned());
    }
    s.info().ok().map(|info| info.run_id.to_string())
}

/// Drops an agent's slot, aborting any running pass.
pub(in crate::coord) fn cancel(s: &mut Runtime, agent: &str) {
    if let Some(slot) = s.speculation.remove(agent) {
        slot.cancel.cancel();
        s.refresh_speculation_status(agent);
    }
}

/// A committed compaction replaces the snapshot base and re-enables the session's native
/// routes.
pub(super) fn committed(s: &mut Runtime, agent: &str) {
    cancel(s, agent);
    if let Some(session) = session_of(s, agent) {
        let prefix = format!("{session}\0");
        s.native_failed.retain(|key| !key.starts_with(&prefix));
    }
}

#[cfg(test)]
impl Speculation {
    pub(in crate::coord) fn is_armed(&self) -> bool {
        self.armed.is_some()
    }
}

/// Stores a finished pass in its slot, or clears the slot when the pass produced nothing.
/// A slot that moved on to a newer generation, or was aborted, is left alone.
fn arm(s: &mut Runtime, agent: &str, generation: u64, draft: Option<Box<Draft>>) {
    let Some(slot) = s
        .speculation
        .get_mut(agent)
        .filter(|slot| slot.generation == generation && !slot.cancel.is_cancelled())
    else {
        return;
    };
    match draft {
        Some(draft) => {
            slot.armed = Some(draft);
            slot.settled.send_replace(true);
        }
        None => {
            s.speculation.remove(agent);
        }
    }
    s.refresh_speculation_status(agent);
}

/// `[threshold - lead, threshold)`: the upper bound is exclusive, so a context exactly at the
/// threshold neither speculates nor compacts.
fn in_band(tokens: u32, threshold: u32) -> bool {
    tokens >= threshold.saturating_sub(policy::speculation_lead(threshold)) && tokens < threshold
}

/// Over the threshold, a pass in flight may still absorb the crossing below this size.
fn grace_cap(threshold: u32, window: u32) -> u32 {
    threshold
        .saturating_add(policy::speculation_lead(threshold))
        .min(window.saturating_sub(GRACE_FLOOR))
}

/// What the live context makes of an existing slot.
#[derive(Debug, PartialEq, Eq)]
enum Slot {
    Empty,
    Running,
    Armed,
    /// Valid but the context grew too far past the snapshot: start over.
    Refresh,
    /// The snapshot no longer applies (model switch, rewind, reset): discarded.
    Invalid,
}

/// Classifies an armed result. Signed Anthropic results stay valid across appended growth.
fn armed_state(valid: bool, tokens: u32, at_start: u32, keep_recent: u32, signed: bool) -> Slot {
    if !valid {
        return Slot::Invalid;
    }
    let grown = tokens.saturating_sub(at_start) > keep_recent.max(GRACE_FLOOR);
    if grown && !signed {
        Slot::Refresh
    } else {
        Slot::Armed
    }
}

/// A walk skips every native `remote` position when the native route failed and another
/// method can still run. A remote-only walk still attempts it.
pub(in crate::coord::compaction) fn skip_native_route(order: &mut Vec<CompactionMethod>) {
    if order.iter().any(|m| *m != CompactionMethod::Remote) {
        order.retain(|m| *m != CompactionMethod::Remote);
    }
}

/// Only a model-written result can be prepared in the background, and only when it is the
/// first method a real walk would run: a local method is never skipped to speculate later.
fn speculative_first(order: &[CompactionMethod]) -> Option<CompactionMethod> {
    order.first().copied().filter(|method| {
        matches!(
            method,
            CompactionMethod::Remote | CompactionMethod::Handoff | CompactionMethod::Soft
        )
    })
}

/// An irrecoverable typed native failure: the same request fails again, so the route is
/// skipped. Credential, endpoint and retriable (rate limit, transport) failures never are.
fn native_irrecoverable(error: &CoordinatorError) -> bool {
    use super::CompactionErrorKind;
    let (category, status, auth) = match error {
        CoordinatorError::Compaction(e) if e.kind == CompactionErrorKind::Native => {
            (e.category, e.status, e.auth)
        }
        CoordinatorError::Provider { category, .. } => (*category, None, false),
        _ => return false,
    };
    if auth {
        return false;
    }
    match category {
        Some(
            ProviderErrorCategory::OutputLimitExceeded
            | ProviderErrorCategory::UnsupportedToolCall
            | ProviderErrorCategory::MalformedStream
            | ProviderErrorCategory::ContextWindowExceeded,
        ) => true,
        Some(ProviderErrorCategory::Other) => status
            .is_some_and(|s| (400..500).contains(&s) && !matches!(s, 401 | 403 | 408 | 409 | 429)),
        _ => false,
    }
}

impl Worker {
    fn agent_id(&self) -> String {
        self.actor.agent_id.clone().unwrap_or_default()
    }

    /// The native route failed irrecoverably earlier in this session for this model.
    pub(in crate::coord::compaction) async fn native_blocked(
        &self,
    ) -> Result<bool, CoordinatorError> {
        let key = native_key(&self.session, &self.turn.model);
        self.handle
            .call(move |s| Ok(s.native_failed.contains(&key)))
            .await
    }

    /// Methods of an automatic walk that may run here, minus every native `remote` position
    /// when the route failed permanently and some later method exists.
    pub(in crate::coord::compaction) async fn walk_order(
        &self,
        request: &Request,
    ) -> Result<Vec<CompactionMethod>, CoordinatorError> {
        let mut order: Vec<_> = request
            .order
            .iter()
            .copied()
            .filter(|m| self.method_available(*m, request).is_ok())
            .collect();
        if request.reason.automatic()
            && order.contains(&CompactionMethod::Remote)
            && self.native_blocked().await?
        {
            skip_native_route(&mut order);
        }
        Ok(order)
    }

    /// Positive known window and automatic maintenance enabled.
    fn speculation_gated(&self, measure: &Measure) -> bool {
        measure.window_known && measure.window > 0 && self.effective_enabled()
    }

    /// The method a speculative pass would run, if speculation may run at all.
    async fn speculative_method(&self) -> Result<Option<CompactionMethod>, CoordinatorError> {
        self.speculative_method_when(true).await
    }

    /// `busy_blocks` false ignores a compaction in flight, which the footer does not care about.
    pub(in crate::coord::compaction) async fn speculative_method_when(
        &self,
        busy_blocks: bool,
    ) -> Result<Option<CompactionMethod>, CoordinatorError> {
        if !self.compaction.async_enabled || !self.auto_compaction_available() {
            return Ok(None);
        }
        let agent = self.agent_id();
        let busy = self
            .handle
            .call(move |s| {
                // A before-compact handler may veto or replace the result, so a background
                // pass must not bypass it.
                let vetoable = s
                    .config
                    .hook_runtime_config
                    .hooks
                    .lifecycle
                    .iter()
                    .any(|h| h.event == Hook::CompactionRequested);
                Ok(vetoable || (busy_blocks && s.compacting.contains_key(&agent)))
            })
            .await?;
        if busy {
            return Ok(None);
        }
        let order = self.compaction.method_order.clone();
        let request = Request::automatic(Reason::Threshold, &order, None);
        Ok(speculative_first(&self.walk_order(&request).await?))
    }

    /// Looks at the agent's slot and drops it when it no longer applies.
    async fn slot(&self, context: &Context, measure: &Measure) -> Result<Slot, CoordinatorError> {
        let agent = self.agent_id();
        let (model, keep) = (self.turn.model.clone(), self.compaction.keep_recent_tokens);
        let seqs: Vec<u64> = context.entries.iter().map(|e| e.seq).collect();
        let tokens = measure.tokens;
        self.handle
            .call(move |s| {
                let Some(slot) = s.speculation.get(&agent) else {
                    return Ok(Slot::Empty);
                };
                let valid = slot.model == model && seqs.starts_with(&slot.prefix);
                let state = if slot.running() {
                    if valid {
                        Slot::Running
                    } else {
                        Slot::Invalid
                    }
                } else {
                    let signed = slot
                        .armed
                        .as_ref()
                        .and_then(|draft| draft.state.native.as_ref())
                        .is_some_and(|meta| meta.lane.starts_with("anthropic"));
                    armed_state(valid, tokens, slot.tokens_at_start, keep, signed)
                };
                if matches!(state, Slot::Refresh | Slot::Invalid) {
                    cancel(s, &agent);
                }
                Ok(state)
            })
            .await
    }

    /// Below the threshold: keeps a speculative pass running or armed inside the lead band.
    /// With notes-backed windows it queues the notes reminder instead of speculating.
    pub(in crate::coord) async fn speculate_below(
        &mut self,
        context: &Context,
        measure: &Measure,
        pending: Option<usize>,
    ) -> Result<(), CoordinatorError> {
        if !in_band(measure.tokens, measure.threshold) || !self.speculation_gated(measure) {
            return Ok(());
        }
        if self.notes_usable() {
            return self.queue_notes_reminder(context, measure).await;
        }
        let Some(method) = self.speculative_method().await? else {
            return Ok(());
        };
        if !matches!(
            self.slot(context, measure).await?,
            Slot::Running | Slot::Armed
        ) {
            self.start_speculation(context, measure, method, pending)
                .await?;
        }
        Ok(())
    }

    /// Over the threshold: true while a speculative pass should absorb this crossing.
    /// Inside the grace band a running pass defers maintenance and a missing one starts;
    /// a valid armed pass ends deferral so the walk commits it; an invalid one is discarded
    /// without deferral.
    pub(in crate::coord) async fn defer_to_speculation(
        &self,
        context: &Context,
        measure: &Measure,
        pending: Option<usize>,
    ) -> Result<bool, CoordinatorError> {
        if !self.speculation_gated(measure) || self.notes_usable() {
            return Ok(false);
        }
        let Some(method) = self.speculative_method().await? else {
            return Ok(false);
        };
        if measure.tokens >= grace_cap(measure.threshold, measure.window) {
            return Ok(false);
        }
        match self.slot(context, measure).await? {
            Slot::Running => Ok(true),
            Slot::Armed | Slot::Invalid => Ok(false),
            Slot::Empty | Slot::Refresh => {
                self.start_speculation(context, measure, method, pending)
                    .await?;
                Ok(true)
            }
        }
    }

    /// Waits until the agent's running speculative pass armed, cleared or was dropped, or
    /// the worker is cancelled. Callers recheck cancellation and the slot afterwards.
    pub(in crate::coord) async fn await_speculation_settled(&self) -> Result<(), CoordinatorError> {
        let agent = self.agent_id();
        let waiting = self
            .handle
            .call(move |s| {
                Ok(s.speculation
                    .get(&agent)
                    .filter(|slot| slot.running())
                    .map(|slot| slot.settled.subscribe()))
            })
            .await?;
        let Some(mut settled) = waiting else {
            return Ok(());
        };
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => {}
            // A dropped slot closes the channel; both ends mean the pass is over.
            _ = settled.wait_for(|done| *done) => {}
        }
        Ok(())
    }
}
