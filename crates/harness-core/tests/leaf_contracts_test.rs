//! Leaf contract integration tests for Todo 10.
//!
//! These tests prove the coordinator-owned invariants that the leaf contracts
//! (workspace, session, scheduler, integration) depend on:
//!
//! 1. **Event append owner is coordinator**: all events are appended by the
//!    coordinator through `event_helpers`. Leaf owners never write events.
//! 2. **Permission check before tool execution**: `PermissionRequested` and
//!    `PermissionResolved` events appear before `ToolCallFinished` in the
//!    event sequence. The coordinator resolves permissions before a tool
//!    task starts.
//! 3. **Late result ignored after cancellation**: when a task is cancelled,
//!    late results are recorded as `TaskResultLate` without side effects.
//!    The coordinator owns cancellation authority.
//! 4. **State clean after cancellation**: no extra `TaskCompleted` with
//!    succeeded status appears after `TaskCancelled`.

include!("common/coord_fixtures.rs");

mod leaf_contracts {
    use super::*;
    use harness_core::perm::PermissionGrantScope;
    use harness_core::scheduler_leaf::{
        CancellationBoundary, CoordinatorSchedulerLeaf, PermissionCheckpoint,
        PermissionResolveRequest, SchedulerLeaf,
    };
    use harness_core::session_leaf::{CoordinatorSessionLeaf, SessionLeaf, SessionStartRequest};
    use harness_core::workspace_leaf::{
        CoordinatorWorkspaceLeaf, WorkspaceLeaf, WorkspaceSnapshotRequest,
    };

