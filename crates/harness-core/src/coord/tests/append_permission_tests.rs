use super::*;

#[test]
fn event_append_helpers_preserve_correlation_fallbacks_and_stream_keys() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator = Coordinator::new(
        test_config(temp_dir.path()),
        clock.clone(),
        redactor.clone(),
        command_rx,
        job_tx,
        job_rx,
    );
    coordinator
        .start_run_internal("append_helpers".to_string(), temp_dir.path().to_path_buf())
        .expect("start run");

    let actor = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let run_state = coordinator.run_state.as_mut().expect("run state");

    let requested = append_tool_call_requested_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        ToolCallRequestedEventArgs {
            actor,
            tool_call_id: "toolcall_000001",
            tool_id: "shell.run",
            args_json: &json!({"cmd": "true"}),
            tool_metadata: None,
            request_correlation_id: Some("req_000001"),
        },
    )
    .expect("append tool call requested");
    assert_eq!(requested.correlation_id.as_deref(), Some("req_000001"));
    assert_eq!(
        requested.stream_key.as_deref(),
        Some("tool_call:toolcall_000001")
    );

    let permission = append_permission_requested_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        PermissionRequestedEventArgs {
            permission_id: "perm_000001",
            tool_call_id: "toolcall_000001",
            kind: PermissionKind::Shell,
            summary: "run shell command".to_string(),
            request_digest: "digest-perm".to_string(),
            timeout_ms: 0,
            default_decision: crate::event::PermissionDecision::Deny,
            request_correlation_id: None,
        },
    )
    .expect("append permission requested");
    assert_eq!(
        permission.correlation_id.as_deref(),
        Some("toolcall_000001")
    );
    assert_eq!(
        permission.stream_key.as_deref(),
        Some("permission:perm_000001")
    );

    let started = append_tool_call_started_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        "toolcall_000001",
        None,
    )
    .expect("append tool call started");
    assert_eq!(started.correlation_id.as_deref(), Some("toolcall_000001"));
    assert_eq!(
        started.stream_key.as_deref(),
        Some("tool_call:toolcall_000001")
    );

    let finished = append_tool_call_finished_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        ToolCallFinishedEventArgs {
            tool_call_id: "toolcall_000001",
            status: ToolCallStatus::Succeeded,
            output_summary: Some("ok".to_string()),
            output_json: None,
            metadata: None,
            request_correlation_id: Some("req_000001"),
        },
    )
    .expect("append tool call finished");
    assert_eq!(finished.correlation_id.as_deref(), Some("req_000001"));
    assert_eq!(
        finished.stream_key.as_deref(),
        Some("tool_call:toolcall_000001")
    );

    let edit_metadata = HashlineEditMetadata {
        edit_id: "edit_000001".to_string(),
        path: "demo.txt".to_string(),
        summary: "edit demo".to_string(),
        patch_digest: "digest-edit".to_string(),
    };
    let edit = append_edit_proposed_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        "toolcall_000001",
        &edit_metadata,
        None,
    )
    .expect("append edit proposed");
    assert_eq!(edit.correlation_id.as_deref(), Some("toolcall_000001"));
    assert_eq!(edit.stream_key.as_deref(), Some("edit:edit_000001"));

    let edit_applied = append_edit_applied_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        EditAppliedEventArgs {
            tool_call_id: "toolcall_000001",
            metadata: &edit_metadata,
            new_file_digest: "digest-new-file".to_string(),
            diff_rel_path: Some("diffs/edit_000001.patch".to_string()),
            diff_digest: Some("digest-diff".to_string()),
            request_correlation_id: None,
        },
    )
    .expect("append edit applied");
    assert_eq!(
        edit_applied.correlation_id.as_deref(),
        Some("toolcall_000001")
    );
    assert_eq!(edit_applied.stream_key.as_deref(), Some("edit:edit_000001"));

    let rejected_metadata = HashlineEditMetadata {
        edit_id: "edit_000002".to_string(),
        path: "demo.txt".to_string(),
        summary: "reject demo".to_string(),
        patch_digest: "digest-reject".to_string(),
    };
    let edit_rejected = append_edit_rejected_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        "toolcall_000001",
        &rejected_metadata,
        "anchor mismatch".to_string(),
        None,
    )
    .expect("append edit rejected");
    assert_eq!(
        edit_rejected.correlation_id.as_deref(),
        Some("toolcall_000001")
    );
    assert_eq!(
        edit_rejected.stream_key.as_deref(),
        Some("edit:edit_000002")
    );

    let grant = PermissionGrant {
        grant_id: "grant_000001".to_string(),
        permission_id: "perm_000001".to_string(),
        scope: PermissionGrantScope::Run,
        expires_at: None,
        kind: PermissionKind::Shell,
        tool: PermissionToolSelector {
            effective_tool_id: "shell.run".to_string(),
            canonical_tool_id: None,
        },
        matcher: PermissionGrantMatcher::RequestDigest {
            request_digest: "digest-perm".to_string(),
        },
    };
    let grant_recorded = append_permission_grant_recorded_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        "perm_000001",
        None,
        grant,
    )
    .expect("append permission grant recorded");
    assert_eq!(
        grant_recorded.correlation_id.as_deref(),
        Some("perm_000001")
    );
    assert_eq!(
        grant_recorded.stream_key.as_deref(),
        Some("permission:perm_000001")
    );

    let artifact = ArtifactRef {
        path: "artifact.txt".to_string(),
        digest: Some("digest-artifact".to_string()),
    };
    let artifact_written = append_artifact_written_event(
        clock.as_ref(),
        redactor.as_ref(),
        run_state,
        "toolcall_000001",
        &artifact,
        None,
        None,
    )
    .expect("append artifact written");
    assert_eq!(
        artifact_written.correlation_id.as_deref(),
        Some("toolcall_000001")
    );
    assert_eq!(
        artifact_written.stream_key.as_deref(),
        Some("tool_call:toolcall_000001")
    );
}

#[tokio::test]
async fn perm_allow_path_proceeds() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = allow_shell_permission_policy();

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_allow", temp_dir.path())
        .await
        .expect("start run");

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request tool call");

    wait_for_events(
        &handle,
        &run.events_path,
        "allowed tool call to finish",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Succeeded
            )
        },
    )
    .await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallRequested(data)
                if data.tool_call_id == tool_call_id
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data)
                if data.tool_call_id == tool_call_id
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id
                    && data.status == ToolCallStatus::Succeeded
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
}
