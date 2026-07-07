use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn native_question_tool_rejects_or_times_out_cleanly() {
    let reject_workspace = setup_workspace_fixture();
    let reject_workspace_root = reject_workspace.workspace();
    let reject_coordinator =
        spawn_question_coordinator(reject_workspace.temp_dir().join("sessions"), 1_000);
    let reject_run = reject_coordinator
        .start_run("native_question_reject", reject_workspace_root)
        .await
        .unwrap_or_abort();

    let reject_task = spawn_question_tool_call(
        reject_coordinator.clone(),
        &reject_run.run_id,
        reject_workspace_root,
        &reject_run.artifacts_dir,
        "native-question-reject",
        json!({
            "questions": [{
                "question": "Pick one",
                "header": "Choice",
                "options": [{"label": "A", "description": "Option A"}]
            }]
        }),
    );

    let reject_permission_id = wait_for_question_permission(&reject_run.events_path).await;
    reject_coordinator
        .resolve_permission(reject_permission_id.clone(), PermissionDecision::Deny, None)
        .await
        .unwrap_or_abort();
    let reject_err = reject_task
        .await
        .unwrap_or_abort()
        .expect_err("denied question should fail");
    assert!(matches!(
        reject_err,
        ToolError::Execution(message) if message == "question rejected by user"
    ));
    assert!(read_events(&reject_run.events_path).iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.permission_id == reject_permission_id
                    && data.reason.is_none()
        )
    }));
    reject_coordinator
        .stop_run()
        .await
        .unwrap_or_abort();

    let timeout_workspace = setup_workspace_fixture();
    let timeout_workspace_root = timeout_workspace.workspace();
    let timeout_coordinator =
        spawn_question_coordinator(timeout_workspace.temp_dir().join("sessions"), 25);
    let timeout_run = timeout_coordinator
        .start_run("native_question_timeout", timeout_workspace_root)
        .await
        .unwrap_or_abort();

    let timeout_task = spawn_question_tool_call(
        timeout_coordinator.clone(),
        &timeout_run.run_id,
        timeout_workspace_root,
        &timeout_run.artifacts_dir,
        "native-question-timeout",
        json!({
            "questions": [{
                "question": "Pick one",
                "header": "Choice",
                "options": [{"label": "A", "description": "Option A"}]
            }]
        }),
    );

    let timeout_err = timeout(Duration::from_secs(2), timeout_task)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort()
        .expect_err("timed out question should fail");
    assert!(matches!(
        timeout_err,
        ToolError::Execution(message) if message == "question timed out awaiting user input"
    ));
    assert!(read_events(&timeout_run.events_path).iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.reason.as_deref() == Some("permission request timed out")
        )
    }));
    timeout_coordinator
        .stop_run()
        .await
        .unwrap_or_abort();
}