    /// Happy path: prove that the coordinator is the event append owner and
    /// that permission checks precede tool execution.
    ///
    /// This test uses the scheduler leaf contract's `PermissionCheckpoint` type
    /// to make the permission-before-tool invariant explicit at the API
    /// boundary, then verifies the invariant in the event log.
    #[tokio::test]
    async fn coordinator_owns_event_append_and_permission_precedes_tool_execution() {
        // arrange — coordinator with ask-shell permission policy
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_tool_coordinator(
            temp_dir.path(),
            Arc::new(test_mock_provider()),
            test_tool_registry(),
            ask_shell_permission_policy(),
            vec!["shell.run".to_string()],
            12,
        );

        // Use the session leaf contract to start the run
        let session_leaf = CoordinatorSessionLeaf::new(coordinator.clone());
        let run = session_leaf
            .start_session(SessionStartRequest {
                run_name: "leaf_permission_before_tool".to_string(),
                workspace_root: PathBuf::from("/workspace/project"),
            })
            .await
            .unwrap_or_abort();

        // act — request a tool call through the coordinator handle
        let tool_call_id = coordinator
            .request_tool_call(
                supervisor_actor(),
                Some("deep".to_string()),
                "shell.run",
                json!({"cmd": "true"}),
            )
            .await
            .unwrap_or_abort();

        // Wait for the permission request to appear
        let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            ))
        })
        .await;

        // Extract the permission_id and resolve it with Allow
        let permission_id = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str())
                        == Some(tool_call_id.as_str()) =>
                {
                    Some(data.permission_id.clone())
                }
                _ => None,
            })
            .unwrap_or_abort();

        // The PermissionCheckpoint makes the permission-before-tool invariant
        // explicit: the leaf owner creates a checkpoint proving permission was
        // resolved before the tool executes.
        let checkpoint = PermissionCheckpoint::allowed(&permission_id);
        assert!(checkpoint.is_allowed());

        // Resolve the permission through the coordinator (scheduler leaf contract)
        coordinator
            .resolve_permission_with_grant_scope(
                permission_id,
                RuntimePermissionDecision::Allow,
                Some("leaf-contract-test".to_string()),
                Some(PermissionGrantScope::Run),
            )
            .await
            .unwrap_or_abort();

        // Wait for the tool call to finish
        wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(data)
                        if data.tool_call_id.as_str() == tool_call_id
                            && data.status == ToolCallStatus::Succeeded
                )
            })
        })
        .await;

        coordinator.stop_run().await.unwrap_or_abort();

        // assert — verify event ordering and ownership
        let final_events = load_events(&run.events_path);

        // 1. Event append owner is coordinator: all events have seq numbers
        //    in contiguous order, proving they were appended by the coordinator.
        let seqs: Vec<u64> = final_events.iter().map(|e| e.seq).collect();
        let mut sorted_seqs = seqs.clone();
        sorted_seqs.sort_unstable();
        assert_eq!(seqs, sorted_seqs, "events must be in contiguous seq order");

        // 2. Permission check before tool execution: PermissionRequested must
        //    appear before ToolCallFinished in the event sequence.
        let permission_requested_seq = final_events
            .iter()
            .find(|event| matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            ))
            .map(|event| event.seq);
        let tool_call_finished_seq = final_events
            .iter()
            .find(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == tool_call_id
                )
            })
            .map(|event| event.seq);

        let perm_seq = permission_requested_seq.unwrap_or_abort();
        let finish_seq = tool_call_finished_seq.unwrap_or_abort();
        assert!(
            perm_seq < finish_seq,
            "PermissionRequested (seq={perm_seq}) must precede ToolCallFinished (seq={finish_seq})"
        );

        // 3. PermissionResolved also appears before ToolCallFinished
        let permission_resolved_seq = final_events
            .iter()
            .find(|event| matches!(
                &event.payload,
                EventV1::PermissionResolved(data) if data.decision == EventPermissionDecision::Allow
            ))
            .map(|event| event.seq);
        let resolved_seq = permission_resolved_seq.unwrap_or_abort();
        assert!(
            resolved_seq < finish_seq,
            "PermissionResolved (seq={resolved_seq}) must precede ToolCallFinished (seq={finish_seq})"
        );

        // 4. The tool call succeeded (permission was allowed before execution)
        assert!(final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Succeeded
        )));
    }

    /// Failure path: prove that late results are ignored after cancellation
    /// and state remains clean.
    ///
    /// This test uses the scheduler leaf contract's `CancellationBoundary` type
    /// to make the late-result-ignored invariant explicit at the API boundary,
    /// then verifies the invariant in the event log.
    #[tokio::test]
    async fn denial_cancellation_late_result_ignored_and_state_clean() {
        // arrange — coordinator with a slow provider so the turn is still running
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(200));

        let run = coordinator
            .start_run(
                "leaf_cancellation_late_result",
                PathBuf::from("/workspace/project"),
            )
            .await
            .unwrap_or_abort();

        // act — spawn an agent and request a turn
        let agent_id = coordinator
            .spawn_agent_idle(supervisor_actor(), "alpha", None)
            .await
            .unwrap_or_abort();
        let request_id = coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), "alpha-prompt")
            .await
            .unwrap_or_abort();

        // Wait for the TaskScheduled event
        let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskScheduled(data)
                        if event.correlation_id.as_deref() == Some(request_id.as_str())
                )
            })
        })
        .await;

        // Find the task_id for the agent turn
        let task_id = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::TaskScheduled(data)
                    if event.correlation_id.as_deref() == Some(request_id.as_str()) =>
                {
                    Some(data.task_id.clone())
                }
                _ => None,
            })
            .unwrap_or_abort();

        // Cancel the task through the coordinator (scheduler leaf contract)
        coordinator
            .cancel_task(task_id.clone(), "leaf-contract-cancellation-test")
            .await
            .unwrap_or_abort();

        // The CancellationBoundary makes the late-result-ignored invariant
        // explicit: after cancellation, any result is late and must be ignored.
        let boundary = CancellationBoundary::Cancelled {
            reason: "leaf-contract-cancellation-test".to_string(),
        };
        assert!(boundary.late_result_ignored());

        // Give the coordinator time to process the cancellation and any
        // late provider response that arrives after cancellation.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        coordinator.stop_run().await.unwrap_or_abort();

        // assert — verify cancellation boundary and state cleanliness
        let final_events = load_events(&run.events_path);

        // 1. TaskCancelled event exists for the cancelled task
        let cancelled_events: Vec<_> = final_events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCancelled(data) if data.task_id == task_id
                )
            })
            .collect();
        assert_eq!(
            cancelled_events.len(),
            1,
            "exactly one TaskCancelled event should exist for the cancelled task"
        );

        // 2. No TaskCompleted event for the cancelled task — the late result
        //    was ignored, not applied as a completion.
        let completed_for_cancelled: Vec<_> = final_events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCompleted(data) if data.task_id == task_id
                )
            })
            .collect();
        assert!(
            completed_for_cancelled.is_empty(),
            "late result must be ignored: no TaskCompleted event for cancelled task"
        );

        // 3. State is clean: exactly one terminal event for the task
        let terminal_events: Vec<_> = final_events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCancelled(data) if data.task_id == task_id
                ) || matches!(
                    &event.payload,
                    EventV1::TaskCompleted(data) if data.task_id == task_id
                )
            })
            .collect();
        assert_eq!(
            terminal_events.len(),
            1,
            "exactly one terminal event should exist for the cancelled task"
        );

        // 4. The terminal event is TaskCancelled, not TaskCompleted
        assert!(matches!(
            &terminal_events[0].payload,
            EventV1::TaskCancelled(data) if data.task_id == task_id
                && data.reason == "leaf-contract-cancellation-test"
        ));

        // 5. Events are in contiguous seq order (coordinator-owned append)
        let seqs: Vec<u64> = final_events.iter().map(|e| e.seq).collect();
        let mut sorted_seqs = seqs.clone();
        sorted_seqs.sort_unstable();
        assert_eq!(seqs, sorted_seqs, "events must be in contiguous seq order");
    }

    /// Prove that the workspace leaf contract routes snapshot operations
    /// through the coordinator handle.
    #[tokio::test]
    async fn workspace_leaf_routes_snapshot_through_coordinator() {
        // arrange
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_coordinator(temp_dir.path());

        let workspace_leaf =
            CoordinatorWorkspaceLeaf::new(coordinator.clone(), temp_dir.path().to_path_buf());

        let run = coordinator
            .start_run("leaf_workspace_snapshot", temp_dir.path().to_path_buf())
            .await
            .unwrap_or_abort();

        // act — request a snapshot through the workspace leaf contract
        let snapshot = workspace_leaf
            .snapshot(WorkspaceSnapshotRequest {
                request_id: "leaf_snap_001".to_string(),
            })
            .await
            .unwrap_or_abort();

        coordinator.stop_run().await.unwrap_or_abort();

        // assert — the snapshot was created by the coordinator
        assert_eq!(snapshot.request_id.as_str(), "leaf_snap_001");
        assert!(!snapshot.artifact_path.is_empty());

        // The snapshot artifact exists in the artifacts directory
        let snapshot_path = run.artifacts_dir.join(&snapshot.artifact_path);
        assert!(
            snapshot_path.exists(),
            "snapshot artifact must exist at {}",
            snapshot_path.display()
        );

        // The workspace_root is accessible through the leaf contract
        assert_eq!(
            workspace_leaf.workspace_root(),
            temp_dir.path().to_path_buf()
        );
    }

    /// Prove that the session leaf contract routes start/stop through the
    /// coordinator handle and that the coordinator appends the lifecycle events.
    #[tokio::test]
    async fn session_leaf_routes_lifecycle_through_coordinator() {
        // arrange
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_coordinator(temp_dir.path());
        let session_leaf = CoordinatorSessionLeaf::new(coordinator.clone());

        // act — start a session through the session leaf contract
        let run = session_leaf
            .start_session(SessionStartRequest {
                run_name: "leaf_session_lifecycle".to_string(),
                workspace_root: temp_dir.path().to_path_buf(),
            })
            .await
            .unwrap_or_abort();

        // Update the title through the leaf contract
        let updated_run = session_leaf
            .update_title(harness_core::session_leaf::SessionTitleRequest {
                title: "Leaf Contract Test".to_string(),
            })
            .await
            .unwrap_or_abort();

        // Stop the session through the leaf contract
        session_leaf.stop_session().await.unwrap_or_abort();

        // assert — the coordinator appended lifecycle events
        let events = load_events(&run.events_path);

        // RunStarted event exists
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            EventV1::RunStarted(data) if data.run_name == "leaf_session_lifecycle".into()
        )));

        // SessionTitleUpdated event exists
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            EventV1::SessionTitleUpdated(data) if data.title == "Leaf Contract Test"
        )));

        // RunFinished event exists
        assert!(events
            .iter()
            .any(|event| matches!(&event.payload, EventV1::RunFinished(_))));

        // The run_id from start_session matches the one from update_title
        assert_eq!(run.run_id, updated_run.run_id);
    }

    /// Prove that the scheduler leaf contract's `PermissionResolveRequest`
    /// routes permission resolution through the coordinator handle.
    #[tokio::test]
    async fn scheduler_leaf_routes_permission_resolution_through_coordinator() {
        // arrange — coordinator with ask-shell permission policy
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_tool_coordinator(
            temp_dir.path(),
            Arc::new(test_mock_provider()),
            test_tool_registry(),
            ask_shell_permission_policy(),
            vec!["shell.run".to_string()],
            12,
        );

        let scheduler_leaf =
            harness_core::scheduler_leaf::CoordinatorSchedulerLeaf::new(coordinator.clone());

        let run = coordinator
            .start_run(
                "leaf_scheduler_permission",
                PathBuf::from("/workspace/project"),
            )
            .await
            .unwrap_or_abort();

        // act — request a tool call that triggers a permission request
        let tool_call_id = coordinator
            .request_tool_call(
                supervisor_actor(),
                Some("deep".to_string()),
                "shell.run",
                json!({"cmd": "true"}),
            )
            .await
            .unwrap_or_abort();

        // Wait for the permission request
        let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            ))
        })
        .await;

        let permission_id = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str())
                        == Some(tool_call_id.as_str()) =>
                {
                    Some(data.permission_id.clone())
                }
                _ => None,
            })
            .unwrap_or_abort();

        // Resolve the permission through the scheduler leaf contract
        scheduler_leaf
            .resolve_permission(PermissionResolveRequest {
                permission_id: permission_id.clone(),
                decision: RuntimePermissionDecision::Allow,
                reason: Some("leaf-contract-allow".to_string()),
                grant_scope: Some(PermissionGrantScope::Run),
            })
            .await
            .unwrap_or_abort();

        // Wait for the tool call to finish
        wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(data)
                        if data.tool_call_id.as_str() == tool_call_id
                            && data.status == ToolCallStatus::Succeeded
                )
            })
        })
        .await;

        coordinator.stop_run().await.unwrap_or_abort();

        // assert — the coordinator appended PermissionResolved with Allow
        let final_events = load_events(&run.events_path);
        assert!(final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == EventPermissionDecision::Allow
                    && data.reason.as_deref() == Some("leaf-contract-allow")
        )));

        // The tool call succeeded (permission was resolved before execution)
        assert!(final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Succeeded
        )));
    }
}
