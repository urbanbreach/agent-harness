use harness::UnwrapOrAbort;

#[test]
fn sessions_export_cli_fails_closed_for_missing_session_dir() {
    // arrange — a session directory that does not exist
    let run_dir = tempdir().unwrap_or_abort();
    let missing_session = run_dir.path().join("missing-session");
    let export_path = run_dir.path().join("missing-session-export.json");

    // act
    let output = run_harness([
        "--session-dir",
        missing_session.to_str().unwrap_or_abort(),
        "sessions",
        "export",
        "missing-session",
        "--output",
        export_path.to_str().unwrap_or_abort(),
    ]);

    // assert — fail-closed: non-zero exit, structured error, no bundle fabricated
    assert_eq!(output.status.code(), 1);
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to read session directory"),
        "stderr:\n{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "stderr:\n{stderr}");
    assert!(!export_path.exists(), "missing session fabricated an export");
}
