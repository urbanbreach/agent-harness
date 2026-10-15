//! The candidate walk itself, independent of any worker so its rules are testable.
use super::candidates::{Candidate, WalkMode};
use super::extract::{cancelled_error, classify, is_cancelled};
use crate::config::CompactionRetrySettings;
use crate::coord::compaction::{CompactionError, CompactionErrorKind};
use harness_compaction::retry;
use harness_providers::CompactionCredential;
use tokio_util::sync::CancellationToken;

/// How a walk behaves; the credentials error is what exhausted or unauthenticated walks
/// report.
pub(super) struct WalkPlan<'a> {
    pub mode: WalkMode,
    pub retry: &'a CompactionRetrySettings,
    pub cancel: &'a CancellationToken,
    pub credentials: CompactionError,
}

/// A native failure remembered by the walk, with the provider whose route it came from.
struct NativeFailure {
    error: CompactionError,
    provider: String,
}

#[derive(Default)]
struct Memory {
    last_error: Option<CompactionError>,
    first_native: Option<NativeFailure>,
}

impl Memory {
    /// A native failure; the first one stays authoritative.
    fn native(&mut self, error: CompactionError, candidate: &Candidate) {
        if self.first_native.is_none() {
            self.last_error = Some(error.clone());
            self.first_native = Some(NativeFailure {
                error,
                provider: candidate.provider.clone(),
            });
        }
    }

    /// A failure that ends the current candidate.
    fn record(&mut self, error: CompactionError, candidate: &Candidate) {
        if error.kind == CompactionErrorKind::Native {
            self.native(error, candidate);
        } else {
            self.last_error = Some(error);
        }
    }

    /// An authentication failure advances. It supplies the provider-specific credentials
    /// error unless a native failure is already remembered.
    fn auth(&mut self, credentials: &CompactionError) {
        if self.first_native.is_none() {
            self.last_error = Some(credentials.clone());
        }
    }
}

/// Walks `candidates` until `attempt` succeeds.
///
/// Each candidate first resolves a credential outside the attempt: none skips it, and a
/// failed lookup ends the walk. `attempt` receives the candidate with its credential and
/// the zero-based retry counter.
///
/// Automatic walks retry per candidate under `plan.retry` (never after a timeout or on
/// context overflow) and advance when the next wait exceeds the cutoff and a later slot
/// exists. Manual and speculative walks never back off: auth failures advance, native
/// failures advance inside the native boundary and everything else propagates. After a
/// native failure only same-provider native-capable candidates may follow; reaching any
/// other candidate throws the first native failure.
pub(super) async fn walk<T, F, Fut, R, RFut>(
    plan: &WalkPlan<'_>,
    candidates: Vec<Candidate>,
    mut resolve: R,
    mut attempt: F,
) -> Result<T, CompactionError>
where
    R: FnMut(&Candidate) -> RFut,
    RFut: Future<Output = Result<Option<CompactionCredential>, CompactionError>>,
    F: FnMut(&Candidate, u32) -> Fut,
    Fut: Future<Output = Result<T, CompactionError>>,
{
    let kind = CompactionErrorKind::Summary;
    let total = candidates.len();
    let mut memory = Memory::default();
    for (index, candidate) in candidates.iter().enumerate() {
        if plan.cancel.is_cancelled() {
            return Err(cancelled_error(kind));
        }
        let Some(credential) = resolve(candidate).await? else {
            continue;
        };
        if let Some(native) = &memory.first_native
            && !(candidate.provider == native.provider && candidate.native_capable)
        {
            return Err(native.error.clone());
        }
        let candidate = Candidate {
            credential: Some(credential),
            ..candidate.clone()
        };
        let mut attempts = 0_u32;
        loop {
            let error = match attempt(&candidate, attempts).await {
                Ok(value) => return Ok(value),
                Err(error) => error,
            };
            if plan.cancel.is_cancelled() || is_cancelled(&error) {
                return Err(error);
            }
            if error.auth {
                memory.auth(&plan.credentials);
                break;
            }
            if plan.mode != WalkMode::Automatic {
                if error.kind == CompactionErrorKind::Native {
                    memory.native(error, &candidate);
                    break;
                }
                return Err(error);
            }
            let class = classify(&error);
            let policy = plan.retry;
            let retry_here = policy.enabled
                && attempts < policy.max_retries
                && !class.timeout
                && class.retryable();
            if !retry_here {
                memory.record(error, &candidate);
                break;
            }
            let delay = retry::automatic_delay_ms(policy.base_delay_ms, attempts, class.hint_ms);
            if delay > retry::ADVANCE_CUTOFF_MS && index + 1 < total {
                memory.record(error, &candidate);
                break;
            }
            attempts += 1;
            tokio::select! {
                biased;
                () = plan.cancel.cancelled() => return Err(cancelled_error(kind)),
                () = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {}
            }
        }
    }
    if let Some(error) = memory.last_error {
        return Err(error);
    }
    Err(match plan.mode {
        WalkMode::Automatic => CompactionError::new(kind, "Compaction failed: no available model"),
        WalkMode::Manual | WalkMode::Speculative => plan.credentials.clone(),
    })
}

#[cfg(test)]
mod tests;
