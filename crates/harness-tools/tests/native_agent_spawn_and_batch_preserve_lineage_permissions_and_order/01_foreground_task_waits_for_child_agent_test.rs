use harness_tools::UnwrapOrAbort;

#[tokio::test]
async fn foreground_task_waits_for_child_agent_turn_after_child_tool_result() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let provider = Arc::new(ChildToolThenFinalProvider::new());
    let provider_clone = Arc::clone(&provider);
    let (handle, run, worker_id) = spawn_run_with_provider(&workspace, provider_clone).await;

    // act
    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Child reads first",
                "prompt": "Read fixture.txt, then report completion.",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);

    // assert
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    assert!(finished
        .output_summary
        .as_deref()
        .unwrap_or_abort()
        .contains("child final after read"));

    let output = finished.output_json.as_ref().unwrap_or_abort();
    assert_eq!(
        output.get("result_summary"),
        Some(&json!("child final after read"))
    );
    assert_eq!(
        output.pointer("/child_tool_call_counts/requested"),
        Some(&json!(1))
    );

    let child_request_id = output
        .get("child_request_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    let child_tool_finish_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(child_request_id)
                    && data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.task_scope)
                        == Some(TaskTerminalScope::ToolCall) =>
            {
                Some(event.seq)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let child_agent_finish_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(data)
                if event.correlation_id.as_deref() == Some(child_request_id)
                    && data
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.task_scope)
                        == Some(TaskTerminalScope::AgentTurn) =>
            {
                Some(event.seq)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let parent_task_tool_finish_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == task_tool_call_id => {
                Some(event.seq)
            }
            _ => None,
        })
        .unwrap_or_abort();

    assert!(child_tool_finish_seq < child_agent_finish_seq);
    assert!(child_agent_finish_seq < parent_task_tool_finish_seq);
    assert_eq!(provider.requests().await.len(), 2);
}

#[tokio::test]
async fn generic_task_inherits_parent_turn_model_when_subagent_model_is_implicit() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let provider = Arc::new(TaskCallingProvider::default());
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    let provider_clone = Arc::clone(&provider);
    config.provider = provider_clone;
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref_explicit = false;
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

    // act
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_task_model_inheritance", &workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .unwrap_or_abort();
    let request_id = handle
        .request_agent_turn_with_model(
            anonymous_supervisor_actor(),
            worker_id,
            "delegate to default",
            Some("default:parent-model".to_string()),
            Some(AgentModelSettings {
                variant: Some("parent-variant".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
                thinking: None,
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_request_terminal(&run.events_path, &request_id).await;
    let requests = provider.requests().await;

    // assert
    assert!(requests.len() >= 2);
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "parent-model");
    assert_eq!(requests[1].variant.as_deref(), Some("parent-variant"));
    assert_eq!(requests[1].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(requests[1].text_verbosity.as_deref(), Some("low"));
    assert_eq!(requests[1].reasoning_summary.as_deref(), Some("auto"));
}

#[tokio::test]
async fn generic_task_keeps_explicit_subagent_model_over_parent_turn_model() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let provider = Arc::new(TaskCallingProvider::default());
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    let provider_clone = Arc::clone(&provider);
    config.provider = provider_clone;
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref = "default:generic-child".to_string();
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

    // act
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_task_model_override", &workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .unwrap_or_abort();
    let request_id = handle
        .request_agent_turn_with_model(
            anonymous_supervisor_actor(),
            worker_id,
            "delegate to default",
            Some("default:parent-model".to_string()),
            Some(AgentModelSettings {
                variant: Some("parent-variant".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
                thinking: None,
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_request_terminal(&run.events_path, &request_id).await;
    let requests = provider.requests().await;

    // assert
    assert!(requests.len() >= 2);
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "generic-child");
    assert_eq!(requests[1].variant, None);
    assert_eq!(requests[1].reasoning_effort, None);
    assert_eq!(requests[1].text_verbosity, None);
    assert_eq!(requests[1].reasoning_summary, None);
}
