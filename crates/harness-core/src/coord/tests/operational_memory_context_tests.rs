use super::*;

pub(crate) fn operational_memory_records_read_and_modified_files_from_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_records",
        &operational_memory_history_events(
            "run_operational_memory_records",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: "read src/lib.rs".to_string(),
                args_digest: "digest-read".to_string(),
                metadata: Some(tool_metadata("read")),
            })],
            vec![
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_read".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read completed".to_string()),
                    output_digest: Some("digest-read-output".to_string()),
                    output_json: Some(json!({ "path": "src/lib.rs" })),
                    metadata: Some(tool_metadata("read")),
                }),
                EventV1::EditApplied(EditAppliedEvent {
                    edit_id: "edit_000001".to_string(),
                    path: "src/lib.rs".to_string(),
                    new_file_digest: "digest-new".to_string(),
                    diff_rel_path: None,
                    diff_digest: None,
                }),
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/toolcalls/toolcall_edit/result.json".to_string(),
                    digest: "digest-artifact".to_string(),
                    bytes: 42,
                    tool_call_id: Some("toolcall_edit".to_string()),
                    tool_metadata: Some(ToolIdentityMetadata {
                        canonical_tool_id: Some("edit".to_string()),
                        alias_source_tool_id: None,
                    }),
                    metadata: std::collections::BTreeMap::from([(
                        "path".to_string(),
                        "src/generated.rs".to_string(),
                    )]),
                }),
            ],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_records",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );

    assert_eq!(checkpoint.facts.read_files.len(), 1);
    assert_eq!(checkpoint.facts.read_files[0].path, "src/lib.rs");
    assert_eq!(checkpoint.facts.read_files[0].operation, "read");
    assert_eq!(checkpoint.facts.read_files[0].first_seq, Some(5));
    assert_eq!(checkpoint.facts.read_files[0].last_seq, Some(5));
    assert_eq!(
        checkpoint.facts.read_files[0].sources,
        vec!["tool:toolcall_read"]
    );
    assert_eq!(checkpoint.facts.modified_files.len(), 2);
    assert!(checkpoint.facts.modified_files.iter().any(|fact| {
        fact.path == "src/generated.rs" && fact.sources == vec!["artifact:toolcall_edit"]
    }));
    assert!(checkpoint
        .facts
        .modified_files
        .iter()
        .any(|fact| { fact.path == "src/lib.rs" && fact.sources == vec!["edit:edit_000001"] }));
    assert_eq!(
        checkpoint.facts.touched_files,
        vec!["src/generated.rs".to_string(), "src/lib.rs".to_string()]
    );
    assert!(checkpoint.summary.contains("## Operational Memory"));
    assert!(checkpoint.summary.contains("Read files:"));
    assert!(checkpoint.summary.contains("Modified files:"));
    assert!(checkpoint.summary.contains("src/generated.rs"));
}

pub(crate) fn compaction_preserves_file_tool_skill_todo_and_plan_context() {
    // arrange
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let run_id = "run_compaction_preserves_context_kinds";
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        run_id,
        &operational_memory_history_events(
            run_id,
            &first_answer,
            &second_answer,
            vec![
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_read".to_string(),
                    tool_id: "read".to_string(),
                    args_summary: "read src/lib.rs".to_string(),
                    args_digest: "digest-read".to_string(),
                    metadata: Some(tool_metadata("read")),
                }),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_read".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read current library surface".to_string()),
                    output_digest: Some("digest-read-output".to_string()),
                    output_json: Some(json!({ "path": "src/lib.rs" })),
                    metadata: Some(tool_metadata("read")),
                }),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_bash".to_string(),
                    tool_id: "bash".to_string(),
                    args_summary: "run focused compaction verification".to_string(),
                    args_digest: "digest-bash".to_string(),
                    metadata: Some(tool_metadata("bash")),
                }),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_bash".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("focused compaction verification passed".to_string()),
                    output_digest: Some("digest-bash-output".to_string()),
                    output_json: None,
                    metadata: Some(tool_metadata("bash")),
                }),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_skill".to_string(),
                    tool_id: "skill".to_string(),
                    args_summary: "load karpathy-guidelines".to_string(),
                    args_digest: "digest-skill".to_string(),
                    metadata: Some(tool_metadata("skill")),
                }),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_skill".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("loaded skill karpathy-guidelines".to_string()),
                    output_digest: Some("digest-skill-output".to_string()),
                    output_json: None,
                    metadata: Some(tool_metadata("skill")),
                }),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_todo".to_string(),
                    tool_id: "todowrite".to_string(),
                    args_summary: "record preservation checklist".to_string(),
                    args_digest: "digest-todo".to_string(),
                    metadata: Some(tool_metadata("todowrite")),
                }),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_todo".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("todo checklist keeps compaction preservation pending".to_string()),
                    output_digest: Some("digest-todo-output".to_string()),
                    output_json: None,
                    metadata: Some(tool_metadata("todowrite")),
                }),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_plan".to_string(),
                    tool_id: "plan_exit".to_string(),
                    args_summary: "handoff .agent-harness/plans/run_compaction_preservation.md".to_string(),
                    args_digest: "digest-plan".to_string(),
                    metadata: Some(tool_metadata("plan_exit")),
                }),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_plan".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "plan handoff references .agent-harness/plans/run_compaction_preservation.md"
                            .to_string(),
                    ),
                    output_digest: Some("digest-plan-output".to_string()),
                    output_json: None,
                    metadata: Some(tool_metadata("plan_exit")),
                }),
            ],
            Vec::new(),
        ),
    );

    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        run_id,
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
        // act
    );

    // assert
    assert!(checkpoint
        .facts
        .read_files
        .iter()
        .any(|fact| fact.path == "src/lib.rs"));
    let operation_facts = checkpoint.facts.operation_facts.join("\n");
    assert!(operation_facts.contains("tool bash"));
    assert!(operation_facts.contains("skill"));
    assert!(operation_facts.contains("karpathy-guidelines"));
    assert!(operation_facts.contains("todowrite"));
    assert!(operation_facts.contains("todo checklist"));
    assert!(operation_facts.contains("plan_exit"));
    assert!(operation_facts.contains("run_compaction_preservation.md"));
    assert!(checkpoint.summary.contains("src/lib.rs"));
    assert!(checkpoint.summary.contains("tool bash"));
    assert!(checkpoint.summary.contains("karpathy-guidelines"));
    assert!(checkpoint.summary.contains("todo checklist"));
    assert!(checkpoint
        .summary
        .contains("run_compaction_preservation.md"));
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.recent_turns[0].user_prompt, "second question");
}
