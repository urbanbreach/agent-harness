#[tokio::test]
async fn background_output_block_waits_for_running_child_completion() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run_with_provider(
        &workspace,
        Arc::new(DelayedProvider),
    )
    .await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Delayed background child",
                "prompt": "Return a delayed completed result",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "block": true,
                "timeout_ms": 5_000
            }),
        )
        .await
        .expect("request blocking background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background output structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert_eq!(output["result_summary"], json!("delayed child result"));
    assert_eq!(output["route"], task_output["route"]);
}
#[tokio::test]
async fn background_output_retrieves_child_result_after_coordinator_resume() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Resumable background child",
                "prompt": "Return a concise completed result after resume",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;
    handle.stop_run().await.expect("stop original coordinator");

    let session_dir = workspace.join("sessions");
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.provider = Arc::new(StaticProvider);
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&[
                "task",
                "background_output",
                "background_cancel",
                "batch",
                "read",
                "bash",
            ]),
        ),
        (
            "explore".to_string(),
            named_worker_profile("explore", &["read", "glob", "grep", "list"]),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
    ]);
    let resumed = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let resumed_run = resumed
        .resume_run(run.run_id.clone(), run.run_name.clone())
        .await
        .expect("resume run");

    let output_tool_call_id = resumed
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({ "request_id": request_id }),
        )
        .await
        .expect("request background output after resume");
    wait_for_tool_call_finish(&resumed_run.events_path, &output_tool_call_id).await;

    let events = read_events(&resumed_run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background output structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["result_summary"], json!("static child result"));
    assert_eq!(output["source"], json!("event_replay"));
    assert_eq!(output["runtime"]["profile"], json!("deep"));
    assert_eq!(output["route"], task_output["route"]);
}
#[tokio::test]
async fn background_cancel_uses_same_coordinator_cancellation_path() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(BlockingProvider)).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Explicit cancellable child",
                "prompt": "Keep running until explicit cancellation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "request_id": request_id,
                "reason": "explicit background_cancel cancellation"
            }),
        )
        .await
        .expect("request explicit background cancellation");
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background_cancel structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["source"], json!("event_replay"));
    assert_eq!(output["previous_status"], json!("running"));
    assert_eq!(output["final_status"], json!("cancelled"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(true));
    assert_eq!(
        output["cancel_reason"],
        json!("explicit background_cancel cancellation")
    );
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(payload)
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && payload.reason == "explicit background_cancel cancellation"
    )));
}
#[tokio::test]
async fn background_output_can_cancel_authorized_child_request() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(BlockingProvider)).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Cancellable child",
                "prompt": "Keep running until cancelled",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "cancel": true,
                "reason": "test requested cancellation"
            }),
        )
        .await
        .expect("request background cancellation");
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background cancel structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("cancelled"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(true));
    assert_eq!(
        output["cancel_reason"],
        json!("test requested cancellation")
    );
    assert_eq!(output["runtime"]["profile"], json!("deep"));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(payload)
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && payload.reason == "test requested cancellation"
    )));
}
#[tokio::test]
async fn background_output_cancel_after_terminal_does_not_report_performed() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Completed child",
                "prompt": "Return before cancellation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "cancel": true,
                "reason": "too late to cancel"
            }),
        )
        .await
        .expect("request terminal cancellation status");
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("terminal cancel structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(false));
    assert!(output["cancel_reason"].is_null());
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(_) if event.correlation_id.as_deref() == Some(request_id.as_str())
    )));

    let explicit_cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "request_id": request_id,
                "reason": "also too late to cancel"
            }),
        )
        .await
        .expect("request terminal explicit cancellation status");
    wait_for_tool_call_finish(&run.events_path, &explicit_cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &explicit_cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("terminal explicit cancel structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["final_status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(false));
    assert!(output["cancel_reason"].is_null());
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(_) if event.correlation_id.as_deref() == Some(request_id.as_str())
    )));
}
#[tokio::test]
async fn background_output_rejects_sibling_request_ids() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let sibling_worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn sibling worker");

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Private child",
                "prompt": "Return a concise completed result",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&sibling_worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "block": false
            }),
        )
        .await
        .expect("request unauthorized background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("not in the caller's task lineage")));
}
#[tokio::test]
async fn background_cancel_rejects_sibling_request_ids() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let sibling_worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn sibling worker");

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Private cancellable child",
                "prompt": "Return a concise completed result",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    // act
    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&sibling_worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "request_id": request_id,
                "reason": "unauthorized sibling cancellation"
            }),
        )
        .await
        .expect("request unauthorized background cancel");
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("not in the caller's task lineage")));
}
#[tokio::test]
async fn background_output_rejects_excessive_block_timeout() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": "req_missing",
                "block": true,
                "timeout_ms": 300_001
            }),
        )
        .await
        .expect("request background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("timeout must be <= 300000 ms")));
}
#[tokio::test]
async fn child_agent_toolset_boundary_is_enforced() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "subagent_type": "explore",
                "description": "Restricted child",
                "prompt": "Stay read-only",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("spawn restricted child");

    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    let output = finished.output_json.expect("task structured output");
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");

    let denied = handle
        .request_tool_call(
            worker_actor(child_session_id),
            Some("explore".to_string()),
            "bash",
            json!({"command": "true", "description": "try child shell"}),
        )
        .await
        .expect_err("explore child must not be able to call bash");
    assert!(denied
        .to_string()
        .contains("tool `bash` is not in worker toolset"));
}
