use harness::UnwrapOrAbort;

#[test]
fn doctor_cli_redacts_provider_credentials_in_output() {
    // arrange — a distinctive secret supplied through the environment
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");
    let secret = "doctor-redaction-probe-sk-9f8e7d6c5b4a";

    // act
    let output = harness_command()
        .current_dir(&repo_root)
        .env("OPENAI_API_KEY", secret)
        .args(["--config", config_path.to_str().unwrap_or_abort(), "doctor"])
        .output()
        .unwrap_or_abort();

    // assert — doctor succeeds and never leaks secret bytes on either stream
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains(secret), "doctor stdout leaked the secret");
    assert!(!stderr.contains(secret), "doctor stderr leaked the secret");
    assert!(stdout.contains("provider_credentials"));
}
