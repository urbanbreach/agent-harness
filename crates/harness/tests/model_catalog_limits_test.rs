#[path = "common/mod.rs"]
mod common;

use std::fs;

use common::CliHarness;
use harness::UnwrapOrAbort;
use tempfile::tempdir;

fn config(model: &str) -> String {
    format!(
        r#"{{
          provider: {{
            local: {{
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:1/v1",
              apiKey: "test-key",
              models: {{ {model} }},
            }},
          }},
          model: "local/custom",
          agent: {{ default: {{ model: "local/custom" }} }},
          permission: "deny",
        }}"#
    )
}

#[test]
fn models_cli_prints_exact_limits_and_provenance_for_known_model() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("harness.jsonc");
    fs::write(
        &path,
        config(
            r#"custom: {
              name: "Custom",
              limit: { context: 128000, input: 96000, output: 16000 },
            }"#,
        ),
    )
    .unwrap_or_abort();

    // act
    let output = CliHarness::new()
        .args(["--config", path.to_str().unwrap_or_abort(), "models"])
        .env("HARNESS_DISABLE_MODELS_FETCH", "1")
        .output();

    // assert
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).unwrap_or_abort();
    assert!(stdout.contains("context=128000"), "{stdout}");
    assert!(stdout.contains("max_input=96000"), "{stdout}");
    assert!(stdout.contains("max_output=16000"), "{stdout}");
    assert!(
        stdout.contains("context_provenance=explicit_config"),
        "{stdout}"
    );
    assert!(
        stdout.contains("max_input_provenance=explicit_config"),
        "{stdout}"
    );
    assert!(
        stdout.contains("max_output_provenance=explicit_config"),
        "{stdout}"
    );
}

#[test]
fn models_cli_surfaces_unknown_custom_limits_without_fabricated_window() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("harness.jsonc");
    fs::write(&path, config(r#"custom: { name: "Custom" }"#)).unwrap_or_abort();

    // act
    let output = CliHarness::new()
        .args(["--config", path.to_str().unwrap_or_abort(), "models"])
        .env("HARNESS_DISABLE_MODELS_FETCH", "1")
        .output();

    // assert
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).unwrap_or_abort();
    let custom = stdout
        .lines()
        .find(|line| line.starts_with("local:custom "))
        .unwrap_or_abort();
    assert!(custom.contains("context=unknown"), "{custom}");
    assert!(custom.contains("context_provenance=unknown"), "{custom}");
    assert!(custom.contains("max_input=unknown"), "{custom}");
    assert!(custom.contains("max_input_provenance=unknown"), "{custom}");
    assert!(custom.contains("max_output=unknown"), "{custom}");
    assert!(custom.contains("max_output_provenance=unknown"), "{custom}");
    assert!(!custom.contains("128000"), "{custom}");
    assert!(!custom.contains("200000"), "{custom}");
    assert!(!custom.contains('%'), "{custom}");
}
