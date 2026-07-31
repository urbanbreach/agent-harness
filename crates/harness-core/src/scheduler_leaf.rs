//! Scheduler leaf contract.
//!
//! Defines the narrow, typed API boundary through which scheduler leaf owners
//! (task scheduling, cancellation, background wait) interact with the
//! coordinator.
//!
//! # Authority preserved by the coordinator
//!
//! - **Scheduling**: the coordinator owns `Scheduler` and `RunState` task
//!   lifecycle. Leaf owners request turns/tool calls; the coordinator decides
//!   start vs. queue based on concurrency limits.
//! - **Cancellation**: the coordinator owns cancellation authority. Late task
//!   results become `TaskResultLate` and must not apply side effects after
//!   cancellation. Leaf owners observe cancellation outcomes, never force them.
//! - **Event append**: all `TaskScheduled`, `TaskCompleted`, `TaskCancelled`,
//!   and `TaskResultLate` events are appended by the coordinator.
//! - **Permission**: permission checks precede tool execution. The coordinator
//!   resolves permissions before a tool task starts; leaf owners never bypass.
//!
//! Leaf owners call into [`CoordinatorHandle`] through this trait; they do not
//! access `Scheduler`, `RunState`, `task_lifecycle`, or `event_helpers`
//! directly.

use async_trait::async_trait;

use crate::coord::{
    BackgroundWaitMode, BackgroundWaitOutcome, CoordinatorError, CoordinatorHandle,
};
use crate::event::EventActor;
use crate::perm::PermissionDecision;
use crate::perm::PermissionGrantScope;

/// Typed request to resolve a pending permission.
///
/// Leaf owners use this to route permission decisions through the coordinator,
/// which validates the permission id and appends `PermissionResolved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionResolveRequest {
    pub permission_id: String,
    pub decision: PermissionDecision,
    pub reason: Option<String>,
    pub grant_scope: Option<PermissionGrantScope>,
}

/// Typed request to cancel a task.
///
/// The coordinator owns cancellation authority. Late results arriving after
/// cancellation are recorded as `TaskResultLate` without side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCancelRequest {
    pub task_id: String,
    pub reason: String,
}

/// Typed request to wait for background request(s) to reach terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundWaitRequest {
    pub targets: Vec<(String, String)>,
    pub mode: BackgroundWaitMode,
    pub already_terminal: Vec<String>,
    pub timeout_ms: u64,
}

/// Scheduler leaf contract.
///
/// Implementors route all scheduling, cancellation, and permission resolution
/// operations through the coordinator handle. No method on this trait appends
/// events, mutates scheduler state, or bypasses permission checks.
///
/// # Cancellation boundary
///
/// When a task is cancelled, any late result arriving afterwards must be
/// ignored (recorded as `TaskResultLate` without side effects). Leaf owners
/// must not attempt to apply late results themselves.
///
/// # Permission boundary
///
/// Permission checks precede tool execution. The coordinator resolves
/// permissions before a tool task starts. Leaf owners must not execute tools
/// without coordinator-owned permission resolution.
///
/// # Integration requests
///
/// If a leaf owner needs a new coordinator-owned scheduling operation, it
/// must produce an integration request describing the exact hunk rather than
/// editing shared coord files.
#[async_trait]
pub trait SchedulerLeaf: Send + Sync {
    /// Resolve a pending permission through the coordinator.
    ///
    /// The coordinator validates the permission id, applies the decision,
    /// and appends `PermissionResolved`. If the decision is `Deny`, the
    /// associated tool call is rejected before execution.
    async fn resolve_permission(
        &self,
        request: PermissionResolveRequest,
    ) -> Result<(), CoordinatorError>;

    /// Cancel a task through the coordinator.
    ///
    /// The coordinator marks the task as cancelled, appends `TaskCancelled`,
    /// and ensures late results are recorded as `TaskResultLate` without
    /// side effects.
    async fn cancel_task(&self, request: TaskCancelRequest) -> Result<(), CoordinatorError>;

    /// Wait for background request(s) to reach terminal state.
    ///
    /// The coordinator subscribes to the event store and waits until the
    /// specified targets are terminal or the timeout expires. Leaf owners
    /// receive the typed outcome.
    async fn wait_background(
        &self,
        request: BackgroundWaitRequest,
    ) -> Result<BackgroundWaitOutcome, CoordinatorError>;

    /// Report job progress to the coordinator.
    ///
    /// This is a fire-and-forget notification that does not append events
    /// directly; the coordinator uses it for watchdog/stale detection.
    async fn report_progress(
        &self,
        task_id: String,
        kind: crate::coord::JobProgressKind,
    ) -> Result<(), CoordinatorError>;
}

/// Coordinator-backed implementation of [`SchedulerLeaf`].
///
/// This is the canonical adapter that routes scheduling operations through
/// the coordinator handle. Leaf owners should depend on the trait, not this
/// struct, so test doubles can substitute.
#[derive(Debug, Clone)]
pub struct CoordinatorSchedulerLeaf {
    handle: CoordinatorHandle,
}

