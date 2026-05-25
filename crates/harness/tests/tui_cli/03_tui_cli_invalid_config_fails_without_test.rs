#[test]
fn tui_cli_invalid_config_fails_without_mock_fallback() {
    let temp = tempdir().expect("tempdir");
    let missing_config = temp.path().join("does-not-exist.jsonc");
    let output = run_harness_in(temp.path(), [
            "--config",
            missing_config
                .to_str()
                .expect("missing config path should be valid utf-8"),
            "tui",
            "--exit-on-finish",
        ]);

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed:"),
        "expected setup failure prefix, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("golden_path") && !stderr.contains("scenario"),
        "invalid interactive config should fail before scenario/mock fallback, got:\n{stderr}"
    );
}
