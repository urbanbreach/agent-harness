#[tokio::test]
async fn batch_tool_accepts_args_alias_on_real_tool_path() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    // act
    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "args": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "read", "args": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch tool");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    // assert
    let finished = find_finished(&events, &batch_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(2)));
    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(
        details[0].pointer("/request/parameter_keys/0"),
        Some(&json!("filePath"))
    );
    assert_eq!(
        details[0].pointer("/request/parameters_redacted"),
        Some(&json!(true))
    );
    assert!(details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first summary")
        .contains("|beta"));
    assert!(details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second summary")
        .contains("|alpha"));
}
#[tokio::test]
async fn batch_tool_accepts_wrapper_calls_inside_tool_calls_on_real_path() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    // act
    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"recipient_name": "functions.read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"recipient_name": "functions.read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch tool");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    // assert
    let finished = find_finished(&events, &batch_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(2)));
    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(details[0].get("tool_id"), Some(&json!("read")));
    assert!(details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first summary")
        .contains("|beta"));
    assert!(details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second summary")
        .contains("|alpha"));
}
#[tokio::test]
async fn task_tool_reenters_existing_child_session_by_session_id() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    // act
    let first_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Initial child",
                "prompt": "First child turn",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request initial task");
    wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let first_events = read_events(&run.events_path);
    let first_finished = find_finished(&first_events, &first_tool_call_id);
    let first_output = first_finished
        .output_json
        .as_ref()
        .expect("initial task output json");
    let child_session_id = first_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("child session id")
        .to_string();
    let first_request_id = first_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &first_request_id).await;

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Resume child by session id",
                "prompt": "Second child turn by session_id",
                "session_id": child_session_id,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task reentry by session_id");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let reentry_events = read_events(&run.events_path);
    // assert
    let reentry_finished = find_finished(&reentry_events, &reentry_tool_call_id);
    assert_eq!(reentry_finished.status, ToolCallStatus::Succeeded);
    let reentry_output = reentry_finished
        .output_json
        .as_ref()
        .expect("reentry task output json");
    assert_eq!(
        reentry_output
            .get("child_session_id")
            .and_then(Value::as_str),
        Some(child_session_id.as_str())
    );
    assert_eq!(
        reentry_output.get("resumed_existing_session"),
        Some(&json!(true))
    );
    assert_eq!(
        reentry_output.pointer("/child_session/resumed_existing_session"),
        Some(&json!(true))
    );
    let second_request_id = reentry_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("second child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &second_request_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let child_spawn_count = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::AgentSpawned(payload) if payload.agent_id == child_session_id
            )
        })
        .count();
    assert_eq!(child_spawn_count, 1, "reentry must not spawn a new child");
}
