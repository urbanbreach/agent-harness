use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn background_output_block_waits_for_running_child_completion() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "background-skill",
        "name: background-skill\ndescription: Background skill description",
        "BACKGROUND SKILL BODY SENTINEL",
    );

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
                "description": "Delayed background child",
                "prompt": "Return a delayed completed result",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": ["background-skill"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    assert_eq!(
        task_output["loaded_skills"][0]["stable_id"],
        json!("skill:project:background-skill")
    );
    assert_eq!(task_output["loaded_skills"][0]["body_loaded"], json!(false));
    assert!(!task_output
        .to_string()
        .contains("BACKGROUND SKILL BODY SENTINEL"));
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert_eq!(output["result_summary"], json!("delayed child result"));
    assert_eq!(output["route"], task_output["route"]);
    assert_eq!(
        output["route"]["loaded_skills"][0]["stable_id"],
        json!("skill:project:background-skill")
    );
    assert_eq!(
        output["route"]["loaded_skills"][0]["body_loaded"],
        json!(false)
    );
    assert!(!output
        .to_string()
        .contains("BACKGROUND SKILL BODY SENTINEL"));
}
#[tokio::test]
async fn background_output_retrieves_child_result_after_coordinator_resume() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    let mut initial_config = CoordinatorConfig::new(session_dir.clone());
    initial_config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    initial_config.provider = Arc::new(StaticProvider);
    initial_config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    initial_config.agent_profiles = BTreeMap::from([
        (
            "default".to_string(),
            named_worker_profile(
                "default",
                &[
                    "task",
                    "background_output",
                    "background_cancel",
                    "read",
                    "bash",
                ],
            ),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
    ]);
    let handle = spawn_coordinator(
        initial_config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("resumable_generic_task", &workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("default".to_string()),
            "task",
            json!({
                "description": "Resumable background child",
                "prompt": "Return a concise completed result after resume",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;
    handle.stop_run().await.unwrap_or_abort();

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
            "default".to_string(),
            named_worker_profile(
                "default",
                &[
                    "task",
                    "background_output",
                    "background_cancel",
                    "read",
                    "bash",
                ],
            ),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
        (
            "explore".to_string(),
            named_worker_profile("explore", &["read", "glob", "grep", "list"]),
        ),
    ]);
    let resumed = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let resumed_run = resumed
        .resume_run(run.run_id.to_string(), run.run_name.to_string())
        .await
        .expect("resume task run");

    let output_tool_call_id = resumed
        .request_tool_call(
            worker_actor(&worker_id),
            Some("default".to_string()),
            "background_output",
            json!({ "request_id": request_id }),
        )
        .await
        .expect("retrieve resumed generic child output");
    wait_for_tool_call_finish(&resumed_run.events_path, &output_tool_call_id).await;

    let events = read_events(&resumed_run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["result_summary"], json!("static child result"));
    assert_eq!(output["source"], json!("event_replay"));
    assert_eq!(output["runtime"]["profile"], json!("general"));
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
                "description": "Explicit cancellable child",
                "prompt": "Keep running until explicit cancellation",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
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
                "description": "Cancellable child",
                "prompt": "Keep running until cancelled",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("cancelled"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(true));
    assert_eq!(
        output["cancel_reason"],
        json!("test requested cancellation")
    );
    assert_eq!(output["runtime"]["profile"], json!("general"));
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
                "description": "Completed child",
                "prompt": "Return before cancellation",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &explicit_cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &explicit_cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
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
        .spawn_agent(anonymous_supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Private child",
                "prompt": "Return a concise completed result",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
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
        .unwrap_or_abort();
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
        .spawn_agent(anonymous_supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Private cancellable child",
                "prompt": "Return a concise completed result",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
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
        .unwrap_or_abort();
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
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("timeout must be <= 300000 ms")));
}
