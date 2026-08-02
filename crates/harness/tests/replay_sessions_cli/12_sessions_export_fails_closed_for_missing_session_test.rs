use harness::UnwrapOrAbort;

#[test]
fn sessions_export_cli_fails_closed_for_missing_session_dir() {
    // arrange — a session directory that does not exist
    let run_dir = tempdir().unwrap_or_abort();
    let missing_session = run_dir.path().join("missing-session");

    // act
    let output = run_harness([
        "sessions",
        "export",
        "--session-dir",
        missing_session.to_str().unwrap_or_abort(),
        "missing-session",
    ]);

    // assert — fail-closed: non-zero exit, structured error, no bundle fabricated
    assert!(
        !output.status.success(),
        "export must fail for a missing session dir; stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to read session directory"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
