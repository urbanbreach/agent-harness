//! Caller-facing claims on the resume of a turn a manual pass interrupted.
//!
//! A client that is about to dispatch a prompt while a manual pass cleans up joins its
//! barrier through [`CoordinatorHandle::claim_compaction_resume`]. It then either starts a
//! real turn (`release(true)`) or does not (`release(false)`), and the interrupted turn is
//! resumed only when nobody else claimed the session.
use super::super::super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// One counted claim. Releasing it more than once does nothing, whatever the arguments.
pub struct ResumeClaim {
    handle: CoordinatorHandle,
    agent: String,
    released: Arc<AtomicBool>,
}

impl ResumeClaim {
    fn new(handle: CoordinatorHandle, agent: String) -> Self {
        Self {
            handle,
            agent,
            released: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Gives the claim back. `dispatched` is true when a real turn owns the session or the
    /// prompt was queued behind one: the owed resume is then dropped for good. False
    /// (a local command, or a failure before dispatch) lets the last claim to leave schedule
    /// the resume.
    pub async fn release(&self, dispatched: bool) -> Result<(), CoordinatorError> {
        if self.released.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        release(&self.handle, self.agent.clone(), dispatched).await
    }
}

impl std::fmt::Debug for ResumeClaim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResumeClaim")
            .field("agent", &self.agent)
            .field("released", &self.released.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

async fn release(
    handle: &CoordinatorHandle,
    agent: String,
    dispatched: bool,
) -> Result<(), CoordinatorError> {
    handle
        .call(move |s| s.release_resume_claim(&agent, dispatched))
        .await
}

impl Drop for ResumeClaim {
    /// A claim dropped without a release counts as one that started no turn.
    fn drop(&mut self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let (handle, agent) = (self.handle.clone(), self.agent.clone());
            runtime.spawn(async move {
                // Nobody is left to report to; a closed coordinator has no debt to settle.
                let _ = release(&handle, agent, false).await;
            });
        }
    }
}

impl CoordinatorHandle {
    /// Joins the cleanup of the manual pass on `agent` and waits for it. Without a pass the
    /// claim exists only while an earlier claim still keeps the owed resume alive; otherwise
    /// there is nothing to wait for and the result is `None`.
    pub async fn claim_compaction_resume(
        &self,
        agent: impl Into<String>,
    ) -> Result<Option<ResumeClaim>, CoordinatorError> {
        let agent = agent.into();
        let key = agent.clone();
        let Some(joined) = self.call(move |s| Ok(s.manual_claims.join(&key))).await? else {
            return Ok(None);
        };
        // The claim exists before the wait so a cancelled caller still gives it back.
        let claim = ResumeClaim::new(self.clone(), agent);
        if let Some(mut barrier) = joined.barrier {
            // A closed barrier means the pass is gone: also done waiting.
            let _ = barrier.wait_for(|done| *done).await;
        }
        Ok(Some(claim))
    }

    /// Adds a claim only while one is already outstanding.
    pub async fn claim_pending_compaction_resume(
        &self,
        agent: impl Into<String>,
    ) -> Result<Option<ResumeClaim>, CoordinatorError> {
        let agent = agent.into();
        let key = agent.clone();
        let claimed = self
            .call(move |s| Ok(s.manual_claims.claim_pending(&key)))
            .await?;
        Ok(claimed.then(|| ResumeClaim::new(self.clone(), agent)))
    }
}
