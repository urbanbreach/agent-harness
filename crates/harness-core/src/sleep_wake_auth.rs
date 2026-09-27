use serde::{Deserialize, Serialize};
mod refresh;
pub use refresh::*;
pub const DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS: i64 = 300_000;
pub const HOOK_EVENT_SOURCE_STRATEGY: &str = "hook";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SleepWakeCredentialPolicy {
    Active { strategy: String },
    NoOp { reason: String },
    Unavailable { reason: String },
}
impl SleepWakeCredentialPolicy {
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Active { .. })
    }
    pub fn is_noop_or_unavailable(&self) -> bool {
        !self.is_active()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Active { strategy } => {
                format!("sleep/wake credential refresh: active (strategy={strategy})")
            }
            Self::NoOp { reason } => format!("sleep/wake credential refresh: noop ({reason})"),
            Self::Unavailable { reason } => {
                format!("sleep/wake credential refresh: unavailable ({reason})")
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepWakeHostEvent {
    Sleep,
    Wake,
    Resume,
    Suspend,
}
impl SleepWakeHostEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sleep => "sleep",
            Self::Wake => "wake",
            Self::Resume => "resume",
            Self::Suspend => "suspend",
        }
    }
    pub fn may_trigger_refresh_evaluation(self) -> bool {
        matches!(self, Self::Wake | Self::Resume)
    }
}
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
        let Self::Recorded { event, policy } = self;
        format!(
            "sleep/wake observe: {} recorded ({})",
            event.as_str(),
            policy.one_line()
        )
    }
    pub fn is_recorded(&self) -> bool {
        true
    }
    pub fn is_recorded_noop(&self) -> bool {
        let Self::Recorded { policy, .. } = self;
        policy.is_noop_or_unavailable()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SleepWakeObservationSummary {
    pub recorded: usize,
    pub recorded_noop: usize,
    pub total: usize,
}
impl SleepWakeObservationSummary {
    pub fn one_line(&self) -> String {
        format!(
            "sleep/wake observations: {} recorded ({} total; {} noop-policy)",
            self.recorded, self.total, self.recorded_noop
        )
    }
    pub fn all_recorded_noop(&self) -> bool {
        self.total > 0 && self.recorded_noop == self.total
    }
}
pub fn summarize_sleep_wake_observations(
    observations: &[SleepWakeObservation],
) -> SleepWakeObservationSummary {
    SleepWakeObservationSummary {
        total: observations.len(),
        recorded: observations.len(),
        recorded_noop: observations.iter().filter(|o| o.is_recorded_noop()).count(),
    }
}
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
            .map(|expiry| expiry.saturating_sub(self.now_unix_ms))
    }
    pub fn is_near_expiry(self) -> bool {
        self.remaining_ms()
            .is_some_and(|left| left <= self.leeway_ms.max(0))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum SleepWakeRefreshDecision {
    Skip {
        event: SleepWakeHostEvent,
        reason: String,
    },
    Refresh {
        event: SleepWakeHostEvent,
        reason: String,
        remaining_ms: i64,
    },
}
impl SleepWakeRefreshDecision {
    pub fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }
    pub fn is_refresh(&self) -> bool {
        matches!(self, Self::Refresh { .. })
    }
    pub fn claims_refresh(&self) -> bool {
        self.is_refresh()
    }
    pub fn event(&self) -> SleepWakeHostEvent {
        match self {
            Self::Skip { event, .. } | Self::Refresh { event, .. } => *event,
        }
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Skip { event, reason } => format!("sleep/wake decision: skip refresh for {} ({reason})", event.as_str()),
            Self::Refresh { event, reason, remaining_ms } => format!("sleep/wake decision: refresh recommended for {} (remaining_ms={remaining_ms}; {reason})", event.as_str()),
        }
    }
}
pub fn decide_sleep_wake_credential_refresh(event: SleepWakeHostEvent) -> SleepWakeRefreshDecision {
    decide_sleep_wake_credential_refresh_for(event, None)
}
pub fn decide_sleep_wake_credential_refresh_for(
    event: SleepWakeHostEvent,
    expiry: Option<&CredentialExpirySnapshot>,
) -> SleepWakeRefreshDecision {
    let skip = |reason: &str| SleepWakeRefreshDecision::Skip {
        event,
        reason: reason.into(),
    };
    if !event.may_trigger_refresh_evaluation() {
        return skip("event does not require refresh");
    }
    let Some(snapshot) = expiry else {
        return skip("credential expiry snapshot is unavailable");
    };
    let Some(remaining_ms) = snapshot.remaining_ms() else {
        return skip("credential expiry is unknown");
    };
    if !snapshot.is_near_expiry() {
        return skip("credentials are still fresh");
    }
    SleepWakeRefreshDecision::Refresh {
        event,
        remaining_ms,
        reason: "credentials are near expiry".into(),
    }
}
pub fn evaluate_sleep_wake_credential_refresh() -> SleepWakeCredentialPolicy {
    SleepWakeCredentialPolicy::Active {
        strategy: HOOK_EVENT_SOURCE_STRATEGY.into(),
    }
}
pub fn sleep_wake_credential_refresh_availability() -> SleepWakeCredentialPolicy {
    evaluate_sleep_wake_credential_refresh()
}
pub fn observe_sleep_wake_host_event(event: SleepWakeHostEvent) -> SleepWakeObservation {
    SleepWakeObservation::Recorded {
        event,
        policy: evaluate_sleep_wake_credential_refresh(),
    }
}
pub fn observe_and_decide_sleep_wake_host_event(
    event: SleepWakeHostEvent,
) -> (SleepWakeObservation, SleepWakeRefreshDecision) {
    observe_and_decide_sleep_wake_host_event_for(event, None)
}
pub fn observe_and_decide_sleep_wake_host_event_for(
    event: SleepWakeHostEvent,
    expiry: Option<&CredentialExpirySnapshot>,
) -> (SleepWakeObservation, SleepWakeRefreshDecision) {
    (
        observe_sleep_wake_host_event(event),
        decide_sleep_wake_credential_refresh_for(event, expiry),
    )
}

