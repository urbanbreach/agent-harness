#[cfg(target_os = "linux")]
#[test]
fn sessions_inspect_surfaces_recovery_message_for_previous_crash() {
    // arrange
    // act
    // assert
    // Given: interactive session with a stale writer lock (previous crash)
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_crash_inspect");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_crash_inspect",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_crash_inspect",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_root".to_string(),
                    profile: "build".to_string(),
                    parent_agent_id: None,
                }),
            ),
            agent_envelope(
                "run_crash_inspect",
                3,
                "agent_root",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".into(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_crash_inspect",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    std::fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();

    // When: human + json inspect
    let human = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "inspect",
        "--run",
        "run_crash_inspect",
    ]);
    let json = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "inspect",
        "--run",
        "run_crash_inspect",
        "--json",
    ]);

    // Then: recovery_message is operator-visible, not only a boolean field
    assert!(
        human.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("previous_crash_detected: true"));
    assert!(stdout.contains("recovery_message: Previous crash detected"));
    assert!(stdout.contains("recovery_action:"));

    assert!(
        json.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap_or_abort();
    assert_eq!(body["previous_crash_detected"], true);
    let message = body["recovery_message"].as_str().unwrap_or_abort();
    assert!(message.contains("Previous crash detected"));
    assert!(message.contains("sessions inspect"));
}

#[test]
fn sessions_inspect_omits_recovery_message_without_crash() {
    // arrange
    // act
    // assert
    // Given: clean finished session
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_clean_inspect");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_clean_inspect",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_clean_inspect",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    // When
    let human = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "inspect",
        "--run",
        "run_clean_inspect",
    ]);
    let json = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "inspect",
        "--run",
        "run_clean_inspect",
        "--json",
    ]);

    // Then
    assert!(human.status.success());
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("previous_crash_detected: false"));
    assert!(!stdout.contains("recovery_message:"));

    assert!(json.status.success());
    let body: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap_or_abort();
    assert_eq!(body["previous_crash_detected"], false);
    assert!(body.get("recovery_message").is_none() || body["recovery_message"].is_null());
}

#[test]
fn sessions_import_events_jsonl_creates_replay_session() {
    // arrange
    // act
    // assert
    // Given: foreign events.jsonl with harness-compatible envelopes
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-session");
    let session_dir = root.path().join("sessions");
    std::fs::create_dir_all(&foreign).unwrap_or_abort();
    write_events_jsonl(
        &foreign,
        &[
            envelope(
                "foreign-run",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/ws".to_string(),
                }),
            ),
            envelope(
                "foreign-run",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let source_before = std::fs::read(foreign.join("events.jsonl")).unwrap_or_abort();

    // When
    let output = run_harness([
        "--session-dir",
        session_dir.to_str().unwrap_or_abort(),
        "sessions",
        "import",
        "--from",
        foreign.to_str().unwrap_or_abort(),
        "--json",
    ]);

    // Then
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(body["event_count"], 2);
    assert_eq!(body["format"], "events_jsonl_v1");
    assert_eq!(body["mode_source"], "replay_only");
    let run_id = body["run_id"].as_str().unwrap_or_abort();
    let run_dir = session_dir.join(run_id);
    assert!(run_dir.join("events.jsonl").is_file());
    assert!(run_dir.join("meta.json").is_file());
    assert_eq!(
        std::fs::read(foreign.join("events.jsonl")).unwrap_or_abort(),
        source_before
    );
}

#[test]
fn sessions_import_unknown_format_fails_closed() {
    // arrange
    // act
    // assert
    // Given: foreign session with session.json only
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("codex-session");
    let session_dir = root.path().join("sessions");
    std::fs::create_dir_all(&foreign).unwrap_or_abort();
    std::fs::write(foreign.join("session.json"), r#"{"id":"x"}"#).unwrap_or_abort();

    // When
    let output = run_harness([
        "--session-dir",
        session_dir.to_str().unwrap_or_abort(),
        "sessions",
        "import",
        "--from",
        foreign.to_str().unwrap_or_abort(),
    ]);

    // Then
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("session import failed"));
    assert!(stderr.contains("unsupported") || stderr.contains("not importable"));
}
