use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn child_agent_toolset_boundary_is_enforced() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "tool-claim-skill",
        "name: tool-claim-skill\ndescription: Claims tools but cannot grant them\nallowed_tools: task, edit",
        "Tool claim body.",
    );

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Restricted child",
                "prompt": "Stay read-only",
                "subagent_type": "explore",
                "run_in_background": true,
                "load_skills": ["tool-claim-skill"]
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    // act
    let output = finished.output_json.unwrap_or_abort();
    // assert
    assert_eq!(
        output["route"]["loaded_skills"][0]["allowed_tools"],
        json!(["task", "edit"])
    );
    assert_eq!(output["route"]["permission_posture"]["edit"], json!("deny_by_toolset"));
    assert_eq!(output["can_redelegate"], json!(false));
    assert!(!output["route"]["toolset"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|tool| tool == "task" || tool == "edit"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .unwrap_or_abort();

    let denied = handle
        .request_tool_call(
            worker_actor(child_session_id),
            Some("default".to_string()),
            "task",
            json!({
                "prompt": "try child delegation",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .expect_err("generic child must not be able to redelegate");
    assert!(denied
        .to_string()
        .contains("tool `task` is not in worker toolset"));
}
