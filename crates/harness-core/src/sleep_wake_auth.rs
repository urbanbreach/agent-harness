// allow: SIZE_OK — sleep/wake auth product surface (policy, hook event source, execution, owner tests)
//! Sleep/wake-aware credential refresh: hook event source + proactive OAuth refresh.
//!
//! Product path:
//! 1. [`open_platform_sleep_wake_event_source`] opens a hook-based listener (Active).
//! 2. Host/OS adapters inject [`SleepWakeHostEvent`]s into the hook.
//! 3. On wake/resume with near-expiry credentials, [`execute_sleep_wake_refresh_decision`]
//!    reuses [`crate::auth::ProviderCredentialManager::refresh_oauth_if_near_expiry`].
//!
//! Unsupported native OS power APIs may still use the hook; if no source is opened,
//! policy is structured [`SleepWakeCredentialPolicy::Unavailable`].

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::auth::{CredentialResolveError, ProviderCredentialManager, ResolvedCredential};

/// Default leeway before expiry (5 minutes) used when callers omit an explicit value.
pub const DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS: i64 = 5 * 60 * 1000;

/// Product-complete hook event-source strategy id.
pub const HOOK_EVENT_SOURCE_STRATEGY: &str = "hook";

/// Policy outcome for sleep/wake credential refresh infrastructure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SleepWakeCredentialPolicy {
    /// Wake-triggered refresh infrastructure is active (hook listener open).
    Active { strategy: String },
    /// No sleep/wake refresh infrastructure; ordinary token expiry handling only.
    NoOp { reason: String },
    /// Explicitly unavailable (fail-closed when no platform source is opened).
    Unavailable { reason: String },
}

impl SleepWakeCredentialPolicy {
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    pub const fn is_noop_or_unavailable(&self) -> bool {
        matches!(self, Self::NoOp { .. } | Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Active { strategy } => {
                format!("sleep/wake credential refresh: active (strategy={strategy})")
            }
            Self::NoOp { reason } => {
                format!("sleep/wake credential refresh: noop ({reason})")
            }
            Self::Unavailable { reason } => {
                format!("sleep/wake credential refresh: unavailable ({reason})")
            }
        }
    }
}

/// Observed host power/session event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepWakeHostEvent {
    Sleep,
    Wake,
    Resume,
    Suspend,
}

impl SleepWakeHostEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::Wake => "wake",
            Self::Resume => "resume",
            Self::Suspend => "suspend",
        }
    }

    pub const fn may_trigger_refresh_evaluation(self) -> bool {
        matches!(self, Self::Wake | Self::Resume)
    }
}

/// Result of observing a host sleep/wake event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SleepWakeObservation {
    Recorded {
        event: SleepWakeHostEvent,
        policy: SleepWakeCredentialPolicy,
    },
}

impl SleepWakeObservation {
    pub fn one_line(&self) -> String {
        match self {
            Self::Recorded { event, policy } => {
                let policy_part = match policy {
                    SleepWakeCredentialPolicy::Active { strategy } => {
                        format!("policy=active strategy={strategy}")
                    }
                    SleepWakeCredentialPolicy::NoOp { reason } => {
                        format!("policy=noop reason={reason}")
                    }
                    SleepWakeCredentialPolicy::Unavailable { reason } => {
                        format!("policy=unavailable reason={reason}")
                    }
                };
                if policy.is_active() {
                    format!(
                        "sleep/wake observe: {} recorded ({policy_part})",
                        event.as_str()
                    )
                } else {
                    format!(
                        "sleep/wake observe: {} recorded as noop ({policy_part})",
                        event.as_str()
                    )
                }
            }
        }
    }

    pub const fn is_recorded(&self) -> bool {
        matches!(self, Self::Recorded { .. })
    }

    /// True when the recorded policy is NoOp/Unavailable (legacy diagnostic name).
    pub fn is_recorded_noop(&self) -> bool {
        match self {
            Self::Recorded { policy, .. } => policy.is_noop_or_unavailable(),
        }
    }
}

/// Operator-facing counts for sleep/wake observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SleepWakeObservationSummary {
    pub recorded: usize,
    /// Observations whose policy was NoOp/Unavailable.
    pub recorded_noop: usize,
    pub total: usize,
}

