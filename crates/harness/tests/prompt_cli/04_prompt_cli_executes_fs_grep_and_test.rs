#[tokio::test]
async fn prompt_cli_executes_fs_grep_and_completes_turn() {
    let provider = ScriptedPromptProvider::sequence(vec![
        tool_call_events(
            "call_grep",
            "grep",
            serde_json::json!({"pattern": "BETA", "path": "fixtures", "include": "*.md"}),
        ),
        text_events("Grep complete: fixtures/notes.md contains BETA on line 2."),
    ]);

    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("fixtures")).expect("create fixtures dir");
    fs::write(
        temp.path().join("fixtures/notes.md"),
        "alpha\nBETA match\ngamma\n",
    )
    .expect("write notes.md");
    fs::write(temp.path().join("fixtures/skip.txt"), "BETA hidden\n").expect("write skip.txt");

    let output = run_prompt_with_single_tool(
        temp.path(),
        provider,
        &["grep"],
        "Use grep in fixtures for BETA and summarize the hit.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "grep");
    assert!(events_body.contains("fixtures/notes.md:2: BETA match"));
}
#[tokio::test]
async fn prompt_cli_reads_absolute_workspace_path_and_completes_turn() {
    let temp = tempdir().expect("tempdir");
    let absolute_target = temp.path().join("tool-target.txt");
    fs::write(&absolute_target, "alpha\nbeta\ngamma\n").expect("seed tool target");
    let provider = ScriptedPromptProvider::sequence(vec![
        tool_call_events(
            "call_1",
            "read",
            serde_json::json!({"path": absolute_target, "offset": 1, "limit": 20}),
        ),
        text_events("Absolute read complete: alpha beta gamma."),
    ]);

    let output = run_prompt_with_single_tool(
        temp.path(),
        provider,
        &["read"],
        "Read the absolute tool-target.txt path and summarize it.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "read");
    assert!(events_body.contains("tool-target.txt"));
    assert!(events_body.contains("alpha"));
}
