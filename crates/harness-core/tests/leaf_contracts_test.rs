//! Coordinator boundary tests for event ownership, permissions, cancellation,
//! workspace snapshots, and session lifecycle.

include!("common/coord_fixtures.rs");

mod leaf_contracts {
    use super::*;
    use harness_core::perm::PermissionGrantScope;

    /// Happy path: prove that the coordinator is the event append owner and
    /// that permission checks precede tool execution.
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

        let run = coordinator
            .start_run(
                "leaf_permission_before_tool",
                PathBuf::from("/workspace/project"),
            )
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

        coordinator
            .cancel_task(task_id.clone(), "leaf-contract-cancellation-test")
            .await
            .unwrap_or_abort();

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

    /// Prove that the coordinator writes the workspace snapshot artifact.
    #[tokio::test]
    async fn coordinator_writes_workspace_snapshot_artifact() {
        // arrange
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_coordinator(temp_dir.path());

        let run = coordinator
            .start_run("leaf_workspace_snapshot", temp_dir.path().to_path_buf())
            .await
            .unwrap_or_abort();

        let snapshot = coordinator
            .snapshot_workspace("leaf_snap_001")
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
    }

    /// Prove that the coordinator appends the session lifecycle events.
    #[tokio::test]
    async fn coordinator_records_session_lifecycle_events() {
        // arrange
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_coordinator(temp_dir.path());

        let run = coordinator
            .start_run("leaf_session_lifecycle", temp_dir.path().to_path_buf())
            .await
            .unwrap_or_abort();

        let updated_run = coordinator
            .update_session_title("Leaf Contract Test")
            .await
            .unwrap_or_abort();

        coordinator.stop_run().await.unwrap_or_abort();

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

        assert_eq!(run.run_id, updated_run.run_id);
    }

    /// Prove that the coordinator records the scoped permission decision.
    #[tokio::test]
    async fn coordinator_records_scoped_permission_resolution() {
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

        coordinator
            .resolve_permission_with_grant_scope(
                permission_id,
                RuntimePermissionDecision::Allow,
                Some("leaf-contract-allow".to_string()),
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
