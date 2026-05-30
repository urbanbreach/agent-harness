#[tokio::test]
async fn child_agent_toolset_boundary_is_enforced() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "tool-claim-skill",
        "name: tool-claim-skill\ndescription: Claims tools but cannot grant them\nallowed_tools: bash, edit",
        "Tool claim body.",
    );

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "subagent_type": "explore",
                "description": "Restricted child",
                "prompt": "Stay read-only",
                "run_in_background": true,
                "load_skills": ["tool-claim-skill"]
            }),
        )
        .await
        .expect("spawn restricted child");

    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    // act
    let output = finished.output_json.expect("task structured output");
    // assert
    assert_eq!(
        output["route"]["loaded_skills"][0]["allowed_tools"],
        json!(["bash", "edit"])
    );
    assert_eq!(output["route"]["permission_posture"]["bash"], json!("deny_by_toolset"));
    assert_eq!(output["route"]["permission_posture"]["edit"], json!("deny_by_toolset"));
    assert!(!output["route"]["toolset"]
        .as_array()
        .expect("child toolset")
        .iter()
        .any(|tool| tool == "bash" || tool == "edit"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");

    let denied = handle
        .request_tool_call(
            worker_actor(child_session_id),
            Some("explore".to_string()),
            "bash",
            json!({"command": "true", "description": "try child shell"}),
        )
        .await
        .expect_err("explore child must not be able to call bash");
    assert!(denied
        .to_string()
        .contains("tool `bash` is not in worker toolset"));
}