#[async_trait::async_trait]
pub trait SleepWakeEventSource: Send {
    fn strategy(&self) -> &'static str;
    async fn recv(&mut self) -> Option<SleepWakeHostEvent>;
}
pub struct HookSleepWakeEventSource {
    rx: tokio::sync::mpsc::Receiver<SleepWakeHostEvent>,
}
#[derive(Clone)]
pub struct HookSleepWakeEventInjector {
    tx: tokio::sync::mpsc::Sender<SleepWakeHostEvent>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SleepWakeInjectError {
    #[error("sleep/wake receiver is closed")]
    Closed,
    #[error("sleep/wake queue is full")]
    Full,
}
impl HookSleepWakeEventSource {
    pub fn open() -> (Self, HookSleepWakeEventInjector) {
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        (Self { rx }, HookSleepWakeEventInjector { tx })
    }
}
impl HookSleepWakeEventInjector {
    pub fn inject(&self, event: SleepWakeHostEvent) -> Result<(), SleepWakeInjectError> {
        self.tx.try_send(event).map_err(|error| match error {
            tokio::sync::mpsc::error::TrySendError::Full(_) => SleepWakeInjectError::Full,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => SleepWakeInjectError::Closed,
        })
    }
}
#[async_trait::async_trait]
impl SleepWakeEventSource for HookSleepWakeEventSource {
    fn strategy(&self) -> &'static str {
        HOOK_EVENT_SOURCE_STRATEGY
    }
    async fn recv(&mut self) -> Option<SleepWakeHostEvent> {
        self.rx.recv().await
    }
}
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
                strategy: (*strategy).into(),
            },
            Self::Unavailable { reason } => SleepWakeCredentialPolicy::Unavailable {
                reason: reason.clone(),
            },
        }
    }
}
pub fn open_platform_sleep_wake_event_source() -> PlatformSleepWakeEventSource {
    let (source, injector) = HookSleepWakeEventSource::open();
    PlatformSleepWakeEventSource::Active {
        strategy: HOOK_EVENT_SOURCE_STRATEGY,
        source,
        injector,
    }
}
pub fn unavailable_sleep_wake_event_source(
    reason: impl Into<String>,
) -> PlatformSleepWakeEventSource {
    PlatformSleepWakeEventSource::Unavailable {
        reason: reason.into(),
    }
}
