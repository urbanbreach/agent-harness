use super::*;
use crate::auth::{CredentialResolveError, ProviderCredentialManager, ResolvedCredential};
use std::{collections::VecDeque, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
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
    pub fn is_refreshed(&self) -> bool {
        matches!(self, Self::Refreshed { .. })
    }
    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
    pub fn is_cancelled(&self) -> bool {
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
                expires_at.as_deref().unwrap_or("unknown"),
                decision.one_line()
            ),
            Self::Failed { decision, error } => format!(
                "sleep/wake execute: failed ({error}; {})",
                decision.one_line()
            ),
            Self::Cancelled { decision, reason } => format!(
                "sleep/wake execute: cancelled ({reason}; {})",
                decision.one_line()
            ),
        }
    }
}
pub async fn execute_sleep_wake_refresh_decision(
    decision: &SleepWakeRefreshDecision,
    manager: &ProviderCredentialManager,
    cancel: &CancellationToken,
) -> SleepWakeRefreshExecution {
    let decision = decision.clone();
    let cancelled = || SleepWakeRefreshExecution::Cancelled {
        decision: decision.clone(),
        reason: "cancellation requested".into(),
    };
    if cancel.is_cancelled() {
        return cancelled();
    }
    let SleepWakeRefreshDecision::Refresh { remaining_ms, .. } = &decision else {
        return SleepWakeRefreshExecution::Skipped { decision };
    };
    let leeway = Duration::from_millis(
        u64::try_from(DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS.max(remaining_ms.saturating_add(1)))
            .unwrap_or(300_000),
    );
    tokio::select! {
        biased;
        () = cancel.cancelled() => cancelled(),
        result = manager.refresh_oauth_if_near_expiry(leeway) => match result {
            Ok(resolved) => SleepWakeRefreshExecution::Refreshed { token_source: resolved_credential_token_source(&resolved), expires_at: resolved.expires_at, decision },
            Err(error) => SleepWakeRefreshExecution::Failed { error: credential_resolve_error_one_line(&error), decision },
        }
    }
}
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
/// Runs until the source closes or cancellation arrives, returning the last 256 outcomes.
pub async fn run_sleep_wake_refresh_loop(
    source: &mut dyn SleepWakeEventSource,
    manager: &ProviderCredentialManager,
    expiry_for_event: impl Fn(SleepWakeHostEvent) -> Option<CredentialExpirySnapshot>,
    cancel: CancellationToken,
) -> Vec<SleepWakeRefreshExecution> {
    // ponytail: retain 256 diagnostics; stream them to an observer if full lifetime history is needed.
    let mut outcomes = VecDeque::new();
    loop {
        let event = tokio::select! { biased; () = cancel.cancelled() => break, event = source.recv() => event };
        let Some(event) = event else {
            break;
        };
        let decision =
            decide_sleep_wake_credential_refresh_for(event, expiry_for_event(event).as_ref());
        let execution = execute_sleep_wake_refresh_decision(&decision, manager, &cancel).await;
        if outcomes.len() == 256 {
            outcomes.pop_front();
        }
        outcomes.push_back(execution);
    }
    outcomes.into_iter().collect()
}
pub fn resolved_credential_token_source(resolved: &ResolvedCredential) -> String {
    use crate::auth::ResolvedCredentialSource;
    match &resolved.source {
        ResolvedCredentialSource::StoredOauth => "StoredOauth".into(),
        ResolvedCredentialSource::StoredApiKey => "StoredApiKey".into(),
        ResolvedCredentialSource::EnvApiKey { env } => format!("EnvApiKey({env})"),
        ResolvedCredentialSource::InlineApiKey => "InlineApiKey".into(),
    }
}
pub fn credential_resolve_error_one_line(error: &CredentialResolveError) -> String {
    error.category().as_str().into()
}
