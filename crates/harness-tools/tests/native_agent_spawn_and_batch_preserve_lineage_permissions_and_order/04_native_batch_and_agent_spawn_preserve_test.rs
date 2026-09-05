use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn native_batch_and_agent_spawn_preserve_child_lineage_permissions_and_order() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_numbered_fixture(&workspace);
    write_skill_fixture(&workspace, "rust-best-practices");

    let (handle, run, worker_id) = spawn_run_with_partial_shell_permissions(&workspace).await;

    let native_spawn_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Native background child",
                "prompt": "Say hello from native child",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
                "command": "delegate-native",
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &native_spawn_tool_call_id).await;

    let compat_task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Compat background child",
                "prompt": "Say hello from compat child",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &compat_task_tool_call_id).await;

    let native_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt"}},
                    {"tool": "bash", "parameters": {"command": "ls", "workdir": ".", "description": "List workspace"}},
                    {
                        "tool": "batch",
                        "parameters": {
                            "tool_calls": [{"tool": "read", "parameters": {"filePath": "fixture.txt"}}]
                        }
                    },
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &native_batch_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &compat_batch_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);

    let native_spawn_finished = find_finished(&events, &native_spawn_tool_call_id);
    assert_eq!(native_spawn_finished.status, ToolCallStatus::Succeeded);
    let native_spawn_metadata = native_spawn_finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        native_spawn_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(native_spawn_metadata.alias_source_tool_id.as_deref(), None);
    let native_spawn_output = native_spawn_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(native_spawn_output.get("mode"), Some(&json!("background")));
    assert_eq!(native_spawn_output.get("status"), Some(&json!("scheduled")));
    let native_child_session = native_spawn_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    let native_child_request = native_spawn_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    assert_eq!(
        native_spawn_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some(native_child_session)
    );
    assert_eq!(
        native_spawn_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some(native_child_request)
    );

    let native_completed = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload)
                if payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.lineage.as_ref())
                    .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                    == Some(native_spawn_tool_call_id.as_str()) =>
            {
                Some(payload)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some(native_child_session)
    );
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some(native_child_request)
    );

    let compat_task_finished = find_finished(&events, &compat_task_tool_call_id);
    let compat_task_metadata = compat_task_finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        compat_task_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(compat_task_metadata.alias_source_tool_id.as_deref(), None);
    let compat_task_output = compat_task_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(compat_task_output.get("mode"), Some(&json!("background")));
    assert_eq!(
        compat_task_output
            .get("child_session_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
    );

    let native_batch_finished = find_finished(&events, &native_batch_tool_call_id);
    let native_batch_metadata = native_batch_finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        native_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(native_batch_metadata.alias_source_tool_id.as_deref(), None);
    let native_batch_output = native_batch_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        native_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/nested_batch_disallowed"),
        Some(&json!(true))
    );
    assert_eq!(
        native_batch_output.pointer("/audit/successful"),
        Some(&json!(2))
    );
    assert_eq!(
        native_batch_output.pointer("/audit/failed"),
        Some(&json!(2))
    );
    let native_details = native_batch_output
        .get("details")
        .and_then(Value::as_array)
        .unwrap_or_abort();
    assert_eq!(native_details.len(), 4);
    assert_eq!(native_details[0].get("index"), Some(&json!(0)));
    assert_eq!(native_details[0].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        native_details[0].pointer("/request/parameter_keys/0"),
        Some(&json!("filePath"))
    );
    assert_eq!(
        native_details[0].pointer("/request/parameters_redacted"),
        Some(&json!(true))
    );
    assert!(native_details[0].get("parameters").is_none());
    assert_eq!(native_details[0].get("success"), Some(&json!(true)));

    assert_eq!(native_details[1].get("index"), Some(&json!(1)));
    assert_eq!(native_details[1].get("tool_id"), Some(&json!("bash")));
    assert_eq!(native_details[1].get("success"), Some(&json!(false)));
    assert!(native_details[1]
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("tool call denied"));

    assert_eq!(native_details[2].get("index"), Some(&json!(2)));
    assert_eq!(native_details[2].get("tool_id"), Some(&json!("batch")));
    assert_eq!(native_details[2].get("success"), Some(&json!(false)));
    assert!(native_details[2]
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("cannot be nested"));

    assert_eq!(native_details[3].get("index"), Some(&json!(3)));
    assert_eq!(native_details[3].get("tool_id"), Some(&json!("read")));
    assert_eq!(native_details[3].get("success"), Some(&json!(true)));

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == EventPermissionDecision::Deny
                    && data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));

    let compat_batch_finished = find_finished(&events, &compat_batch_tool_call_id);
    let compat_batch_metadata = compat_batch_finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        compat_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(compat_batch_metadata.alias_source_tool_id.as_deref(), None);

    let compat_batch_output = compat_batch_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .unwrap_or_abort();
    assert_eq!(compat_details.len(), 2);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("|line-02"));
    assert!(compat_details[1]
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("|line-01"));
}
#[tokio::test]
async fn compat_task_and_batch_delegate_to_native_orchestration() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_numbered_fixture(&workspace);
    write_skill_fixture(&workspace, "rust-best-practices");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let compat_task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Compat child",
                "prompt": "Say hello from compat child",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &compat_task_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {
                        "tool": "batch",
                        "parameters": {
                            "tool_calls": [
                                {"tool": "read", "parameters": {"filePath": "fixture.txt"}}
                            ]
                        }
                    },
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &compat_batch_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);

    let compat_task_finished = find_finished(&events, &compat_task_tool_call_id);
    assert_eq!(compat_task_finished.status, ToolCallStatus::Succeeded);
    let compat_task_metadata = compat_task_finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        compat_task_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(compat_task_metadata.alias_source_tool_id.as_deref(), None);
    let compat_task_output = compat_task_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(compat_task_output.get("mode"), Some(&json!("background")));
    assert_eq!(compat_task_output.get("status"), Some(&json!("scheduled")));
    assert_eq!(
        compat_task_output
            .get("child_session_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
    );
    assert_eq!(
        compat_task_output
            .get("child_request_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref())
    );

    let compat_batch_finished = find_finished(&events, &compat_batch_tool_call_id);
    assert_eq!(compat_batch_finished.status, ToolCallStatus::Succeeded);
    let compat_batch_metadata = compat_batch_finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        compat_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(compat_batch_metadata.alias_source_tool_id.as_deref(), None);
    let compat_batch_output = compat_batch_finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        compat_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        compat_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        compat_batch_output.pointer("/execution/nested_batch_disallowed"),
        Some(&json!(true))
    );
    assert_eq!(
        compat_batch_output.pointer("/audit/successful"),
        Some(&json!(2))
    );
    assert_eq!(
        compat_batch_output.pointer("/audit/failed"),
        Some(&json!(1))
    );
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .unwrap_or_abort();
    assert_eq!(compat_details.len(), 3);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[0].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        compat_details[0].pointer("/request/parameter_keys/0"),
        Some(&json!("filePath"))
    );
    assert_eq!(
        compat_details[0].pointer("/request/parameters_redacted"),
        Some(&json!(true))
    );
    assert_eq!(
        compat_details[0].get("canonical_tool_id"),
        Some(&json!("read"))
    );
    assert_eq!(compat_details[0].get("success"), Some(&json!(true)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("|line-02"));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert_eq!(compat_details[1].get("tool_id"), Some(&json!("batch")));
    assert_eq!(
        compat_details[1].get("canonical_tool_id"),
        Some(&json!("batch"))
    );
    assert_eq!(compat_details[1].get("success"), Some(&json!(false)));
    assert!(compat_details[1]
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("cannot be nested"));
    assert_eq!(compat_details[2].get("index"), Some(&json!(2)));
    assert_eq!(compat_details[2].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        compat_details[2].get("canonical_tool_id"),
        Some(&json!("read"))
    );
    assert_eq!(compat_details[2].get("success"), Some(&json!(true)));
    assert!(compat_details[2]
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_abort()
        .contains("|line-01"));
}
