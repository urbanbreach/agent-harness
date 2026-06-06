#[test]
fn doctor_cli_fails_invalid_category_routes_even_when_some_are_missing() {
    // arrange
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
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
            "visual-engineering": { hidden: true },
            artistry: { enable: false },
          },
          permission: "ask",
        }
        "#,
    )
    .expect("write invalid category route config");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
        ])
        .output()
        // act
        .expect("run harness doctor with invalid category routes");

    // assert
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor found issues:"));
    assert!(stdout.contains("[FAIL] category_routes"));
    assert!(stdout.contains("visual-engineering"));
    assert!(stdout.contains("artistry"));
}
#[test]
fn doctor_cli_reports_model_profile_fallback_targets() {
    // arrange
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
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
            build: { enable: true, model: "fast" },
          },
          permission: "ask",
        }
        "#,
    )
    .expect("write config with model profile fallback");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        // act
        .expect("run harness doctor with model profile fallback");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let model_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "model_references")
        .expect("model references check");
    assert_eq!(model_check["status"], "pass");
    let message = model_check["message"]
        .as_str()
        .expect("model check message");
    assert!(message.contains("1 model profile(s) resolve"));
    assert!(message.contains("fallback target(s)"));
}
#[test]
fn doctor_cli_warns_when_provider_credentials_are_missing() {
    // arrange
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
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
    .expect("write config without credentials");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
        ])
        .output()
        // act
        .expect("run harness doctor with missing credentials");

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
            let temp = tempdir().expect("tempdir");
            fs::create_dir_all(temp.path().join(".agent-harness")).expect("create session parent");
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
            .expect("write env credential config");

            let output = command
                .current_dir(temp.path())
                .args([
                    "--config",
                    config_path.to_str().expect("config path utf-8"),
                    "doctor",
                    "--json",
                ])
                .output()
                // act
                .expect("run harness doctor with env credentials");

            // assert
            assert!(
                output.status.success(),
                "stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("super-secret-test-key"));
            let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
            let credential_check = report["checks"]
                .as_array()
                .expect("checks array")
                .iter()
                .find(|check| check["name"] == "provider_credentials")
                .expect("provider credential check");
            assert_eq!(credential_check["status"], "pass");
            assert!(credential_check["message"]
                .as_str()
                .expect("credential message")
                .contains("1 via environment"));
        },
    );
}
