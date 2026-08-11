use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn background_output_retrieves_completed_child_result_by_request_id() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Background child",
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
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "block": true,
                "timeout_ms": 1
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    // act
    let finished = find_finished(&events, &output_tool_call_id);
    // assert
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .unwrap_or_abort();
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert!(output["result_summary"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(output["runtime"]["profile"], json!("general"));
    assert_eq!(output["runtime"]["model_ref"], json!("default:test-model"));
    assert_eq!(output["runtime"]["can_redelegate"], json!(false));
    assert!(output["next_actions"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|action| action["action"] == json!("check_status")));
}
