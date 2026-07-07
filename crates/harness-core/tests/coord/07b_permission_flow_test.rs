use harness_core::UnwrapOrAbort;
use harness_core::perm::PermissionGrantScope;

#[tokio::test]
async fn permission_flow_covers_allow_headless_ask_deny_and_worker_policy_violation() {
    // arrange
    let allow_temp_dir = tempfile::tempdir().unwrap_or_abort();
    let allow_coordinator = test_agent_tool_coordinator(
        allow_temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        shell_only_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );
    let allow_run = allow_coordinator
        .start_run("coord_permission_allow", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();

    // act
    let allowed_tool_call_id = allow_coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();
    wait_for_events(&allow_run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data) if data.tool_call_id == allowed_tool_call_id
        ))
    })
    .await;
    allow_coordinator.stop_run().await.unwrap_or_abort();

    // assert
    let allow_events = load_events(&allow_run.events_path);
    assert!(allow_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::ToolCallFinished(data)
            if data.tool_call_id == allowed_tool_call_id && data.status == ToolCallStatus::Succeeded
    )));

    // arrange
    let ask_temp_dir = tempfile::tempdir().unwrap_or_abort();
    let ask_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1);
    let ask_coordinator = test_agent_tool_coordinator(
        ask_temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        ask_policy,
        vec!["shell.run".to_string()],
        12,
    );
    let ask_run = ask_coordinator
        .start_run(
            "coord_permission_headless_ask_deny",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    // act
    let ask_tool_call_id = ask_coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();
    let ask_events = wait_for_events(&ask_run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == EventPermissionDecision::Deny
                    && data.reason.as_deref() == Some("permission request timed out")
        ))
    })
    .await;
    ask_coordinator.stop_run().await.unwrap_or_abort();

    // assert
    assert!(ask_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PermissionRequested(data)
            if data.tool_call_id.as_deref() == Some(ask_tool_call_id.as_str())
                && data.default_decision == EventPermissionDecision::Deny
                && data.summary.contains("tool=shell.run")
                && data.summary.contains("true")
    )));
    assert!(ask_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::ToolCallFinished(data)
            if data.tool_call_id == ask_tool_call_id && data.status == ToolCallStatus::Failed
    )));

    // arrange
    let worker_temp_dir = tempfile::tempdir().unwrap_or_abort();
    let worker_coordinator = test_agent_tool_coordinator(
        worker_temp_dir.path(),
        Arc::new(test_mock_provider()),
        test_tool_registry(),
        allow_all_permission_policy(),
        Vec::new(),
        12,
    );
    let worker_run = worker_coordinator
        .start_run(
            "coord_permission_worker_policy_violation",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    // act
    let worker_agent_id = worker_coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let worker_error = worker_coordinator
        .request_tool_call(
            EventActor::new(ActorKind::Worker, Some(worker_agent_id)),
            Some("spoof-allow".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect_err("worker tool outside toolset must be denied");
    worker_coordinator
        .stop_run()
        .await
        .unwrap_or_abort();

    // assert
    assert!(matches!(worker_error, CoordinatorError::PolicyViolation(_)));
    let worker_events = load_events(&worker_run.events_path);
    assert!(worker_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PolicyViolationDetected(data)
            if data.policy == "tool_not_in_toolset" && data.detail.contains("shell.run")
    )));
}

#[tokio::test]
async fn repeated_shell_command_after_run_grant_uses_prefix_pattern() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(CountingShellTool {
        calls: Arc::clone(&calls),
    }));
    let tool_registry = Arc::new(registry);

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_agent_tool_coordinator(
        temp_dir.path(),
        Arc::new(test_mock_provider()),
        tool_registry,
        ask_shell_permission_policy(),
        vec!["shell.run".to_string()],
        12,
    );
    let run = coordinator
        .start_run(
            "coord_permission_shell_reuse",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let first_tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"command": "cargo test -p harness-core"}),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| matches!(
            &event.payload,
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(first_tool_call_id.as_str())
        ))
    })
    .await;
    let permission_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(first_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert!(
        events.iter().any(|event| matches!(
            &event.payload,
            EventV1::PermissionRequested(data) if data.summary.contains("cargo test *")
        )),
        "permission summary should advertise the reusable prefix pattern"
    );

    coordinator
        .resolve_permission_with_grant_scope(
            permission_id,
            RuntimePermissionDecision::Allow,
            Some("allow run-scoped".to_string()),
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();

    common::wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let second_tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"command": "cargo test -p harness-tools --test native_tool_parity_matrix_test"}),
        )
        .await
        .unwrap_or_abort();

    common::wait_for_tool_call_finish(&run.events_path, &second_tool_call_id).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let final_events = load_events(&run.events_path);
    let requested = final_events
        .iter()
        .filter(|event| matches!(&event.payload, EventV1::PermissionRequested(_)))
        .count();
    let resolved = final_events
        .iter()
        .filter(|event| matches!(&event.payload, EventV1::PermissionResolved(_)))
        .count();
    let succeeded = final_events
        .iter()
        .filter(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data) if data.status == ToolCallStatus::Succeeded
        ))
        .count();

    assert_eq!(requested, 1, "only the first shell command should prompt");
    assert_eq!(resolved, 1, "only one permission resolution should be recorded");
    assert_eq!(succeeded, 2, "both shell calls should succeed");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "shell tool should be invoked twice"
    );
}
