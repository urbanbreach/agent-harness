#[test]
fn sessions_export_cli_redacts_secret_payloads_and_reports_manifest() {
    // arrange
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export_redaction");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_redaction",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "export-redaction".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_export_redaction",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "raw token sk-AbCdEf0123456789 and Authorization: Bearer abc.def-ghi_123"
                            .to_string(),
                    ),
                    output_digest: Some("digest-secret-output".to_string()),
                    output_json: Some(serde_json::json!({
                        "secret": "sk-AbCdEf0123456789",
                        "authorization": "Bearer abc.def-ghi_123"
                    })),
                    metadata: None,
                }),
            ),
            envelope(
                "run_export_redaction",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let export_path = session_dir.path().join("session-export-redacted.json");
    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().expect("session dir utf-8"),
        "sessions",
        "export",
        "run_export_redaction",
        "--output",
        export_path.to_str().expect("export path utf-8"),
    ]);
    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let export_text = std::fs::read_to_string(&export_path).expect("read redacted export");
    assert!(!export_text.contains("sk-AbCdEf0123456789"));
    assert!(!export_text.contains("Bearer abc.def-ghi_123"));
    assert!(export_text.contains("[REDACTED_API_KEY]"));
    assert!(export_text.contains("Bearer [REDACTED]"));

    let bundle: serde_json::Value =
        serde_json::from_str(&export_text).expect("redacted export bundle should parse");
    assert_eq!(bundle["support"]["redaction_manifest"]["status"], "clean");
    assert_eq!(
        bundle["support"]["secret_scan_status"]["secret_finding_count"],
        0
    );
}

#[test]
fn sessions_export_cli_support_includes_artifact_index() {
    // arrange
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export_artifacts");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &delegated_recovery_events("run_export_artifacts"),
    );

    let export_path = session_dir.path().join("session-export-artifacts.json");
    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().expect("session dir utf-8"),
        "sessions",
        "export",
        "run_export_artifacts",
        "--output",
        export_path.to_str().expect("export path utf-8"),
    ]);
    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).expect("read artifact-index export"),
    )
    .expect("artifact-index export bundle should parse");
    assert_eq!(
        bundle["support"]["artifact_index"][0]["path"],
        "artifacts/delegated/task-output.json"
    );
    assert_eq!(
        bundle["support"]["artifact_index"][0]["child_session_id"],
        "child-run-001"
    );
    assert_eq!(
        bundle["support"]["artifact_index"][0]["canonical_tool_id"],
        "agent.spawn"
    );
}
