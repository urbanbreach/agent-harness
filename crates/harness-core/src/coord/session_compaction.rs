//! Coordinator-side session compaction.
//!
//! The coordinator prepares an immutable session snapshot, runs summary generation outside its
//! command loop, and is the sole authority allowed to commit a `SessionCompaction` event.

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use harness_providers::Provider;

#[cfg(test)]
use crate::clock::Clock;
#[cfg(test)]
use crate::config::CompactionSettings;
#[cfg(test)]
use crate::context_budget::RequestBudgetSnapshot;
#[cfg(test)]
use crate::redact::Redactor;

#[cfg(test)]
use super::{CoordinatorError, RunState};
#[cfg(test)]
use pipeline::generate_session_compaction;
#[cfg(test)]
use prepared::{prepare_session_compaction, SessionCompactionPreparationRequest};

mod budget;
mod completion;
mod lifecycle;
mod pipeline;
mod preparation;
mod prepared;
mod request_context;
mod summary;
mod summary_reducer;
mod typed_preparation;
mod validation;

pub(in crate::coord) use pipeline::GeneratedSessionCompaction;

/// Result of a successful session compaction.
#[derive(Debug, Clone)]
pub struct AppliedCompaction {
    /// The generated compaction summary (including appended file operation tags).
    pub summary: String,
    /// Sequence number of the first event kept after compaction.
    pub first_kept_event_seq: u64,
    /// Estimated token count before compaction.
    pub tokens_before: u32,
    /// Estimated token count after compaction.
    pub tokens_after: u32,
}

/// Runs the compaction phases inline for focused library owners.
///
/// Runtime coordinator requests use the cancellable lifecycle in `lifecycle` so provider work does
/// not block command dispatch. Returns `Ok(None)` when compaction is disabled or not needed.
#[cfg(test)]
pub(in crate::coord) async fn compact_session<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    provider: Arc<dyn Provider>,
    agent_id: &str,
    trigger_reason: &str,
    settings: &CompactionSettings,
    prepared_budget: Option<RequestBudgetSnapshot>,
) -> Result<Option<AppliedCompaction>, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let Some(prepared) = prepare_session_compaction(SessionCompactionPreparationRequest {
        run_state,
        agent_id,
        trigger_reason,
        settings,
        prepared_budget,
    })
    .await?
    else {
        return Ok(None);
    };
    let cancellation = run_state.shutdown_token.child_token();
    let generated = generate_session_compaction(provider, prepared, cancellation).await?;
    generated.commit(clock, redactor, run_state).map(Some)
}