impl SleepWakeObservationSummary {
    pub fn one_line(&self) -> String {
        if self.recorded_noop == self.total && self.total > 0 {
            format!(
                "sleep/wake observations: {} recorded-noop ({} total)",
                self.recorded_noop, self.total
            )
        } else {
            format!(
                "sleep/wake observations: {} recorded ({} total; {} noop-policy)",
                self.recorded, self.total, self.recorded_noop
            )
        }
    }

    pub const fn all_recorded_noop(&self) -> bool {
        self.total > 0 && self.recorded_noop == self.total
    }
}

/// Summarize a batch of sleep/wake observations for operator surfaces.
pub fn summarize_sleep_wake_observations(
    observations: &[SleepWakeObservation],
) -> SleepWakeObservationSummary {
    let mut summary = SleepWakeObservationSummary {
        total: observations.len(),
        ..SleepWakeObservationSummary::default()
    };
    for observation in observations {
        if observation.is_recorded() {
            summary.recorded = summary.recorded.saturating_add(1);
        }
        if observation.is_recorded_noop() {
            summary.recorded_noop = summary.recorded_noop.saturating_add(1);
        }
    }
    summary
}

/// Snapshot of credential expiry for pure policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialExpirySnapshot {
    pub expires_at_unix_ms: Option<i64>,
    pub now_unix_ms: i64,
    pub leeway_ms: i64,
}

impl CredentialExpirySnapshot {
    pub fn with_default_leeway(expires_at_unix_ms: Option<i64>, now_unix_ms: i64) -> Self {
        Self {
            expires_at_unix_ms,
            now_unix_ms,
            leeway_ms: DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS,
        }
    }

    pub fn remaining_ms(self) -> Option<i64> {
        self.expires_at_unix_ms
            .map(|expires_at| expires_at.saturating_sub(self.now_unix_ms))
    }

    pub fn is_near_expiry(self) -> bool {
        match self.remaining_ms() {
            Some(remaining) => remaining <= self.leeway_ms,
            None => false,
        }
    }
}

/// Platform event-source abstraction. Implementations may poll, hook, or bridge OS APIs.
#[async_trait]
pub trait SleepWakeEventSource: Send {
    fn strategy(&self) -> &'static str;
    async fn recv(&mut self) -> Option<SleepWakeHostEvent>;
}

/// Hook-based event source: OS adapters and tests inject events via [`HookSleepWakeEventInjector`].
pub struct HookSleepWakeEventSource {
    rx: mpsc::UnboundedReceiver<SleepWakeHostEvent>,
}

/// Injector half of a hook event source.
#[derive(Clone)]
pub struct HookSleepWakeEventInjector {
    tx: mpsc::UnboundedSender<SleepWakeHostEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleepWakeInjectError {
    Closed,
}

impl HookSleepWakeEventSource {
    pub fn open() -> (Self, HookSleepWakeEventInjector) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { rx }, HookSleepWakeEventInjector { tx })
    }
}

impl HookSleepWakeEventInjector {
    pub fn inject(&self, event: SleepWakeHostEvent) -> Result<(), SleepWakeInjectError> {
        self.tx
            .send(event)
            .map_err(|_| SleepWakeInjectError::Closed)
    }
}

#[async_trait]
impl SleepWakeEventSource for HookSleepWakeEventSource {
    fn strategy(&self) -> &'static str {
        HOOK_EVENT_SOURCE_STRATEGY
    }

    async fn recv(&mut self) -> Option<SleepWakeHostEvent> {
        self.rx.recv().await
    }
}

/// Result of opening the platform default sleep/wake event source.
pub enum PlatformSleepWakeEventSource {
    Active {
        strategy: &'static str,
        source: HookSleepWakeEventSource,
        injector: HookSleepWakeEventInjector,
    },
    Unavailable {
        reason: String,
    },
}

impl PlatformSleepWakeEventSource {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }

    pub fn policy(&self) -> SleepWakeCredentialPolicy {
        match self {
            Self::Active { strategy, .. } => SleepWakeCredentialPolicy::Active {
                strategy: (*strategy).to_string(),
            },
            Self::Unavailable { reason } => SleepWakeCredentialPolicy::Unavailable {
                reason: reason.clone(),
            },
        }
    }
}

/// Open the product-default sleep/wake event source (hook-based listener).
///
/// Always Active with strategy [`HOOK_EVENT_SOURCE_STRATEGY`]. Native OS power
/// signals are expected to be bridged into the returned injector by the host.
pub fn open_platform_sleep_wake_event_source() -> PlatformSleepWakeEventSource {
    let (source, injector) = HookSleepWakeEventSource::open();
    PlatformSleepWakeEventSource::Active {
        strategy: HOOK_EVENT_SOURCE_STRATEGY,
        source,
        injector,
    }
}

