use harness::UnwrapOrAbort;

#[test]
fn replay_cli_fails_closed_for_missing_events_file() {
    // arrange — a session path that was never written
    let run_dir = tempdir().unwrap_or_abort();
    let missing_session = run_dir.path().join("missing-session");

    // act
    let output = run_harness([
        "replay",
        "--session",
        missing_session.to_str().unwrap_or_abort(),
        "--json",
    ]);

    // assert — fail-closed: non-zero exit, structured error, no summary fabricated
    assert!(
        !output.status.success(),
        "replay must fail for a missing session; stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("replay failed"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
