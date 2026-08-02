use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn mid_flight_second_tool_denied_by_explicit_resolve() {
    // arrange
    // Given: Ask mode for shell; two sequential tool calls in one run.
    // When: first call is operator-allowed (once); second is operator-denied via resolve_permission.
    // Then: first Succeeded; second Failed with PermissionRequested + PermissionResolved Deny;
    //       denied tool never starts (distinct from static pre-start deny and headless timeout).
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
            "coord_permission_mid_flight_deny_stress",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    // act
    let first_tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str())
                        == Some(first_tool_call_id.as_str())
            )
        })
    })
    .await;
    let first_permission_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(first_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .resolve_permission(
            first_permission_id,
            RuntimePermissionDecision::Allow,
            Some("allow first mid-flight".to_string()),
        )
        .await
        .unwrap_or_abort();
    common::wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let second_tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo denied"}),
        )
        .await
        .unwrap_or_abort();

    let events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str())
                        == Some(second_tool_call_id.as_str())
            )
        })
    })
    .await;
    let second_permission_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(second_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .resolve_permission(
            second_permission_id.clone(),
            RuntimePermissionDecision::Deny,
            Some("operator deny mid-flight".to_string()),
        )
        .await
        .unwrap_or_abort();

    let final_events = wait_for_events(&run.events_path, Duration::from_millis(500), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.permission_id == second_permission_id
                        && data.decision == EventPermissionDecision::Deny
            )
        }) && events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id.as_str() == second_tool_call_id
                        && data.status == ToolCallStatus::Failed
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    // assert
    assert!(
        final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == first_tool_call_id
                    && data.status == ToolCallStatus::Succeeded
        )),
        "first mid-flight tool must succeed after explicit allow"
    );
    assert!(
        final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(second_tool_call_id.as_str())
        )),
        "second tool must emit PermissionRequested before deny"
    );
    assert!(
        final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.permission_id == second_permission_id
                    && data.decision == EventPermissionDecision::Deny
                    && data.reason.as_deref() == Some("operator deny mid-flight")
        )),
        "operator deny must emit PermissionResolved Deny"
    );
    assert!(
        final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == second_tool_call_id
                    && data.status == ToolCallStatus::Failed
        )),
        "denied tool must finish Failed"
    );
    assert!(
        !final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == second_tool_call_id
        )),
        "denied tool must not start execution"
    );
    assert!(
        !final_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == second_tool_call_id
                    && data.status == ToolCallStatus::Succeeded
        )),
        "denied tool must not succeed"
    );
}
