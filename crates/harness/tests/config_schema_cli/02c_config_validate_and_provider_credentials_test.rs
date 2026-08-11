use harness::UnwrapOrAbort;
#[test]
fn config_validate_rejects_role_shaped_agent_entries() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    fs::create_dir_all(temp.path().join(".agent-harness")).unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "DUMMY",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "default/gpt-5.4-mini",
          agent: {
            "visual-engineering": {},
          },
          permission: "ask",
        }
        "#,
    )
    .unwrap_or_abort();

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
        ])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("visual-engineering"));
    assert!(stderr.contains("unknown field"));
}
#[test]
fn doctor_cli_reports_model_profile_fallback_targets() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    fs::create_dir_all(temp.path().join(".agent-harness")).unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              apiKey: "DUMMY",
              models: {
                "gpt-5.5": { name: "GPT-5.5" },
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model_profile: {
            fast: {
              model: "default/gpt-5.5",
              fallback: [{ model: "default/gpt-5.4-mini" }],
            },
          },
          model: "fast",
          agent: {
            default: { model: "fast" },
          },
          permission: "ask",
        }
        "#,
    )
    .unwrap_or_abort();

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let model_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "model_references")
        .unwrap_or_abort();
    assert_eq!(model_check["status"], "pass");
    let message = model_check["message"]
        .as_str()
        .unwrap_or_abort();
    assert!(message.contains("1 model profile(s) resolve"));
    assert!(message.contains("fallback target(s)"));
}
#[test]
fn doctor_cli_warns_when_provider_credentials_are_missing() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    fs::create_dir_all(temp.path().join(".agent-harness")).unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "default/gpt-5.4-mini",
          permission: "ask",
        }
        "#,
    )
    .unwrap_or_abort();

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
        ])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor ok with warnings:"));
    assert!(stdout.contains("[WARN] provider_credentials"));
    assert!(stdout.contains("default (set apiKey or apiKeyEnv)"));
}
#[test]
fn doctor_cli_reports_env_provider_credentials_without_revealing_values() {
    // arrange
    with_env_var_state(
        "HARNESS_DOCTOR_TEST_API_KEY",
        Some("super-secret-test-key"),
        |command| {
            let temp = tempdir().unwrap_or_abort();
            fs::create_dir_all(temp.path().join(".agent-harness")).unwrap_or_abort();
            let config_path = temp.path().join("harness.jsonc");
            fs::write(
                &config_path,
                r#"
            {
              provider: {
                default: {
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  apiKeyEnv: ["HARNESS_DOCTOR_TEST_API_KEY"],
                  models: {
                    "gpt-5.4-mini": { name: "GPT-5.4 mini" },
                  },
                },
              },
              model: "default/gpt-5.4-mini",
              permission: "ask",
            }
            "#,
            )
            .unwrap_or_abort();

            let output = command
                .current_dir(temp.path())
                .args([
                    "--config",
                    config_path.to_str().unwrap_or_abort(),
                    "doctor",
                    "--json",
                ])
                .output()
                // act
                .unwrap_or_abort();

            // assert
            assert!(
                output.status.success(),
                "stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("super-secret-test-key"));
            let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
            let credential_check = report["checks"]
                .as_array()
                .unwrap_or_abort()
                .iter()
                .find(|check| check["name"] == "provider_credentials")
                .unwrap_or_abort();
            assert_eq!(credential_check["status"], "pass");
            assert!(credential_check["message"]
                .as_str()
                .unwrap_or_abort()
                .contains("1 via environment"));
        },
    );
}
