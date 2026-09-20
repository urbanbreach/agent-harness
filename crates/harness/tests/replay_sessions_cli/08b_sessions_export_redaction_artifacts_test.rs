use harness::UnwrapOrAbort;

#[test]
fn sessions_export_cli_omits_legacy_mcp_media_without_changing_journal() {
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_export_media");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    let content = serde_json::json!([
        {"type": "text", "text": "neighboring text", "data": "application-data", "blob": "application-blob"},
        {"type": "image", "mimeType": "image/png", "data": "aW1hZ2UtcHJpdmF0ZQ=="},
        {"type": "audio", "mimeType": "audio/wav", "data": "YXVkaW8tcHJpdmF0ZQ=="},
        {"type": "resource", "resource": {
            "uri": "fixture://media", "mimeType": "application/octet-stream", "blob": "cmVzb3VyY2UtcHJpdmF0ZQ=="
        }}
    ]);
    let application = serde_json::json!({
        "type": "image", "data": "application-image-data", "blob": "application-blob",
        "details": [{"structured_output": {
            "server": {"id": "application"}, "protocolVersion": "2025-06-18",
            "payload": {"result": {"content": [{"type": "image", "data": "application-envelope-data"}]}}
        }}]
    });
    let mut batch_details = Vec::new();
    let mut events = vec![envelope(
        "run_export_media",
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "export-media".into(),
            workspace_root: "/tmp/workspace".to_string(),
        }),
    )];
    for (index, payload) in [
        serde_json::json!({"tool": "media", "result": {"content": content, "structuredContent": application}}),
        serde_json::json!({"uri": "fixture://media", "contents": [
            {"uri": "fixture://text", "text": "neighboring text", "metadata": application},
            content[3]["resource"]
        ]}),
        serde_json::json!({"name": "media", "messages": [{"role": "user", "content": content}]}),
        serde_json::json!({"name": "media", "messages": [{"role": "user", "content": content[2]}]}),
    ].into_iter().enumerate() {
        // Legacy fallback rendering copied encoded content into summary fields.
        let summary = format!("neighboring text: {payload}");
        let mut output = serde_json::json!({
            "server": {"id": "fixture", "transport": "stdio"},
            "protocolVersion": "2025-06-18",
            "payload": payload,
        });
        let batch_result = serde_json::json!({
            "success": true, "status": "succeeded", "summary": summary,
            "structured_output": output, "artifacts": [],
        });
        batch_details.push(serde_json::json!({
            "index": index, "tool_id": "mcp.fixture.media",
            "success": true, "status": "succeeded", "summary": summary,
            "structured_output": output, "result": batch_result,
        }));
        output["_harness"] = serde_json::json!({"output_summary": summary});
        events.push(envelope("run_export_media", u64::try_from(events.len() + 1).unwrap_or_abort(),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: format!("task_{index}").into(),
                result_summary: summary.clone(),
                result_digest: "legacy-digest".to_string(),
                metadata: None,
            })));
        events.push(envelope("run_export_media", u64::try_from(events.len() + 1).unwrap_or_abort(),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: format!("toolcall_{index:06}").into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(summary.clone()),
                output_digest: Some("legacy-digest".to_string()),
                output_json: Some(output),
                metadata: None,
            })));
    }
    batch_details.push(serde_json::json!({
        "tool_id": "ordinary", "summary": "ordinary summary", "structured_output": application,
    }));
    events.push(envelope(
        "run_export_media",
        u64::try_from(events.len() + 1).unwrap_or_abort(),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "toolcall_999998".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("All tools executed successfully.".to_string()),
            output_digest: None,
            output_json: Some(serde_json::json!({"details": batch_details})),
            metadata: Some(ToolCallMetadata { canonical_tool_id: Some("batch".to_string()), ..Default::default() }),
        }),
    ));
    // An application object using the same field names is not an MCP envelope.
    events.push(envelope(
        "run_export_media",
        u64::try_from(events.len() + 1).unwrap_or_abort(),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "toolcall_999999".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: None,
            output_digest: None,
            output_json: Some(application.clone()),
            metadata: None,
        }),
    ));
    events.push(envelope(
        "run_export_media",
        u64::try_from(events.len() + 1).unwrap_or_abort(),
        EventV1::RunFinished(RunFinishedEvent { summary: "done".to_string() }),
    ));
    write_events_jsonl(&run_dir, &events);
    let journal_path = run_dir.join("events.jsonl");
    let original = std::fs::read(&journal_path).unwrap_or_abort();
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "export",
        "run_export_media",
    ]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(&journal_path).unwrap_or_abort(), original);
    let export_text = String::from_utf8(output.stdout).unwrap_or_abort();
    for encoded in [
        "aW1hZ2UtcHJpdmF0ZQ==",
        "YXVkaW8tcHJpdmF0ZQ==",
        "cmVzb3VyY2UtcHJpdmF0ZQ==",
    ] {
        assert!(
            !export_text.contains(encoded),
            "legacy media leaked into support export"
        );
    }
    assert!(export_text.contains("media omitted"));
    for ordinary in [
        "neighboring text",
        "application-data",
        "application-blob",
        "application-image-data",
        "image/png",
        "audio/wav",
        "application/octet-stream",
    ] {
        assert!(export_text.contains(ordinary));
    }
    let bundle: serde_json::Value = serde_json::from_str(&export_text).unwrap_or_abort();
    assert_eq!(
        bundle["events"][2]["payload"]["data"]["output_json"]["payload"]["result"]["structuredContent"],
        application
    );
    assert_eq!(
        bundle["events"][10]["payload"]["data"]["output_json"],
        application
    );
    let details = &bundle["events"][9]["payload"]["data"]["output_json"]["details"];
    assert_eq!(details[0]["structured_output"]["payload"]["result"]["structuredContent"], application);
    assert_eq!(details[0]["result"]["structured_output"]["payload"]["result"]["structuredContent"], application);
    assert_eq!(details[4]["structured_output"], application);
    assert_eq!(
        bundle["support"]["secret_scan_status"]["secret_finding_count"],
        0
    );
}

#[test]
fn sessions_export_cli_redacts_secret_payloads_and_reports_manifest() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_export_redaction");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_redaction",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "export-redaction".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_export_redaction",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".into(),
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
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "export",
        "run_export_redaction",
        "--output",
        export_path.to_str().unwrap_or_abort(),
    ]);
    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let export_text = std::fs::read_to_string(&export_path).unwrap_or_abort();
    assert!(!export_text.contains("sk-AbCdEf0123456789"));
    assert!(!export_text.contains("Bearer abc.def-ghi_123"));
    assert!(export_text.contains("[REDACTED_API_KEY]"));
    assert!(export_text.contains("Bearer [REDACTED]"));

    let bundle: serde_json::Value =
        serde_json::from_str(&export_text).unwrap_or_abort();
    assert_eq!(bundle["support"]["redaction_manifest"]["status"], "clean");
    assert_eq!(
        bundle["support"]["secret_scan_status"]["secret_finding_count"],
        0
    );
}

#[test]
fn sessions_export_cli_support_includes_artifact_index() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_export_artifacts");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &delegated_recovery_events("run_export_artifacts"),
    );

    let export_path = session_dir.path().join("session-export-artifacts.json");
    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "export",
        "run_export_artifacts",
        "--output",
        export_path.to_str().unwrap_or_abort(),
    ]);
    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).unwrap_or_abort(),
    )
    .unwrap_or_abort();
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