/// Structured unavailable source (tests / platforms that refuse to open a listener).
pub fn unavailable_sleep_wake_event_source(
    reason: impl Into<String>,
) -> PlatformSleepWakeEventSource {
    PlatformSleepWakeEventSource::Unavailable {
        reason: reason.into(),
    }
}

/// Evaluate sleep/wake credential refresh **infrastructure** policy.
///
/// Returns Active with the hook strategy — product-complete event source.
pub fn evaluate_sleep_wake_credential_refresh() -> SleepWakeCredentialPolicy {
    SleepWakeCredentialPolicy::Active {
        strategy: HOOK_EVENT_SOURCE_STRATEGY.to_string(),
    }
}

/// Alias used by product layers that expect an availability enum.
pub fn sleep_wake_credential_refresh_availability() -> SleepWakeCredentialPolicy {
    match evaluate_sleep_wake_credential_refresh() {
        SleepWakeCredentialPolicy::NoOp { reason } => {
            SleepWakeCredentialPolicy::Unavailable { reason }
        }
        other => other,
    }
}

/// Observe a host sleep/wake event under the current infrastructure policy.
pub fn observe_sleep_wake_host_event(event: SleepWakeHostEvent) -> SleepWakeObservation {
    let policy = evaluate_sleep_wake_credential_refresh();
    SleepWakeObservation::Recorded { event, policy }
}

/// Credential refresh decision after a host sleep/wake observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum SleepWakeRefreshDecision {
    Skip {
        event: SleepWakeHostEvent,
        reason: String,
    },
    /// Near-expiry on wake/resume — proactive refresh should run.
    Refresh {
        event: SleepWakeHostEvent,
        reason: String,
        remaining_ms: i64,
    },
}

impl SleepWakeRefreshDecision {
    pub const fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }

    pub const fn is_refresh(&self) -> bool {
        matches!(self, Self::Refresh { .. })
    }

    pub const fn claims_refresh(&self) -> bool {
        matches!(self, Self::Refresh { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Skip { event, reason } => {
                format!(
                    "sleep/wake decision: skip refresh for {} ({reason})",
                    event.as_str()
                )
            }
            Self::Refresh {
                event,
                reason,
                remaining_ms,
            } => {
                format!(
                    "sleep/wake decision: refresh recommended for {} (remaining_ms={remaining_ms}; {reason})",
                    event.as_str()
                )
            }
        }
    }

    pub fn event(self) -> SleepWakeHostEvent {
        match self {
            Self::Skip { event, .. } | Self::Refresh { event, .. } => event,
        }
    }
}

/// Decide whether a host sleep/wake event should recommend credential refresh.
pub fn decide_sleep_wake_credential_refresh(event: SleepWakeHostEvent) -> SleepWakeRefreshDecision {
    decide_sleep_wake_credential_refresh_for(event, None)
}

/// Decide refresh recommendation with optional credential expiry snapshot.
pub fn decide_sleep_wake_credential_refresh_for(
    event: SleepWakeHostEvent,
    expiry: Option<&CredentialExpirySnapshot>,
) -> SleepWakeRefreshDecision {
    if !event.may_trigger_refresh_evaluation() {
        return SleepWakeRefreshDecision::Skip {
            event,
            reason: format!(
                "host event `{}` does not evaluate credential refresh",
                event.as_str()
            ),
        };
    }

    let Some(snapshot) = expiry else {
        return SleepWakeRefreshDecision::Skip {
            event,
            reason: "no credential expiry snapshot provided; cannot recommend refresh".to_string(),
        };
    };

    match snapshot.remaining_ms() {
        None => SleepWakeRefreshDecision::Skip {
            event,
            reason: "credential expiry unknown; cannot recommend refresh".to_string(),
        },
        Some(remaining) if remaining <= snapshot.leeway_ms => SleepWakeRefreshDecision::Refresh {
            event,
            remaining_ms: remaining,
            reason: format!(
                "credentials near expiry (remaining_ms={remaining} <= leeway_ms={}); \
                 proactive refresh via ProviderCredentialManager",
                snapshot.leeway_ms
            ),
        },
        Some(remaining) => SleepWakeRefreshDecision::Skip {
            event,
            reason: format!(
                "credentials still fresh (remaining_ms={remaining} > leeway_ms={})",
                snapshot.leeway_ms
            ),
        },
    }
}

