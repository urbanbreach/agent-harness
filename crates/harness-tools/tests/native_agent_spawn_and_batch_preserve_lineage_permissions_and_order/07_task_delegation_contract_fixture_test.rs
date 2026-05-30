#[tokio::test]
async fn task_delegation_fixture_covers_structured_prompt_lineage_and_summary_cap() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "ws9-skill",
        "name: ws9-skill\ndescription: WS9 skill description",
        "WS9 skill body marker.",
    );

    let provider = Arc::new(DelegationContractProvider::new());
    let profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "background_cancel"]),
        ),
        (
            "visual-engineering".to_string(),
            named_worker_profile_with_prompt(
                "visual-engineering",
                &["read", "bash"],
                "visual-engineering category prompt append marker",
            ),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
    ]);
    let (handle, run, worker_id) =
        spawn_run_with_provider_and_profiles(&workspace, provider.clone(), profiles).await;

    let structured_prompt = "context: inspect the WS9 task contract\n\
goal: prove delegation evidence\n\
downstream use: parent will synchronize PRD evidence\n\
request: return an intentionally oversized summary\n\
required tools: no tool calls\n\
must-do: preserve the skill and category context\n\
must-not-do: do not edit files";
    // act
    let sync_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "visual-engineering",
                "description": "WS9 sync child",
                "prompt": structured_prompt,
                "run_in_background": false,
                "load_skills": ["ws9-skill"]
            }),
        )
        .await
        .expect("request sync task");
    wait_for_tool_call_finish(&run.events_path, &sync_tool_call_id).await;

    let background_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "visual-engineering",
                "description": "WS9 background child",
                "prompt": structured_prompt,
                "run_in_background": true,
                "load_skills": ["ws9-skill"]
            }),
        )
        .await
        .expect("request background task");
    wait_for_tool_call_finish(&run.events_path, &background_tool_call_id).await;

    let events = read_events(&run.events_path);
    // assert
    let sync_finished = find_finished(&events, &sync_tool_call_id);
    assert_eq!(sync_finished.status, ToolCallStatus::Succeeded);
    let sync_output = sync_finished
        .output_json
        .as_ref()
        .expect("sync task structured output");
    assert_delegation_output_is_capped(sync_output, "foreground");
    assert_eq!(sync_output["route"]["requested_category"], json!("visual-engineering"));
    assert_eq!(sync_output["route"]["resolved_profile"], json!("visual-engineering"));
    assert_eq!(sync_output["loaded_skills"][0]["name"], json!("ws9-skill"));
    assert_eq!(sync_output["lineage"]["parent_tool_call_id"], json!(sync_tool_call_id));

    let background_finished = find_finished(&events, &background_tool_call_id);
    assert_eq!(background_finished.status, ToolCallStatus::Succeeded);
    let background_task_output = background_finished
        .output_json
        .as_ref()
        .expect("background task structured output");
    assert_eq!(background_task_output["mode"], json!("background"));
    assert_eq!(background_task_output["status"], json!("scheduled"));
    assert_eq!(
        background_task_output["lineage"]["parent_tool_call_id"],
        json!(background_tool_call_id)
    );
    let background_request_id = background_task_output["child_request_id"]
        .as_str()
        .expect("background child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &background_request_id).await;

    let background_output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": background_request_id,
                "block": true,
                "timeout_ms": 1
            }),
        )
        .await
        .expect("request background output");
    wait_for_tool_call_finish(&run.events_path, &background_output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let background_output_finished = find_finished(&events, &background_output_tool_call_id);
    assert_eq!(background_output_finished.status, ToolCallStatus::Succeeded);
    let background_output = background_output_finished
        .output_json
        .as_ref()
        .expect("background output structured json");
    assert_delegation_output_is_capped(background_output, "background");
    assert_eq!(background_output["route"]["resolved_profile"], json!("visual-engineering"));
    assert_eq!(background_output["source"], json!("event_replay"));

    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCompleted(payload)
            if event.correlation_id.as_deref() == Some(background_request_id.as_str())
                && payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.lineage.as_ref())
                    .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                    == Some(background_tool_call_id.as_str())
    )));

    let child_prompts = provider
        .requests()
        .await
        .into_iter()
        .map(|request| {
            request
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|prompt| prompt.contains("visual-engineering category prompt append marker"))
        .collect::<Vec<_>>();
    assert_eq!(
        child_prompts.len(),
        2,
        "expected sync and background child prompts"
    );
    for prompt in child_prompts {
        assert!(prompt.contains("visual-engineering category prompt append marker"));
        assert!(prompt.contains("<skill_content name=\"ws9-skill\">"));
        assert!(prompt.contains("WS9 skill body marker."));
        for field in [
            "context:",
            "goal:",
            "downstream use:",
            "request:",
            "required tools:",
            "must-do:",
            "must-not-do:",
        ] {
            assert!(prompt.contains(field), "missing structured field {field}");
        }
    }
}

fn assert_delegation_output_is_capped(output: &Value, expected_mode: &str) {
    assert_eq!(output["mode"], json!(expected_mode));
    let result_summary = output["result_summary"]
        .as_str()
        .expect("parent-visible result summary");
    assert!(
        result_summary.chars().count() <= 1201,
        "summary should stay within the documented 1200 char cap plus ellipsis"
    );
    assert!(result_summary.ends_with('…'));
    assert_eq!(output["child_summary"]["kind"], json!("result"));
    assert_eq!(output["child_summary"]["max_chars"], json!(1200));
    assert_eq!(output["child_summary"]["truncated"], json!(true));
    assert_eq!(
        output["child_summary"]["summary"],
        output["result_summary"]
    );
    assert!(
        output["child_summary"]["original_chars"]
            .as_u64()
            .is_some_and(|count| count > 1200)
    );
}
