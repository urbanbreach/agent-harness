//! Workspace leaf contract.
//!
//! Defines the narrow, typed API boundary through which workspace leaf owners
//! (snapshot, revert, environment discovery) interact with the coordinator.
//!
//! # Authority preserved by the coordinator
//!
//! - **Event append**: all workspace snapshot/revert events are appended by the
//!   coordinator through `event_helpers`. Leaf owners never write events.
//! - **Permission**: workspace revert requires coordinator-owned permission
//!   resolution before side effects are applied.
//! - **Lifecycle**: snapshot/revert lifecycle state is managed by `RunState`
//!   inside the coordinator. Leaf owners observe results, never mutate state.
//!
//! Leaf owners call into [`CoordinatorHandle`] through this trait; they do not
//! access `RunState`, `event_helpers`, or the event store directly.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::coord::{
    CoordinatorError, CoordinatorHandle, WorkspaceRevertSummary, WorkspaceSnapshotSummary,
};

/// Typed request for a workspace snapshot.
///
/// The `request_id` is caller-supplied so the leaf owner can correlate the
/// snapshot with its own tracking without touching coordinator internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshotRequest {
    pub request_id: String,
}

/// Typed request for a workspace revert.
///
/// The `snapshot_request_id` must reference a prior snapshot accepted by the
/// coordinator. The coordinator validates ownership and ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRevertRequest {
    pub snapshot_request_id: String,
}

/// Workspace leaf contract.
///
/// Implementors route all workspace operations through the coordinator handle.
/// No method on this trait appends events, resolves permissions, or mutates
/// coordinator lifecycle state directly.
///
/// # Integration requests
///
/// If a leaf owner needs a new coordinator-owned workspace operation (e.g. a
/// new snapshot format), it must produce an integration request describing the
/// exact hunk rather than editing shared coord files.
#[async_trait]
pub trait WorkspaceLeaf: Send + Sync {
    /// Request a workspace snapshot through the coordinator.
    ///
    /// The coordinator appends the snapshot event and writes the artifact.
    /// Leaf owners receive the typed summary only.
    async fn snapshot(
        &self,
        request: WorkspaceSnapshotRequest,
    ) -> Result<WorkspaceSnapshotSummary, CoordinatorError>;

    /// Request a workspace revert through the coordinator.
    ///
    /// The coordinator validates the snapshot reference, applies the revert,
    /// and appends the revert event. Leaf owners receive the typed summary.
    async fn revert(
        &self,
        request: WorkspaceRevertRequest,
    ) -> Result<WorkspaceRevertSummary, CoordinatorError>;

    /// Discover the workspace environment for display/tracking.
    ///
    /// This is a read-only operation that does not touch coordinator state.
    fn workspace_root(&self) -> PathBuf;
}

/// Coordinator-backed implementation of [`WorkspaceLeaf`].
///
/// This is the canonical adapter that routes workspace operations through the
/// coordinator handle. Leaf owners should depend on the trait, not this struct,
/// so test doubles can substitute.
#[derive(Debug, Clone)]
pub struct CoordinatorWorkspaceLeaf {
    handle: CoordinatorHandle,
    workspace_root: PathBuf,
}

impl CoordinatorWorkspaceLeaf {
    pub fn new(handle: CoordinatorHandle, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            handle,
            workspace_root: workspace_root.into(),
        }
    }
}

#[async_trait]
impl WorkspaceLeaf for CoordinatorWorkspaceLeaf {
    async fn snapshot(
        &self,
        request: WorkspaceSnapshotRequest,
    ) -> Result<WorkspaceSnapshotSummary, CoordinatorError> {
        self.handle.snapshot_workspace(request.request_id).await
    }

    async fn revert(
        &self,
        request: WorkspaceRevertRequest,
    ) -> Result<WorkspaceRevertSummary, CoordinatorError> {
        self.handle
            .revert_workspace(request.snapshot_request_id)
            .await
    }

    fn workspace_root(&self) -> PathBuf {
        self.workspace_root.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_snapshot_request_preserves_caller_supplied_id() {
        // arrange
        // act
        let request = WorkspaceSnapshotRequest {
            request_id: "snap_001".to_string(),
        };

        // assert
        assert_eq!(request.request_id, "snap_001");
    }

    #[test]
    fn workspace_revert_request_references_snapshot_id() {
        // arrange
        // act
        let request = WorkspaceRevertRequest {
            snapshot_request_id: "snap_001".to_string(),
        };

        // assert
        assert_eq!(request.snapshot_request_id, "snap_001");
    }

    // Full integration tests that exercise CoordinatorWorkspaceLeaf with a
    // live coordinator are in tests/leaf_contracts_test.rs. The inline tests
    // here cover only the pure request/response types, since CoordinatorHandle
    // construction requires a live coordinator channel.
}