/// Observe a host event and produce the matching refresh decision (no expiry context).
pub fn observe_and_decide_sleep_wake_host_event(
    event: SleepWakeHostEvent,
) -> (SleepWakeObservation, SleepWakeRefreshDecision) {
    observe_and_decide_sleep_wake_host_event_for(event, None)
}

/// Observe a host event and decide with optional credential expiry.
pub fn observe_and_decide_sleep_wake_host_event_for(
    event: SleepWakeHostEvent,
    expiry: Option<&CredentialExpirySnapshot>,
) -> (SleepWakeObservation, SleepWakeRefreshDecision) {
    (
        observe_sleep_wake_host_event(event),
        decide_sleep_wake_credential_refresh_for(event, expiry),
    )
}

/// Outcome of executing a sleep/wake refresh decision against a credential manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleepWakeRefreshExecution {
    Skipped {
        decision: SleepWakeRefreshDecision,
    },
    Refreshed {
        decision: SleepWakeRefreshDecision,
        token_source: String,
        expires_at: Option<String>,
    },
    Failed {
        decision: SleepWakeRefreshDecision,
        error: String,
    },
    Cancelled {
        decision: SleepWakeRefreshDecision,
        reason: String,
    },
}

impl SleepWakeRefreshExecution {
    pub const fn is_refreshed(&self) -> bool {
        matches!(self, Self::Refreshed { .. })
    }

    pub const fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Skipped { decision } => {
                format!("sleep/wake execute: skipped ({})", decision.one_line())
            }
            Self::Refreshed {
                decision,
                token_source,
                expires_at,
            } => format!(
                "sleep/wake execute: refreshed source={token_source} expires={} ({})",
                expires_at.as_deref().unwrap_or("none"),
                decision.one_line()
            ),
            Self::Failed { decision, error } => {
                format!(
                    "sleep/wake execute: failed ({error}; {})",
                    decision.one_line()
                )
            }
            Self::Cancelled { decision, reason } => {
                format!(
                    "sleep/wake execute: cancelled ({reason}; {})",
                    decision.one_line()
                )
            }
        }
    }
}

/// Execute a sleep/wake refresh decision using ordinary auth refresh (cancellable).
pub async fn execute_sleep_wake_refresh_decision(
    decision: &SleepWakeRefreshDecision,
    manager: &ProviderCredentialManager,
    cancel: &CancellationToken,
) -> SleepWakeRefreshExecution {
    if cancel.is_cancelled() {
        return SleepWakeRefreshExecution::Cancelled {
            decision: decision.clone(),
            reason: "cancellation requested before refresh".to_string(),
        };
    }
    match decision {
        SleepWakeRefreshDecision::Skip { .. } => SleepWakeRefreshExecution::Skipped {
            decision: decision.clone(),
        },
        SleepWakeRefreshDecision::Refresh { .. } => {
            let leeway = Duration::from_millis(
                u64::try_from(DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS).unwrap_or(u64::MAX),
            );
            let refresh = manager.refresh_oauth_if_near_expiry(leeway);
            tokio::select! {
                biased;
                () = cancel.cancelled() => SleepWakeRefreshExecution::Cancelled {
                    decision: decision.clone(),
                    reason: "cancellation requested during refresh".to_string(),
                },
                result = refresh => match result {
                    Ok(resolved) => SleepWakeRefreshExecution::Refreshed {
                        decision: decision.clone(),
                        token_source: format!("{:?}", resolved.source),
                        expires_at: resolved.expires_at,
                    },
                    Err(err) => SleepWakeRefreshExecution::Failed {
                        decision: decision.clone(),
                        error: err.to_string(),
                    },
                },
            }
        }
    }
}

/// Observe → decide → execute for a host event with optional expiry snapshot.
pub async fn observe_decide_and_execute_sleep_wake_host_event(
    event: SleepWakeHostEvent,
    expiry: Option<&CredentialExpirySnapshot>,
    manager: &ProviderCredentialManager,
    cancel: &CancellationToken,
) -> (
    SleepWakeObservation,
    SleepWakeRefreshDecision,
    SleepWakeRefreshExecution,
) {
    let (observation, decision) = observe_and_decide_sleep_wake_host_event_for(event, expiry);
    let execution = execute_sleep_wake_refresh_decision(&decision, manager, cancel).await;
    (observation, decision, execution)
}