impl CoordinatorSchedulerLeaf {
    pub fn new(handle: CoordinatorHandle) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl SchedulerLeaf for CoordinatorSchedulerLeaf {
    async fn resolve_permission(
        &self,
        request: PermissionResolveRequest,
    ) -> Result<(), CoordinatorError> {
        self.handle
            .resolve_permission_with_grant_scope(
                request.permission_id,
                request.decision,
                request.reason,
                request.grant_scope,
            )
            .await
    }

    async fn cancel_task(&self, request: TaskCancelRequest) -> Result<(), CoordinatorError> {
        self.handle
            .cancel_task(request.task_id, request.reason)
            .await
    }

    async fn wait_background(
        &self,
        request: BackgroundWaitRequest,
    ) -> Result<BackgroundWaitOutcome, CoordinatorError> {
        self.handle
            .wait_background_requests_terminal(
                &request.targets,
                request.mode,
                &request.already_terminal,
                request.timeout_ms,
            )
            .await
    }

    async fn report_progress(
        &self,
        task_id: String,
        kind: crate::coord::JobProgressKind,
    ) -> Result<(), CoordinatorError> {
        self.handle.job_progress(task_id, kind).await
    }
}

/// Marker for the permission-before-tool invariant.
///
/// This type exists to make the invariant visible in the type system: a tool
/// call request must carry a [`PermissionCheckpoint`] proving that permission
/// was resolved before execution. The coordinator enforces this at runtime;
/// this type makes the contract explicit at the API boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionCheckpoint {
    pub permission_id: String,
    pub decision: PermissionDecision,
}

impl PermissionCheckpoint {
    pub fn allowed(permission_id: impl Into<String>) -> Self {
        Self {
            permission_id: permission_id.into(),
            decision: PermissionDecision::Allow,
        }
    }

    pub fn denied(permission_id: impl Into<String>) -> Self {
        Self {
            permission_id: permission_id.into(),
            decision: PermissionDecision::Deny,
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.decision == PermissionDecision::Allow
    }
}

/// Marker for the cancellation boundary invariant.
///
/// When a task is cancelled, late results must be ignored. This type makes
/// the cancellation state explicit at the API boundary so leaf owners can
/// check whether a result is still valid before attempting to use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancellationBoundary {
    /// The task is still active; results are valid.
    Active,
    /// The task was cancelled; late results must be ignored.
    Cancelled { reason: String },
}

impl CancellationBoundary {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Returns `true` if a late result should be ignored.
    ///
    /// After cancellation, any result arriving for this task is late and
    /// must not apply side effects. The coordinator records it as
    /// `TaskResultLate`; leaf owners must drop it.
    pub fn late_result_ignored(&self) -> bool {
        self.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_resolve_request_preserves_fields() {
        // arrange
        // act
        let request = PermissionResolveRequest {
            permission_id: "perm_001".to_string(),
            decision: PermissionDecision::Allow,
            reason: Some("run-scoped".to_string()),
            grant_scope: Some(PermissionGrantScope::Run),
        };

        // assert
        assert_eq!(request.permission_id, "perm_001");
        assert_eq!(request.decision, PermissionDecision::Allow);
    }

    #[test]
    fn task_cancel_request_preserves_fields() {
        // arrange
        // act
        let request = TaskCancelRequest {
            task_id: "task_000001".to_string(),
            reason: "user cancelled".to_string(),
        };

        // assert
        assert_eq!(request.task_id, "task_000001");
        assert_eq!(request.reason, "user cancelled");
    }

    #[test]
    fn permission_checkpoint_allowed_is_allowed() {
        // arrange
        // act
        let checkpoint = PermissionCheckpoint::allowed("perm_001");

        // assert
        assert!(checkpoint.is_allowed());
        assert_eq!(checkpoint.decision, PermissionDecision::Allow);
    }

    #[test]
    fn permission_checkpoint_denied_is_not_allowed() {
        // arrange
        // act
        let checkpoint = PermissionCheckpoint::denied("perm_001");

        // assert
        assert!(!checkpoint.is_allowed());
        assert_eq!(checkpoint.decision, PermissionDecision::Deny);
    }

    #[test]
    fn cancellation_boundary_active_is_not_cancelled() {
        // arrange
        // act
        let boundary = CancellationBoundary::Active;

        // assert
        assert!(!boundary.is_cancelled());
        assert!(!boundary.late_result_ignored());
    }

    #[test]
    fn cancellation_boundary_cancelled_ignores_late_result() {
        // arrange
        // act
        let boundary = CancellationBoundary::Cancelled {
            reason: "timeout".to_string(),
        };

        // assert
        assert!(boundary.is_cancelled());
        assert!(boundary.late_result_ignored());
    }

    // Full integration tests that exercise CoordinatorSchedulerLeaf with a
    // live coordinator are in tests/leaf_contracts_test.rs. CoordinatorHandle
    // construction requires a live coordinator channel (tx is pub(in crate::coord)).
}
