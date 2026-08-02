use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn foreground_task_waits_for_child_agent_turn_after_child_tool_result() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let provider = Arc::new(ChildToolThenFinalProvider::new());
    let provider_clone = Arc::clone(&provider);
    let (handle, run, worker_id) = spawn_run_with_provider(&workspace, provider_clone).await;

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

    assert!(
        child_tool_finish_seq < child_agent_finish_seq,
        "child tool task should finish before the child agent turn"
    );
    assert!(
        child_agent_finish_seq < parent_task_tool_finish_seq,
        "foreground task tool must wait for child agent turn completion"
    );

    let requests = provider.requests().await;
    assert_eq!(
        requests.len(),
        2,
        "child should make tool-use and final requests"
    );
}
#[tokio::test]
async fn task_subagent_inherits_parent_turn_model_when_profile_model_is_defaulted() {
    // arrange
    // act
    // assert
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
            "delegate to general",
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
    assert!(
        requests.len() >= 2,
        "expected parent and child provider requests, got {requests:#?}"
    );
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "parent-model");
    assert_eq!(requests[1].variant.as_deref(), Some("parent-variant"));
    assert_eq!(requests[1].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(requests[1].text_verbosity.as_deref(), Some("low"));
    assert_eq!(requests[1].reasoning_summary.as_deref(), Some("auto"));
}
#[tokio::test]
async fn task_subagent_keeps_explicit_profile_model_over_parent_turn_model() {
    // arrange
    // act
    // assert
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
    general.model_ref = "default:general".to_string();
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

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
            "delegate to general",
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
    assert!(
        requests.len() >= 2,
        "expected parent and child provider requests, got {requests:#?}"
    );
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "general");
    assert_eq!(requests[1].variant, None);
    assert_eq!(requests[1].reasoning_effort, None);
    assert_eq!(requests[1].text_verbosity, None);
    assert_eq!(requests[1].reasoning_summary, None);
}
#[tokio::test]
async fn native_plan_exit_switches_to_build_agent_after_approval() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry_with_question_answers(
        ShellAllowlist::default(),
        vec![vec!["Yes".to_string()]],
    ));
    config.agent_profiles = BTreeMap::from([
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
        ("build".to_string(), named_worker_profile("build", &[])),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_exit", &workspace)
        .await
        .unwrap_or_abort();
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "plan_exit",
            json!({}),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["agent"], "build");
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    assert_eq!(output["approved"], true);
    let build_agent_id = output["build_agent_id"]
        .as_str()
        .unwrap_or_abort()
        .to_string();
    assert!(output["request_id"].as_str().is_some());

    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == build_agent_id
                && payload.profile == "build"
                && payload.parent_agent_id.as_deref() == Some(plan_agent_id.as_str())
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from plan to build")
                && payload.text.contains("has been approved, and you can now edit files")
                && payload.text.contains(".agent-harness/plans/")
    )));
}
#[tokio::test]
async fn native_plan_exit_decline_leaves_plan_agent_active_without_spawning_build() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry_with_question_answers(
        ShellAllowlist::default(),
        vec![vec!["No".to_string()]],
    ));
    config.agent_profiles = BTreeMap::from([
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
        ("build".to_string(), named_worker_profile("build", &[])),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_exit_decline", &workspace)
        .await
        .unwrap_or_abort();
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "plan_exit",
            json!({}),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["agent"], "plan");
    assert_eq!(output["approved"], false);
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "build"
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from plan to build")
    )));
}
#[tokio::test]
async fn native_plan_enter_switches_to_plan_agent_after_approval() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry_with_question_answers(
        ShellAllowlist::default(),
        vec![vec!["Yes".to_string()]],
    ));
    config.agent_profiles = BTreeMap::from([
        (
            "build".to_string(),
            named_worker_profile("build", &["plan_enter"]),
        ),
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_enter", &workspace)
        .await
        .unwrap_or_abort();
    let build_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "build", None)
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&build_agent_id),
            Some("build".to_string()),
            "plan_enter",
            json!({"goal": "implement parity", "reason": "multi-file change"}),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["agent"], "plan");
    assert_eq!(output["goal"], "implement parity");
    assert_eq!(output["approved"], true);
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    let plan_agent_id = output["plan_agent_id"]
        .as_str()
        .unwrap_or_abort()
        .to_string();
    assert!(output["request_id"].as_str().is_some());

    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == plan_agent_id
                && payload.profile == "plan"
                && payload.parent_agent_id.as_deref() == Some(build_agent_id.as_str())
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from build to plan")
                && payload.text.contains("Original goal to plan: implement parity")
                && payload.text.contains(".agent-harness/plans/")
    )));
}
