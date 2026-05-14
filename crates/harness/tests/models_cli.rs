use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn execute_models_command_returns_error_when_config_is_missing() {
    let temp = tempdir().expect("failed to create temp dir");
    let missing_path = temp.path().join("missing.jsonc");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--config")
        .arg(&missing_path)
        .arg("models")
        .env_remove("HARNESS_CONFIG_CONTENT")
        .output()
        .expect("Failed to execute harness command");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn execute_models_command_returns_success_with_valid_config() {
    let temp = tempdir().expect("failed to create temp dir");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"{
            "providers": {
                "test_provider": {
                    "type": "openai_compatible",
                    "options": { "baseURL": "http://localhost" },
                    "models": {
                        "test_model": {
                            "name": "Test Model",
                            "limit": {
                                "context": 8192
                            },
                            "metadata": {
                                "supportsToolCalls": true
                            },
                            "modalities": {
                                "input": ["text"],
                                "output": ["text"]
                            }
                        }
                    }
                }
            },
            "model_profiles": {
                "default": {
                    "model": "test_provider:test_model"
                }
            },
            "agents": {
                "build": {
                    "model_ref": "default",
                    "description": "test agent",
                    "system_prompt": "test prompt"
                }
            }
        }"#
    ).expect("failed to write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--config")
        .arg(&config_path)
        .arg("models")
        .env_remove("HARNESS_CONFIG_CONTENT")
        .output()
        .expect("Failed to execute harness command");

    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test_provider:test_model"));
}
