#[tokio::test]
async fn task_tool_reenters_existing_child_session_by_task_id() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

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
    let child_task_id = first_output
        .get("task_id")
        .and_then(Value::as_str)
        .expect("child task id")
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
                "description": "Resume child by task id",
                "prompt": "Second child turn by task_id",
                "task_id": child_task_id,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task reentry by task_id");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let reentry_events = read_events(&run.events_path);
    // act
    let reentry_finished = find_finished(&reentry_events, &reentry_tool_call_id);
    // assert
    assert_eq!(reentry_finished.status, ToolCallStatus::Succeeded);
    let reentry_output = reentry_finished
        .output_json
        .as_ref()
        .expect("reentry task output json");
    assert_eq!(
        reentry_output
            .get("child_session_id")
            .and_then(Value::as_str),
        Some(child_task_id.as_str())
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
                EventV1::AgentSpawned(payload) if payload.agent_id == child_task_id
            )
        })
        .count();
    assert_eq!(child_spawn_count, 1, "reentry must not spawn a new child");
}
