use harness::UnwrapOrAbort;
#[test]
fn doctor_cli_json_reports_formatter_status() {
    // arrange
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    // act
    let output = harness_command()
        .current_dir(&repo_root)
        .env("OPENAI_API_KEY", "doctor-formatter-status-test-key")
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let formatters_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "formatters")
        .unwrap_or_abort();

    assert!(
        formatters_check["status"] == "pass" || formatters_check["status"] == "warn",
        "formatters check should be best-effort non-fatal"
    );
    assert_eq!(formatters_check["details"]["no_network_probes"], true);

    let entries = formatters_check["details"]["formatters"]
        .as_array()
        .unwrap_or_abort();
    assert!(
        entries.iter().any(|entry| entry["name"] == "rustfmt"),
        "rustfmt status entry should be present"
    );
    assert!(
        entries.iter().any(|entry| entry["name"] == "uv"),
        "uv status entry should be present using the canonical key"
    );
    assert!(
        !entries.iter().any(|entry| entry["name"] == "uvformat"),
        "legacy uvformat key should not appear as a formatter name"
    );

    for entry in entries {
        assert!(entry["name"].is_string());
        assert!(entry["extensions"].is_array());
        assert!(entry["enabled"].is_boolean());
    }
}
