use super::*;

#[derive(Clone, Copy)]
pub(super) enum WaitHint {
    NotRequested,
    Elapsed { requested: u64, waited: u64 },
    ReturnedEarly,
}

impl WaitHint {
    pub(super) fn apply(
        self,
        mut result: GetCommandOrSubagentOutputResult,
        child: bool,
    ) -> GetCommandOrSubagentOutputResult {
        if result.status != "running" && !(child && result.status == "initializing") {
            return result;
        }
        let noun = if child { "subagent" } else { "task" };
        let stop = if child {
            " and do not tell it to stop"
        } else {
            ""
        };
        let lead = match self {
            Self::NotRequested => "Use timeout_ms to wait for completion.".into(),
            Self::ReturnedEarly => "Wait returned early because another finished.".into(),
            Self::Elapsed { requested, waited } => {
                if requested > waited {
                    format!(
                        "Waited {}, the per-call maximum, of the {} you requested.",
                        duration(waited),
                        duration(requested)
                    )
                } else {
                    format!("Waited the requested {}.", duration(waited))
                }
            }
        };
        result.output.push_str(&format!("\n\n{lead} Unless the user specified, do not kill this {noun}{stop} just because this wait returned. It is still working. You will be notified automatically when it completes. Do other work, or wait again with a longer timeout_ms."));
        result
    }
}

fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{}s", ms / 1000)
    }
}

pub(super) async fn wait_subscriptions(
    subscriptions: &[Subscription],
    any: bool,
    timeout: u64,
    cancellation: CancellationToken,
    interject: Option<watch::Receiver<u64>>,
) -> Result<WaitHint, CoordinatorError> {
    if timeout == 0 {
        return Ok(if any {
            WaitHint::Elapsed {
                requested: 0,
                waited: 0,
            }
        } else {
            WaitHint::NotRequested
        });
    }
    let mut waits = tokio::task::JoinSet::new();
    for subscription in subscriptions
        .iter()
        .filter(|subscription| subscription.pending())
    {
        let subscription = subscription.cloned();
        waits.spawn(subscription.wait_terminal());
    }
    if waits.is_empty() {
        return Ok(WaitHint::ReturnedEarly);
    }
    let pending = async {
        while let Some(result) = waits.join_next().await {
            result.map_err(|_| CoordinatorError::Closed)??;
            if any {
                return Ok(());
            }
        }
        Ok::<(), CoordinatorError>(())
    };
    let interrupted = async {
        match interject {
            Some(mut interject) => {
                let _ = interject.changed().await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let waited = timeout.min(milliseconds("HARNESS_MAX_WAIT_BLOCK_MS", 3_600_000));
    let elapsed = WaitHint::Elapsed {
        requested: timeout,
        waited,
    };
    let deadline = tokio::time::Instant::now() + Duration::from_millis(waited);
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(CoordinatorError::Cancelled("wait cancelled".into())),
        () = interrupted => Err(CoordinatorError::Cancelled("wait interrupted by a subagent interject".into())),
        result = tokio::time::timeout_at(deadline, pending) => {
            match result {
                Ok(result) => {
                    result?;
                    Ok(if tokio::time::Instant::now() >= deadline { elapsed } else { WaitHint::ReturnedEarly })
                },
                Err(_) => Ok(elapsed),
            }
        }
    }
}

pub(super) fn aggregate(
    mode: &str,
    subscriptions: &[Subscription],
    hint: WaitHint,
) -> GetCommandOrSubagentOutputResults {
    let results: Vec<_> = subscriptions
        .iter()
        .map(|subscription| subscription.result(hint))
        .collect();
    let completed = subscriptions
        .iter()
        .filter(|subscription| {
            !subscription.pending() && !matches!(subscription, Subscription::Missing(_))
        })
        .count();
    GetCommandOrSubagentOutputResults {
        mode: mode.into(),
        summary: format!("{completed}/{} tasks completed ({mode})", results.len()),
        results,
    }
}