/// Drain hook events until the source closes or `cancel` fires.
pub async fn run_sleep_wake_refresh_loop(
    source: &mut dyn SleepWakeEventSource,
    manager: &ProviderCredentialManager,
    expiry_for_event: impl Fn(SleepWakeHostEvent) -> Option<CredentialExpirySnapshot>,
    cancel: CancellationToken,
) -> Vec<SleepWakeRefreshExecution> {
    let mut executions = Vec::new();
    loop {
        if cancel.is_cancelled() {
            break;
        }
        let event = tokio::select! {
            biased;
            () = cancel.cancelled() => break,
            event = source.recv() => event,
        };
        let Some(event) = event else {
            break;
        };
        let expiry = expiry_for_event(event);
        let (_, _, execution) = observe_decide_and_execute_sleep_wake_host_event(
            event,
            expiry.as_ref(),
            manager,
            &cancel,
        )
        .await;
        executions.push(execution);
    }
    executions
}

/// Helper for tests/callers that already hold a resolved credential result.
pub fn resolved_credential_token_source(resolved: &ResolvedCredential) -> String {
    format!("{:?}", resolved.source)
}

/// Map credential resolve errors into a stable operator string (no secrets).
pub fn credential_resolve_error_one_line(err: &CredentialResolveError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{
        CredentialClock, CredentialRefreshError, CredentialStore, OAuthRefreshOutcome,
        OAuthTokenRefresher, ProviderId, StoredCredential,
    };
    use crate::UnwrapOrAbort;
    use harness_providers::ProviderErrorCategory;
    use std::fmt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::SystemTime;
    use tokio::sync::oneshot;

    #[derive(Debug)]
    struct FixedClock(SystemTime);

    impl CredentialClock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    struct CountingRefresher {
        calls: AtomicUsize,
        expires_at: String,
        access_token: String,
        refresh_token: String,
        started: StdMutex<Option<oneshot::Sender<()>>>,
        release: StdMutex<Option<oneshot::Receiver<()>>>,
        fail: bool,
    }

    impl fmt::Debug for CountingRefresher {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("CountingRefresher")
                .field("calls", &self.calls.load(Ordering::SeqCst))
                .finish_non_exhaustive()
        }
    }

    #[async_trait]
    impl OAuthTokenRefresher for CountingRefresher {
        async fn refresh(
            &self,
            _provider: &ProviderId,
            _credential: &StoredCredential,
        ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(started) = self.started.lock().unwrap_or_abort().take() {
                let _ = started.send(());
            }
            let release = self.release.lock().unwrap_or_abort().take();
            if let Some(release) = release {
                let _ = release.await;
            }
            if self.fail {
                return Err(CredentialRefreshError::new(
                    ProviderErrorCategory::TransportFailure,
                    "transient idp failure",
                ));
            }
            Ok(OAuthRefreshOutcome {
                access_token: self.access_token.clone(),
                refresh_token: Some(self.refresh_token.clone()),
                expires_at: Some(self.expires_at.clone()),
                account_id: Some("acct-new".to_string()),
                scopes: vec!["openid".to_string()],
            })
        }
    }

    fn fixed_now() -> SystemTime {
        humantime::parse_rfc3339("2026-05-30T00:00:00Z").unwrap_or_abort()
    }

    fn manager_with_oauth(
        store: CredentialStore,
        access: &str,
        refresh: Option<&str>,
        expires_at: &str,
        refresher: Arc<CountingRefresher>,
    ) -> ProviderCredentialManager {
        let mut credential = StoredCredential::oauth(
            ProviderId::codex(),
            access,
            refresh.unwrap_or(""),
            Some(expires_at.to_string()),
            "2026-05-29T00:00:00Z",
        );
        if refresh.is_none() {
            credential.refresh_token = None;
        } else if refresh == Some("") {
            credential.refresh_token = Some(String::new());
        }
        store.save(&credential).unwrap_or_abort();
        let r: Arc<dyn OAuthTokenRefresher> = refresher;
        ProviderCredentialManager::new(store, ProviderId::codex(), Vec::new(), "", |_| None)
            .with_clock(Arc::new(FixedClock(fixed_now())))
            .with_refresher(r)
    }

    fn near_expiry_snapshot() -> CredentialExpirySnapshot {
        let now = 1_748_563_200_000i64;
        CredentialExpirySnapshot {
            expires_at_unix_ms: Some(now + 120_000),
            now_unix_ms: now,
            leeway_ms: DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS,
        }
    }

    fn fresh_snapshot() -> CredentialExpirySnapshot {
        let now = 1_748_563_200_000i64;
        CredentialExpirySnapshot {
            expires_at_unix_ms: Some(now + 3_600_000),
            now_unix_ms: now,
            leeway_ms: DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS,
        }
    }

    #[test]
    fn sleep_wake_policy_is_active_hook_strategy() {
        let policy = evaluate_sleep_wake_credential_refresh();
        assert!(policy.is_active());
        assert!(!policy.is_noop_or_unavailable());
        match &policy {
            SleepWakeCredentialPolicy::Active { strategy } => {
                assert_eq!(strategy, HOOK_EVENT_SOURCE_STRATEGY);
            }
            other => panic!("expected Active, got {other:?}"),
        }
        assert!(policy.one_line().contains("active (strategy=hook)"));
    }

    #[test]
    fn availability_alias_is_active_when_hook_source_is_product_default() {
        let availability = sleep_wake_credential_refresh_availability();
        assert!(availability.is_active());
        assert!(availability.one_line().contains("active"));
    }

    #[test]
    fn platform_event_source_opens_active_hook() {
        let opened = open_platform_sleep_wake_event_source();
        assert!(opened.is_active());
        assert!(opened.policy().is_active());
        match opened {
            PlatformSleepWakeEventSource::Active {
                strategy,
                source,
                injector,
            } => {
                assert_eq!(strategy, HOOK_EVENT_SOURCE_STRATEGY);
                assert_eq!(source.strategy(), HOOK_EVENT_SOURCE_STRATEGY);
                injector.inject(SleepWakeHostEvent::Wake).unwrap_or_abort();
            }
            PlatformSleepWakeEventSource::Unavailable { reason } => {
                panic!("expected Active, got Unavailable: {reason}");
            }
        }
    }

    #[test]
    fn unavailable_event_source_is_structured_fail_closed() {
        let opened = unavailable_sleep_wake_event_source("no native wake API on this host");
        assert!(!opened.is_active());
        match opened.policy() {
            SleepWakeCredentialPolicy::Unavailable { reason } => {
                assert!(reason.contains("no native wake API"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn observe_host_events_records_active_policy() {
        for event in [
            SleepWakeHostEvent::Sleep,
            SleepWakeHostEvent::Wake,
            SleepWakeHostEvent::Resume,
            SleepWakeHostEvent::Suspend,
        ] {
            let observation = observe_sleep_wake_host_event(event);
            match &observation {
                SleepWakeObservation::Recorded {
                    event: observed,
                    policy,
                } => {
                    assert_eq!(*observed, event);
                    assert!(policy.is_active());
                    assert!(!observation.is_recorded_noop());
                    assert!(observation.one_line().contains("policy=active"));
                }
            }
        }
    }

    #[test]
    fn sleep_wake_operator_diagnostics_cover_active_policy() {
        let policy = evaluate_sleep_wake_credential_refresh();
        let observations = [
            observe_sleep_wake_host_event(SleepWakeHostEvent::Sleep),
            observe_sleep_wake_host_event(SleepWakeHostEvent::Wake),
        ];
        let summary = summarize_sleep_wake_observations(&observations);
        assert!(policy.one_line().contains("active"));
        assert_eq!(summary.total, 2);
        assert_eq!(summary.recorded, 2);
        assert_eq!(summary.recorded_noop, 0);
        assert!(!summary.all_recorded_noop());
        assert!(summary.one_line().contains("2 recorded"));
    }

    #[test]
    fn decide_sleep_wake_skips_refresh_without_expiry_context() {
        for event in [
            SleepWakeHostEvent::Sleep,
            SleepWakeHostEvent::Wake,
            SleepWakeHostEvent::Resume,
            SleepWakeHostEvent::Suspend,
        ] {
            let decision = decide_sleep_wake_credential_refresh(event);
            assert!(decision.is_skip());
            assert!(!decision.claims_refresh());
            assert_eq!(decision.clone().event(), event);
        }
    }

    #[test]
    fn decide_recommends_refresh_on_wake_when_credentials_near_expiry() {
        let expiry = near_expiry_snapshot();
        let wake =
            decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Wake, Some(&expiry));
        let resume =
            decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Resume, Some(&expiry));
        let sleep =
            decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Sleep, Some(&expiry));
        assert!(wake.is_refresh());
        assert!(wake.one_line().contains("refresh recommended"));
        assert!(resume.is_refresh());
        assert!(sleep.is_skip());
    }

    #[test]
    fn decide_skips_refresh_on_wake_when_credentials_still_fresh() {
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&fresh_snapshot()),
        );
        assert!(decision.is_skip());
        assert!(decision.one_line().contains("still fresh"));
    }

    #[test]
    fn decide_skips_when_expiry_unknown_even_on_wake() {
        let expiry = CredentialExpirySnapshot::with_default_leeway(None, 1_700_000_000_000);
        let decision =
            decide_sleep_wake_credential_refresh_for(SleepWakeHostEvent::Wake, Some(&expiry));
        assert!(decision.is_skip());
        assert!(decision.one_line().contains("expiry unknown"));
    }

    #[test]
    fn credential_expiry_snapshot_near_expiry_includes_already_expired() {
        let now = 1_000i64;
        let expired = CredentialExpirySnapshot {
            expires_at_unix_ms: Some(now - 1),
            now_unix_ms: now,
            leeway_ms: 100,
        };
        assert!(expired.is_near_expiry());
        assert_eq!(expired.remaining_ms(), Some(-1));
    }

    #[test]
    fn observe_and_decide_dual_cycle_records_active_and_skip_without_expiry() {
        let mut observations = Vec::new();
        let mut decisions = Vec::new();
        for _cycle in 0..2 {
            for event in [
                SleepWakeHostEvent::Sleep,
                SleepWakeHostEvent::Wake,
                SleepWakeHostEvent::Resume,
                SleepWakeHostEvent::Suspend,
            ] {
                let (observation, decision) = observe_and_decide_sleep_wake_host_event(event);
                observations.push(observation);
                decisions.push(decision);
            }
        }
        let summary = summarize_sleep_wake_observations(&observations);
        assert_eq!(summary.total, 8);
        assert_eq!(summary.recorded, 8);
        assert_eq!(summary.recorded_noop, 0);
        assert!(decisions.iter().all(|d| d.is_skip()));
    }

    #[tokio::test]
    async fn execute_skips_when_decision_is_skip_fresh_token() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: false,
        });
        let manager = manager_with_oauth(
            store,
            "access-old",
            Some("refresh-old"),
            "2026-05-30T01:00:00Z",
            Arc::clone(&refresher),
        );
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&fresh_snapshot()),
        );
        let cancel = CancellationToken::new();

        let execution = execute_sleep_wake_refresh_decision(&decision, &manager, &cancel).await;

        assert!(execution.is_skipped());
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn execute_near_expiry_single_refresh_and_persists_replacement() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: false,
        });
        let manager = manager_with_oauth(
            store.clone(),
            "access-old",
            Some("refresh-old"),
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        );
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&near_expiry_snapshot()),
        );
        assert!(decision.is_refresh());
        let cancel = CancellationToken::new();

        let execution = execute_sleep_wake_refresh_decision(&decision, &manager, &cancel).await;
        let _execution2 = execute_sleep_wake_refresh_decision(&decision, &manager, &cancel).await;

        assert!(
            execution.is_refreshed(),
            "execution={}",
            execution.one_line()
        );
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
        let stored = store
            .load(&ProviderId::codex())
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(stored.access_token.as_deref(), Some("access-new"));
        assert_eq!(stored.refresh_token.as_deref(), Some("refresh-new"));
        assert_eq!(stored.expires_at.as_deref(), Some("2026-05-31T00:00:00Z"));
    }

    #[tokio::test]
    async fn execute_transient_failure_is_failed_without_store_replacement() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: true,
        });
        let manager = manager_with_oauth(
            store.clone(),
            "access-old",
            Some("refresh-old"),
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        );
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&near_expiry_snapshot()),
        );
        let cancel = CancellationToken::new();

        let execution = execute_sleep_wake_refresh_decision(&decision, &manager, &cancel).await;

        assert!(execution.is_failed(), "execution={}", execution.one_line());
        assert!(execution.one_line().contains("transient"));
        let stored = store
            .load(&ProviderId::codex())
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(stored.access_token.as_deref(), Some("access-old"));
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_missing_refresh_token_is_failed_unavailable() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: false,
        });
        let manager = manager_with_oauth(
            store.clone(),
            "access-old",
            None,
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        );
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&near_expiry_snapshot()),
        );
        let cancel = CancellationToken::new();

        let execution = execute_sleep_wake_refresh_decision(&decision, &manager, &cancel).await;

        assert!(execution.is_failed(), "execution={}", execution.one_line());
        assert!(
            execution.one_line().contains("cannot be refreshed")
                || execution.one_line().contains("RefreshUnavailable")
                || execution.one_line().contains("expired and cannot"),
            "execution={}",
            execution.one_line()
        );
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 0);
        let stored = store
            .load(&ProviderId::codex())
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(stored.access_token.as_deref(), Some("access-old"));
    }

    #[tokio::test]
    async fn execute_cancellation_before_refresh() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: false,
        });
        let manager = manager_with_oauth(
            store,
            "access-old",
            Some("refresh-old"),
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        );
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&near_expiry_snapshot()),
        );
        let cancel = CancellationToken::new();
        cancel.cancel();

        let execution = execute_sleep_wake_refresh_decision(&decision, &manager, &cancel).await;

        assert!(
            execution.is_cancelled(),
            "execution={}",
            execution.one_line()
        );
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn execute_cancellation_during_refresh() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(Some(started_tx)),
            release: StdMutex::new(Some(release_rx)),
            fail: false,
        });
        let manager = Arc::new(manager_with_oauth(
            store,
            "access-old",
            Some("refresh-old"),
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        ));
        let decision = decide_sleep_wake_credential_refresh_for(
            SleepWakeHostEvent::Wake,
            Some(&near_expiry_snapshot()),
        );
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let handle = tokio::spawn({
            let manager = Arc::clone(&manager);
            let decision = decision.clone();
            async move { execute_sleep_wake_refresh_decision(&decision, &manager, &cancel_clone).await }
        });
        started_rx.await.unwrap_or_abort();
        cancel.cancel();
        tokio::task::yield_now().await;
        let _ = release_tx.send(());
        let execution = handle.await.unwrap_or_abort();

        assert!(
            execution.is_cancelled() || execution.is_refreshed(),
            "execution={}",
            execution.one_line()
        );
    }

    #[tokio::test]
    async fn hook_source_loop_near_expiry_refreshes_once() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: false,
        });
        let manager = manager_with_oauth(
            store.clone(),
            "access-old",
            Some("refresh-old"),
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        );
        let (mut source, injector) = HookSleepWakeEventSource::open();
        let cancel = CancellationToken::new();

        injector.inject(SleepWakeHostEvent::Sleep).unwrap_or_abort();
        injector.inject(SleepWakeHostEvent::Wake).unwrap_or_abort();
        injector.inject(SleepWakeHostEvent::Wake).unwrap_or_abort();
        drop(injector);

        let near = near_expiry_snapshot();
        let executions = run_sleep_wake_refresh_loop(
            &mut source,
            &manager,
            |event| {
                if event.may_trigger_refresh_evaluation() {
                    Some(near)
                } else {
                    None
                }
            },
            cancel,
        )
        .await;

        assert_eq!(executions.len(), 3);
        assert!(executions[0].is_skipped());
        assert!(
            executions[1].is_refreshed(),
            "first wake: {}",
            executions[1].one_line()
        );
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
        let stored = store
            .load(&ProviderId::codex())
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(stored.access_token.as_deref(), Some("access-new"));
    }

    #[tokio::test]
    async fn observe_decide_and_execute_product_path_on_resume() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
            access_token: "access-new".to_string(),
            refresh_token: "refresh-new".to_string(),
            started: StdMutex::new(None),
            release: StdMutex::new(None),
            fail: false,
        });
        let manager = manager_with_oauth(
            store,
            "access-old",
            Some("refresh-old"),
            "2026-05-30T00:02:00Z",
            Arc::clone(&refresher),
        );
        let cancel = CancellationToken::new();
        let (obs, decision, execution) = observe_decide_and_execute_sleep_wake_host_event(
            SleepWakeHostEvent::Resume,
            Some(&near_expiry_snapshot()),
            &manager,
            &cancel,
        )
        .await;
        assert!(obs.is_recorded());
        assert!(decision.is_refresh());
        assert!(execution.is_refreshed());
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
    }
}
