#[test]
fn doctor_cli_json_reports_formatter_status() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .env("OPENAI_API_KEY", "doctor-formatter-status-test-key")
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor --json for formatter status");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let formatters_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "formatters")
        .expect("formatters check");

    assert!(
        formatters_check["status"] == "pass" || formatters_check["status"] == "warn",
        "formatters check should be best-effort non-fatal"
    );
    assert_eq!(formatters_check["details"]["no_network_probes"], true);

    let entries = formatters_check["details"]["formatters"]
        .as_array()
        .expect("formatters details array");
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
