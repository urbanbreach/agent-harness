use harness::UnwrapOrAbort;
#[test]
fn session_history_entries_sort_by_recency() {
    let session_dir = tempdir().unwrap_or_abort();
    let older_dir = session_dir.path().join("alpha_session");
    std::fs::create_dir_all(&older_dir).unwrap_or_abort();
    let older_events = [
        envelope(
            "run_older",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "older-run".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_older",
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&older_dir, &older_events);
    let older_sort_ms = events_modified_unix_ms(&older_dir);

    let newer_dir = session_dir.path().join("omega_session");
    std::fs::create_dir_all(&newer_dir).unwrap_or_abort();
    let newer_events = [
        envelope(
            "run_newer",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "newer-run".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_newer",
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&newer_dir, &newer_events);

    for _ in 0..1_000 {
        if events_modified_unix_ms(&newer_dir) > older_sort_ms {
            break;
        }
        std::thread::yield_now();
        write_events_jsonl(&newer_dir, &newer_events);
    }

    assert!(
        events_modified_unix_ms(&newer_dir) > older_sort_ms,
        "test fixture must encode millisecond-level recency independent of lexical directory names"
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let rows = stdout.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "expected exactly two session rows\n{stdout}");
    assert!(
        rows[0].contains("run_newer"),
        "expected newer session first\n{stdout}"
    );
    assert!(
        rows[1].contains("run_older"),
        "expected older session second\n{stdout}"
    );
}
#[test]
fn session_history_marks_corrupt_runs_without_hiding_them() {
    let session_dir = tempdir().unwrap_or_abort();
    let good_dir = session_dir.path().join("run_good");
    let corrupt_dir = session_dir.path().join("run_corrupt");
    std::fs::create_dir_all(&good_dir).unwrap_or_abort();
    std::fs::create_dir_all(&corrupt_dir).unwrap_or_abort();

    write_events_jsonl(
        &good_dir,
        &[
            envelope(
                "run_good",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_good",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    write_events_lines(&corrupt_dir, &["{this is not json"]);

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_good"));
    assert!(stdout.contains("run_corrupt"));
    assert!(stdout.contains("events unavailable:"));
    assert!(stdout.contains("<unavailable>"));
}
#[test]
fn session_history_excludes_scenario_fixture_runs_by_default() {
    let session_dir = tempdir().unwrap_or_abort();
    let interactive_dir = session_dir.path().join("interactive_run");
    let scenario_dir = session_dir.path().join("scenario_run");
    std::fs::create_dir_all(&interactive_dir).unwrap_or_abort();
    std::fs::create_dir_all(&scenario_dir).unwrap_or_abort();

    write_events_jsonl(
        &interactive_dir,
        &[
            envelope(
                "run_interactive",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_interactive",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    write_events_jsonl(
        &scenario_dir,
        &[
            envelope(
                "run_scenario",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "golden_path".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_scenario",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_interactive"));
    assert!(
        !stdout.contains("run_scenario"),
        "scenario fixture sessions must be excluded by default\n{stdout}"
    );
}
#[test]
fn session_history_exposes_profile_and_model_labels() {
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_profile_model");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_profile_model",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_profile_model",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_profile_model",
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_profile_model",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("worker"));
    assert!(stdout.contains("openai/gpt-5.4-mini"));
}
#[test]
fn session_history_flags_non_resumable_sessions_with_reason() {
    let session_dir = tempdir().unwrap_or_abort();
    let prompt_run_dir = session_dir.path().join("prompt_run");
    std::fs::create_dir_all(&prompt_run_dir).unwrap_or_abort();

    write_events_jsonl(
        &prompt_run_dir,
        &[
            envelope(
                "run_prompt",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_prompt",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_prompt"));
    assert!(stdout.contains("no"));
    assert!(stdout.contains("prompt runs are not resumable"));
}
#[test]
fn sessions_list_surfaces_artifact_and_lineage_columns() {
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_context");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    write_events_jsonl(&run_dir, &delegated_recovery_events("run_context"));

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("artifacts"));
    assert!(stdout.contains("children"));
    assert!(stdout.contains("parent"));
    assert!(stdout.contains("run_context"));
    assert!(stdout.contains("agent_supervisor"));
    assert!(stdout.contains("1"));
}
#[test]
fn sessions_help_lists_lifecycle_subcommands() {
    let output = run_harness(["sessions", "--help"]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("inspect"));
    assert!(stdout.contains("reopen"));
    assert!(stdout.contains("replay"));
    assert!(stdout.contains("continue"));
    assert!(stdout.contains("export"));
}
#[test]
fn sessions_lineage_help_is_harness_branded() {
    let forbidden_terms = forbidden_brand_terms();
    let sessions_help = run_harness_help(&["sessions", "--help"]);
    assert_harness_branded("harness sessions --help", &sessions_help, &forbidden_terms);

    for command in ["tree", "fork", "clone"] {
        if sessions_help.contains(command) {
            let help = run_harness_help(&["sessions", command, "--help"]);
            assert_harness_branded(
                &format!("harness sessions {command} --help"),
                &help,
                &forbidden_terms,
            );
        }
    }
}
