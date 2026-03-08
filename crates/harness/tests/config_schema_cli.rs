use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn schema_cli_prints_json_schema() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("schema")
        .output()
        .expect("run harness schema");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.contains("\"type\": \"object\""));
    assert!(body.contains("backgroundTask") || body.contains("providers"));
}

#[test]
fn config_validate_cli_reports_missing_config() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate without config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
}

#[test]
fn config_validate_cli_accepts_valid_config_and_session_override() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.test.jsonc");
    let session_dir = temp.path().join("override-sessions");
    let config = serde_json::json!({
        "backgroundTask": {
            "defaultConcurrency": 2,
            "providerConcurrency": 2,
            "modelConcurrency": 2,
            "staleTimeoutMs": 30000,
            "messageStalenessTimeoutMs": 10000
        },
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000
            }
        },
        "categories": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            }
        },
        "permissions": {
            "edit": "allow",
            "shell": "allow",
            "network": "allow"
        },
        "paths": {
            "session_dir": temp.path().join("sessions")
        }
    })
    .to_string();
    fs::write(&config_path, config).expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with valid config");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(config_path.to_str().expect("config path utf-8")));
}
