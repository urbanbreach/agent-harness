use harness_tools::UnwrapOrAbort;

#[tokio::test]
async fn background_output_wait_any_returns_on_first_cancel_while_peer_still_running() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(BlockingProvider)).await;

    let mut request_ids = Vec::new();
    for i in 1..=2 {
        let task_tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "category": "deep",
                    "description": format!("Blocking child {i}"),
                    "prompt": "Keep running until cancelled",
                    "run_in_background": true,
                    "load_skills": []
                }),
            )
            .await
            .unwrap_or_abort();
        wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
        let request_id = find_finished(&read_events(&run.events_path), &task_tool_call_id)
            .output_json
            .unwrap_or_abort()["child_request_id"]
            .as_str()
            .unwrap_or_abort()
            .to_string();
        request_ids.push(request_id);
    }

    // act — start wait_any concurrently, then cancel the first child
    let wait_handle = handle.clone();
    let wait_worker = worker_id.clone();
    let wait_ids = request_ids.clone();
    let wait_fut = tokio::spawn(async move {
        wait_handle
            .request_tool_call(
                worker_actor(&wait_worker),
                Some("deep".to_string()),
                "background_output",
                json!({
                    "request_ids": wait_ids,
                    "wait_mode": "any",
                    "block": true,
                    "timeout_ms": 5_000
                }),
            )
            .await
    });

    // Yield so the wait tool can start blocking before cancel.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "request_id": request_ids[0],
                "reason": "cancel first terminal for wait_any"
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let wait_tool_call_id = wait_fut.await.unwrap_or_abort().unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &wait_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &wait_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["wait_mode"], json!("any"));
    assert_eq!(output["satisfied"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(
        output["first_terminal_request_id"],
        json!(request_ids[0])
    );
    assert_eq!(output["results"].as_array().map(Vec::len), Some(2));
    let first = output["results"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|row| row["request_id"] == json!(request_ids[0]))
        .unwrap_or_abort();
    assert_eq!(first["status"], json!("cancelled"));
    assert_eq!(first["terminal"], json!(true));
    let second = output["results"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|row| row["request_id"] == json!(request_ids[1]))
        .unwrap_or_abort();
    assert_eq!(second["terminal"], json!(false));
}

#[tokio::test]
async fn background_output_wait_all_returns_when_every_request_is_terminal() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let mut request_ids = Vec::new();
    for i in 1..=2 {
        let task_tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "category": "deep",
                    "description": format!("Completing child {i}"),
                    "prompt": "Return a concise completed result",
                    "run_in_background": true,
                    "load_skills": []
                }),
            )
            .await
            .unwrap_or_abort();
        wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
        let request_id = find_finished(&read_events(&run.events_path), &task_tool_call_id)
            .output_json
            .unwrap_or_abort()["child_request_id"]
            .as_str()
            .unwrap_or_abort()
            .to_string();
        request_ids.push(request_id);
    }

    // act
    let wait_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_ids": request_ids,
                "wait_mode": "all",
                "block": true,
                "timeout_ms": 5_000
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &wait_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &wait_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["wait_mode"], json!("all"));
    assert_eq!(output["satisfied"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert_eq!(output["terminal"], json!(true));
    let results = output["results"].as_array().unwrap_or_abort();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|row| row["terminal"] == json!(true)));
    assert!(results
        .iter()
        .all(|row| row["status"] == json!("completed")));
}

#[tokio::test]
async fn background_output_wait_all_completes_when_cancel_makes_remaining_terminal() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(BlockingProvider)).await;

    let mut request_ids = Vec::new();
    for i in 1..=2 {
        let task_tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "category": "deep",
                    "description": format!("Blocking child {i}"),
                    "prompt": "Keep running until cancelled",
                    "run_in_background": true,
                    "load_skills": []
                }),
            )
            .await
            .unwrap_or_abort();
        wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
        let request_id = find_finished(&read_events(&run.events_path), &task_tool_call_id)
            .output_json
            .unwrap_or_abort()["child_request_id"]
            .as_str()
            .unwrap_or_abort()
            .to_string();
        request_ids.push(request_id);
    }

    // act — wait_all blocks until both cancelled
    let wait_handle = handle.clone();
    let wait_worker = worker_id.clone();
    let wait_ids = request_ids.clone();
    let wait_fut = tokio::spawn(async move {
        wait_handle
            .request_tool_call(
                worker_actor(&wait_worker),
                Some("deep".to_string()),
                "background_output",
                json!({
                    "request_ids": wait_ids,
                    "wait_mode": "all",
                    "block": true,
                    "timeout_ms": 5_000
                }),
            )
            .await
    });

    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_cancel",
            json!({
                "all": true,
                "reason": "cancel mid wait_all"
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let wait_tool_call_id = wait_fut.await.unwrap_or_abort().unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &wait_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &wait_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["wait_mode"], json!("all"));
    assert_eq!(output["satisfied"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    let results = output["results"].as_array().unwrap_or_abort();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|row| row["terminal"] == json!(true)));
    assert!(results
        .iter()
        .all(|row| row["status"] == json!("cancelled")));
}

#[tokio::test]
async fn background_output_multi_wait_requires_wait_mode() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let (handle, run, worker_id) = spawn_run(&workspace).await;

    // act
    let wait_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_ids": ["req_a", "req_b"],
                "block": true,
                "timeout_ms": 1_000
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &wait_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &wait_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("wait_mode is required")));
}
