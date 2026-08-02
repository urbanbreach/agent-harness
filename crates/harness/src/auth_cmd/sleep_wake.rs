//! Sleep/wake credential-refresh simulation CLI surface (`harness auth sleep-wake-simulate`).
//!
//! Surfaces the real observe → decide → (optionally execute) pipeline from
//! [`harness_core::sleep_wake_auth`] behind an operator command. The operator
//! supplies the host event and optional credential-expiry snapshot; the decision
//! half is always exercised. With `--execute`, the execution half runs against
//! the credential store (best-effort: no OAuth refresher is configured in the
//! CLI context, so a Refresh decision reports Failed gracefully).

use std::io::Write;

use clap::Args;
use harness_core::sleep_wake_auth::{
    observe_and_decide_sleep_wake_host_event_for, CredentialExpirySnapshot, SleepWakeHostEvent,
    SleepWakeRefreshDecision,
};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(super) struct SleepWakeSimulateCommand {
    /// Simulated host power event: sleep, wake, resume, or suspend.
    #[arg(long)]
    event: String,

    /// Credential expiry timestamp in unix milliseconds (enables refresh evaluation).
    #[arg(long = "expires-at-ms")]
    expires_at_ms: Option<i64>,

    /// Current timestamp in unix milliseconds (default: 0; use with --expires-at-ms).
    #[arg(long = "now-ms", default_value_t = 0)]
    now_ms: i64,

    /// Also execute the refresh decision against the credential store (best-effort).
    #[arg(long, default_value_t = false)]
    execute: bool,
}

#[derive(Debug, Serialize)]
struct SleepWakeSimulateOutput {
    event: String,
    observation_policy: String,
    decision: String,
    decision_detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution: Option<String>,
}

fn parse_event(raw: &str) -> Option<SleepWakeHostEvent> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "sleep" => Some(SleepWakeHostEvent::Sleep),
        "wake" => Some(SleepWakeHostEvent::Wake),
        "resume" => Some(SleepWakeHostEvent::Resume),
        "suspend" => Some(SleepWakeHostEvent::Suspend),
        _ => None,
    }
}

pub(super) fn run_sleep_wake_simulate(
    cmd: SleepWakeSimulateCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let Some(event) = parse_event(&cmd.event) else {
        let _ = writeln!(
            io.stderr,
            "auth sleep-wake-simulate: invalid event `{}` (expected sleep, wake, resume, or suspend)",
            cmd.event
        );
        return 2;
    };

    let expiry = cmd.expires_at_ms.map(|expires_at| {
        CredentialExpirySnapshot::with_default_leeway(Some(expires_at), cmd.now_ms)
    });

    let (observation, decision) =
        observe_and_decide_sleep_wake_host_event_for(event, expiry.as_ref());

    let observation_policy = match &observation {
        harness_core::sleep_wake_auth::SleepWakeObservation::Recorded { policy, .. } => {
            policy.one_line()
        }
    };

    let (decision_label, decision_detail, remaining_ms) = match &decision {
        SleepWakeRefreshDecision::Skip { reason, .. } => ("skip".to_string(), reason.clone(), None),
        SleepWakeRefreshDecision::Refresh {
            reason,
            remaining_ms,
            ..
        } => ("refresh".to_string(), reason.clone(), Some(*remaining_ms)),
    };

    let execution = if cmd.execute {
        let execution = execute_decision(&decision, io, deps);
        Some(execution)
    } else {
        None
    };

    let output = SleepWakeSimulateOutput {
        event: event.as_str().to_string(),
        observation_policy,
        decision: decision_label,
        decision_detail,
        remaining_ms,
        execution,
    };

    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth sleep-wake-simulate: failed to serialize JSON: {err}"
            );
            1
        }
    }
}

fn execute_decision(
    decision: &SleepWakeRefreshDecision,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> String {
    use super::support::credential_store_from_deps;
    use harness_core::auth::ProviderCredentialManager;
    use harness_core::sleep_wake_auth::execute_sleep_wake_refresh_decision;
    use tokio_util::sync::CancellationToken;

    let Some(store) = credential_store_from_deps(deps) else {
        let _ = writeln!(
            io.stderr,
            "auth sleep-wake-simulate: cannot resolve credential store for --execute"
        );
        return "no_credential_store".to_string();
    };

    let manager = ProviderCredentialManager::new(
        store,
        harness_core::auth::AuthProviderId::codex(),
        Vec::new(),
        "",
        |_| None,
    );
    let cancel = CancellationToken::new();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth sleep-wake-simulate: failed to build async runtime: {err}"
            );
            return "runtime_error".to_string();
        }
    };

    let execution = runtime.block_on(execute_sleep_wake_refresh_decision(
        decision, &manager, &cancel,
    ));
    execution.one_line()
}
