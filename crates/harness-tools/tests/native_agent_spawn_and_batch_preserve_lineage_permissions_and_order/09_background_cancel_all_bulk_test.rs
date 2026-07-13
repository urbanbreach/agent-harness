use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn background_cancel_all_cancels_all_non_terminal_background_tasks() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    // act
    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(BlockingProvider)).await;

    let mut request_ids = Vec::new();
    for i in 1..=3 {
        let task_tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "category": "deep",
                    "description": format!("Background child {i}"),
                    "prompt": "Keep running until cancelled",
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
        request_ids.push(request_id);
    }

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "all": true,
                "reason": "bulk cancel all tasks"
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["all"], json!(true));
    assert_eq!(output["cancelled_count"], json!(3));
    assert_eq!(output["skipped_count"], json!(0));
    assert_eq!(output["cancel_reason"], json!("bulk cancel all tasks"));

    for request_id in &request_ids {
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            EventV1::TaskCancelled(payload)
                if event.correlation_id.as_deref() == Some(request_id.as_str())
                    && payload.reason == "bulk cancel all tasks"
        )));
    }
}

#[tokio::test]
async fn background_cancel_all_false_without_request_id_returns_validation_error() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    // act
    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "all": false
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
        .is_some_and(|summary| summary.contains("request_id is required when all is false")));
}
