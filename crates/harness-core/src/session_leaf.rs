//! Session leaf contract.
//!
//! Defines the narrow, typed API boundary through which session leaf owners
//! (start, resume, stop, title update) interact with the coordinator.
//!
//! # Authority preserved by the coordinator
//!
//! - **Event append**: all session lifecycle events (`RunStarted`,
//!   `RunFinished`, `RunFailed`, `SessionTitleUpdated`) are appended by the
//!   coordinator. Leaf owners never write events.
//! - **Lifecycle state**: `RunState` inside the coordinator tracks whether a
//!   run is active, stopped, or failed. Leaf owners observe `RunInfo`
//!   results, never mutate state.
//! - **Resume restoration**: the coordinator restores replay-derived state
//!   from the event store. Leaf owners request resume; they do not replay.
//!
//! Leaf owners call into [`CoordinatorHandle`] through this trait; they do not
//! access `RunState`, `event_helpers`, or the event store directly.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::coord::{CoordinatorError, CoordinatorHandle, RunInfo};

/// Typed request to start a new session run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStartRequest {
    pub run_name: String,
    pub workspace_root: PathBuf,
}

/// Typed request to resume an existing session run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResumeRequest {
    pub run_id: String,
    pub run_name: String,
}

/// Typed request to update the session title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTitleRequest {
    pub title: String,
}

/// Session leaf contract.
///
/// Implementors route all session lifecycle operations through the
/// coordinator handle. No method on this trait appends events or mutates
/// coordinator lifecycle state directly.
///
/// # Integration requests
///
/// If a leaf owner needs a new coordinator-owned session operation, it must
/// produce an integration request describing the exact hunk rather than
/// editing shared coord files.
#[async_trait]
pub trait SessionLeaf: Send + Sync {
    /// Start a new session run through the coordinator.
    ///
    /// The coordinator creates the session directory, appends `RunStarted`,
    /// and returns the typed run info. Leaf owners receive the result only.
    async fn start_session(
        &self,
        request: SessionStartRequest,
    ) -> Result<RunInfo, CoordinatorError>;

    /// Resume an existing session run through the coordinator.
    ///
    /// The coordinator restores replay-derived state from the event store,
    /// appends resume metadata, and returns the typed run info.
    async fn resume_session(
        &self,
        request: SessionResumeRequest,
    ) -> Result<RunInfo, CoordinatorError>;

    /// Stop the active session run through the coordinator.
    ///
    /// The coordinator appends `RunFinished` and releases lifecycle state.
    async fn stop_session(&self) -> Result<(), CoordinatorError>;

    /// Fail the active session run through the coordinator.
    ///
    /// The coordinator appends `RunFailed` and releases lifecycle state.
    async fn fail_session(&self, error: String) -> Result<(), CoordinatorError>;

    /// Update the session title through the coordinator.
    ///
    /// The coordinator appends `SessionTitleUpdated` and returns the
    /// updated run info.
    async fn update_title(&self, request: SessionTitleRequest)
        -> Result<RunInfo, CoordinatorError>;

    /// Get the current run info from the coordinator.
    ///
    /// This is a read-only query that does not append events.
    async fn run_info(&self) -> Result<RunInfo, CoordinatorError>;
}

/// Coordinator-backed implementation of [`SessionLeaf`].
///
/// This is the canonical adapter that routes session operations through the
/// coordinator handle. Leaf owners should depend on the trait, not this
/// struct, so test doubles can substitute.
#[derive(Debug, Clone)]
pub struct CoordinatorSessionLeaf {
    handle: CoordinatorHandle,
}

impl CoordinatorSessionLeaf {
    pub fn new(handle: CoordinatorHandle) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl SessionLeaf for CoordinatorSessionLeaf {
    async fn start_session(
        &self,
        request: SessionStartRequest,
    ) -> Result<RunInfo, CoordinatorError> {
        self.handle
            .start_run(request.run_name, request.workspace_root)
            .await
    }

    async fn resume_session(
        &self,
        request: SessionResumeRequest,
    ) -> Result<RunInfo, CoordinatorError> {
        self.handle
            .resume_run(request.run_id, request.run_name)
            .await
    }

    async fn stop_session(&self) -> Result<(), CoordinatorError> {
        self.handle.stop_run().await
    }

    async fn fail_session(&self, error: String) -> Result<(), CoordinatorError> {
        self.handle.fail_run(error).await
    }

    async fn update_title(
        &self,
        request: SessionTitleRequest,
    ) -> Result<RunInfo, CoordinatorError> {
        self.handle.update_session_title(request.title).await
    }

    async fn run_info(&self) -> Result<RunInfo, CoordinatorError> {
        self.handle.run_info().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_start_request_preserves_fields() {
        // arrange
        // act
        let request = SessionStartRequest {
            run_name: "test-run".to_string(),
            workspace_root: PathBuf::from("/workspace/project"),
        };

        // assert
        assert_eq!(request.run_name, "test-run");
        assert_eq!(request.workspace_root, PathBuf::from("/workspace/project"));
    }

    #[test]
    fn session_resume_request_preserves_fields() {
        // arrange
        // act
        let request = SessionResumeRequest {
            run_id: "run_000001".to_string(),
            run_name: "resumed-run".to_string(),
        };

        // assert
        assert_eq!(request.run_id, "run_000001");
        assert_eq!(request.run_name, "resumed-run");
    }

    #[test]
    fn session_title_request_preserves_title() {
        // arrange
        // act
        let request = SessionTitleRequest {
            title: "My Session Title".to_string(),
        };

        // assert
        assert_eq!(request.title, "My Session Title");
    }

    // Full integration tests that exercise CoordinatorSessionLeaf with a live
    // coordinator are in tests/leaf_contracts_test.rs. CoordinatorHandle
    // construction requires a live coordinator channel (tx is pub(in crate::coord)).
}
