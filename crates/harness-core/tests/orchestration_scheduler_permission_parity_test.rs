include!("common/coord_fixtures.rs");

mod parity_contracts {
    use super::*;
    use harness_core::perm::PermissionGrantScope;

    #[tokio::test]
    async fn permission_approval_precedes_execution_and_run_grant_is_remembered() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_tool_coordinator(
            temp.path(),
            Arc::new(test_mock_provider()),
            test_tool_registry(),
            ask_shell_permission_policy(),
            vec!["shell.run".to_string()],
            12,
        );
        let run = coordinator
            .start_run("parity_approval", temp.path())
            .await
            .unwrap_or_abort();
        let first = coordinator
            .request_tool_call(
                supervisor_actor(),
                Some("deep".to_string()),
                "shell.run",
                json!({"command": "cargo test -p harness-core"}),
            )
            .await
            .unwrap_or_abort();
        let pending = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::PermissionRequested(data)
                        if data.tool_call_id.as_ref().is_some_and(|id| id.as_str() == first)
                )
            })
        })
        .await;
        let permission_id = pending
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::PermissionRequested(data)
                    if data
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|id| id.as_str() == first) =>
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
                Some("remember this run".to_string()),
                Some(PermissionGrantScope::Run),
            )
            .await
            .unwrap_or_abort();
        common::wait_for_tool_call_finish(&run.events_path, &first).await;
        let second = coordinator
            .request_tool_call(
                supervisor_actor(),
                Some("deep".to_string()),
                "shell.run",
                json!({"command": "cargo test -p harness-tools"}),
            )
            .await
            .unwrap_or_abort();
        common::wait_for_tool_call_finish(&run.events_path, &second).await;
        coordinator.stop_run().await.unwrap_or_abort();

        let events = load_events(&run.events_path);
        let requested = events
            .iter()
            .filter(|event| matches!(&event.payload, EventV1::PermissionRequested(_)))
            .count();
        let first_finished = events.iter().find(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == first && data.status == ToolCallStatus::Succeeded
        ));
        let requested_seq = events.iter().find(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().is_some_and(|id| id.as_str() == first)
            )
        });
        let resolved_seq = events.iter().find(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data) if data.decision == EventPermissionDecision::Allow
            )
        });
        assert_eq!(requested, 1, "the run-scoped shell grant must be reused");
        assert!(requested_seq.unwrap_or_abort().seq < resolved_seq.unwrap_or_abort().seq);
        assert!(resolved_seq.unwrap_or_abort().seq < first_finished.unwrap_or_abort().seq);
    }

    #[tokio::test]
    async fn question_answer_routes_through_permission_resolution() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_coordinator(temp.path());
        let run = coordinator
            .start_run("parity_question", temp.path())
            .await
            .unwrap_or_abort();
        let answer_task = tokio::spawn({
            let coordinator = coordinator.clone();
            async move {
                coordinator
                    .request_question(
                        supervisor_actor(),
                        "question_parity",
                        json!({"questions": [{
                            "question": "Proceed?", "header": "Confirm",
                            "options": [{"label": "Yes", "description": "continue"}]
                        }]}),
                    )
                    .await
            }
        });
        let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events
                .iter()
                .any(|event| matches!(&event.payload, EventV1::PermissionRequested(_)))
        })
        .await;
        let permission_id = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::PermissionRequested(data) => Some(data.permission_id.clone()),
                _ => None,
            })
            .unwrap_or_abort();
        coordinator
            .resolve_permission(
                permission_id,
                RuntimePermissionDecision::Allow,
                Some("[[\"Yes\"]]".to_string()),
            )
            .await
            .unwrap_or_abort();
        assert_eq!(
            answer_task.await.unwrap_or_abort().unwrap_or_abort(),
            vec![vec!["Yes"]]
        );
        coordinator.stop_run().await.unwrap_or_abort();
    }

    #[tokio::test]
    async fn foreground_child_can_demote_then_cancel_with_owner_receipt_and_no_redelegation_bypass()
    {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let coordinator = test_agent_coordinator(temp.path(), Duration::from_millis(250));
        let run = coordinator
            .start_run("parity_background", temp.path())
            .await
            .unwrap_or_abort();
        let parent = coordinator
            .spawn_agent_idle(supervisor_actor(), "alpha", None)
            .await
            .unwrap_or_abort();
        let child = coordinator
            .spawn_agent_idle(supervisor_actor(), "beta", Some(parent.clone()))
            .await
            .unwrap_or_abort();
        let child_actor = EventActor::new(ActorKind::Worker, Some(child.clone()));
        let request_id = coordinator
            .request_child_agent_turn_with_model(
                supervisor_actor(),
                child.clone(),
                "long-running child work",
                None,
                None,
                ChildTaskRequestMetadata {
                    parent_tool_call_id: "task_parent_tool".to_string(),
                    parent_session_id: run.run_id.as_str().into(),
                    parent_agent_id: Some(parent),
                    child_session_id: child.clone().into(),
                    task_id: child.into(),
                    description: "child work".to_string(),
                    run_in_background: false,
                },
            )
            .await
            .unwrap_or_abort();
        let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
            events.iter().any(|event| matches!(
                &event.payload,
                EventV1::TaskScheduled(data) if event.correlation_id.as_deref() == Some(request_id.as_str())
            ))
        })
        .await;
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
        assert_eq!(
            coordinator
                .background_foreground_child_tasks()
                .await
                .unwrap_or_abort(),
            1
        );
        coordinator
            .cancel_task(task_id.clone(), "operator cancelled child")
            .await
            .unwrap_or_abort();
        assert!(coordinator
            .wait_background_request_terminal(request_id.clone(), &task_id, 500)
            .await
            .unwrap_or_abort());
        let bypass = coordinator
            .spawn_agent(
                EventActor::new(ActorKind::Worker, Some("worker_child".to_string())),
                "alpha",
                None,
            )
            .await;
        assert!(matches!(bypass, Err(CoordinatorError::PolicyViolation(_))));
        coordinator.stop_run().await.unwrap_or_abort();

        let events = load_events(&run.events_path);
        let receipt = events.iter().find(|event| matches!(
            &event.payload,
            EventV1::TaskCancelled(data) if data.task_id == task_id && data.reason == "operator cancelled child"
        ));
        assert_task_event_context(receipt.unwrap_or_abort(), &child_actor, &request_id);
        assert!(!events.iter().any(|event| matches!(
            &event.payload, EventV1::TaskCompleted(data) if data.task_id == task_id
        )));
    }

    mod recurring_and_team {
        use super::*;
        include!("orchestration_scheduler_permission_parity/recurring_and_team.rs");
    }
}
