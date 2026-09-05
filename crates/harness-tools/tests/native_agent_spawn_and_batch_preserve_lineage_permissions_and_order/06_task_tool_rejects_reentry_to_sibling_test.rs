use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn task_tool_rejects_reentry_to_sibling_child_session() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let sibling_parent = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    let sibling_child = handle
        .spawn_agent_idle(
            anonymous_supervisor_actor(),
            "general",
            Some(sibling_parent),
        )
        .await
        .unwrap_or_abort();

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Forbidden sibling reentry",
                "prompt": "Try to drive another parent's child",
                "subagent_type": "general",
                "session_id": sibling_child,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &reentry_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .unwrap_or_abort()
        .contains("is not a direct child of the calling agent"));
}
#[tokio::test]
async fn task_reentry_rejects_mismatched_subagent_type() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = plan_task_profiles();

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_task_reentry_boundary", &workspace)
        .await
        .unwrap_or_abort();
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .unwrap_or_abort();
    let general_child = handle
        .spawn_agent_idle(
            anonymous_supervisor_actor(),
            "general",
            Some(plan_agent_id.clone()),
        )
        .await
        .unwrap_or_abort();

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "description": "Forbidden profile reentry",
                "prompt": "Try to drive a write-capable existing child",
                "subagent_type": "explore",
                "session_id": general_child,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &reentry_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .unwrap_or_abort()
        .contains("uses profile `general`, but the request selected `explore`"));
}
#[tokio::test]
async fn batch_rejects_more_than_25_calls_preserving_input_order() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let fixture = (1..=25)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace.join("fixture.txt"), format!("{fixture}\n")).unwrap_or_abort();

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let tool_calls = (0..26)
        .map(|index| {
            json!({
                "tool": "read",
                "parameters": {
                    "filePath": "fixture.txt",
                    "offset": index + 1,
                    "limit": 1
                }
            })
        })
        .collect::<Vec<_>>();

    // act
    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({ "tool_calls": tool_calls }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);

    // assert
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().unwrap_or_abort();
    assert_eq!(output.get("requested_call_count"), Some(&json!(26)));
    assert_eq!(output.get("max_calls"), Some(&json!(25)));
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(25)));
    assert_eq!(output.pointer("/audit/failed"), Some(&json!(1)));
    assert_eq!(
        output.pointer("/audit/discarded_call_count"),
        Some(&json!(1))
    );

    let details = output
        .get("details")
        .and_then(Value::as_array)
        .unwrap_or_abort();
    assert_eq!(details.len(), 26);
    for (index, detail) in details.iter().enumerate() {
        assert_eq!(detail.get("index"), Some(&json!(index)));
        assert_eq!(
            detail.pointer("/request/parameter_shape"),
            Some(&json!("object"))
        );
        assert_eq!(
            detail.pointer("/request/parameters_redacted"),
            Some(&json!(true))
        );
        assert!(detail.get("parameters").is_none());
    }
    for detail in &details[..25] {
        assert_eq!(detail.get("success"), Some(&json!(true)));
    }
    assert_eq!(details[25].get("success"), Some(&json!(false)));
    assert!(details[25]
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("Maximum of 25 tools allowed in batch"));
}
