use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn batch_model_facing_text_enumerates_order_status_and_permission_attribution() {
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
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}},
                    {"tool": "batch", "parameters": {"tool_calls": []}}
                ]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);
    let summary = finished.output_summary.unwrap_or_abort();

    // assert
    assert!(summary.contains("Batch results (input order)"));
    assert!(summary.contains("[0] read: succeeded"));
    assert!(summary.contains("[1] batch: failed"));
    assert!(summary.contains("Permission attribution:"));
    assert!(summary.contains("own coordinator permission check"));
}
