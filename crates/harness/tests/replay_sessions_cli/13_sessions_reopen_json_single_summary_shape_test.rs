use harness::UnwrapOrAbort;

#[test]
fn sessions_reopen_json_emits_single_summary_shape() {
    // arrange — a minimal completed run under a session dir
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_reopen_shape");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_reopen_shape",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "shape-fixture".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_reopen_shape",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "reopen",
        "--session",
        "run_reopen_shape",
        "--json",
    ]);

    // assert — exactly one canonical response object
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_abort();

    // top-level keys are `summary` only (crash_recovery omitted for a clean
    // run); no duplicated summary fields survive at the top level
    let top_keys = response.as_object().unwrap_or_abort();
    assert_eq!(
        top_keys.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["summary"],
        "reopen --json must nest the summary exactly once: {response}"
    );
    for duplicated in [
        "run_id",
        "total_events",
        "resumable",
        "previous_crash_detected",
        "artifacts",
        "prompt_context",
        "child_sessions",
    ] {
        assert!(
            top_keys.get(duplicated).is_none(),
            "legacy duplicated top-level field {duplicated} must be gone: {response}"
        );
    }
    assert_eq!(response["summary"]["run_id"], "run_reopen_shape");
    assert_eq!(response["summary"]["total_events"], 2);
    assert_eq!(response["summary"]["previous_crash_detected"], false);
}
