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

#[test]
fn auth_login_list_and_logout_run_outside_onboarding_without_printing_secrets() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let data_home = temp.path().join("data");
    let config_path = temp.path().join("harness.jsonc");
    let config_body = r#"
        {
          provider: {
            codex_route: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              authProvider: "codex",
              apiKeyEnv: ["HARNESS_AUTH_FALLBACK_KEY"],
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "codex_route/gpt-5.4-mini",
          permission: "ask",
        }
        "#;
    fs::write(&config_path, config_body).expect("write auth config");

    // act: API-key login stores a credential without editing config or echoing the key.
    let api_secret = "sk-auth-cli-secret-value";
    let api_login = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .args([
            "auth",
            "login",
            "codex",
            "--method",
            "api-key",
            "--api-key-stdin",
        ])
        .stdin(format!("{api_secret}\n"))
        .output()
        .expect("run auth api-key login");

    // assert
    assert!(
        api_login.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&api_login.stdout),
        String::from_utf8_lossy(&api_login.stderr)
    );
    assert!(!String::from_utf8_lossy(&api_login.stdout).contains(api_secret));
    assert!(!String::from_utf8_lossy(&api_login.stderr).contains(api_secret));
    let credential_path = data_home.join("harness/credentials/codex.json");
    assert!(
        credential_path.is_file(),
        "credential should be stored outside config"
    );
    assert_eq!(
        fs::read_to_string(&config_path).expect("read config after login"),
        config_body,
        "auth login must not edit harness config"
    );

    // act: mocked OAuth login replaces the active stored credential and list redacts metadata.
    let oauth_secret = "oauth-access-secret-value";
    let refresh_secret = "oauth-refresh-secret-value";
    let account_id = "acct-secret-value";
    let mock_login = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .args([
            "auth",
            "login",
            "codex",
            "--mock-token",
            oauth_secret,
            "--mock-refresh-token",
            refresh_secret,
            "--expires-at",
            "2099-01-02T03:04:05Z",
            "--account-id",
            account_id,
        ])
        .output()
        .expect("run mocked OAuth login");
    assert!(mock_login.status.success());
    let mock_stdout = String::from_utf8_lossy(&mock_login.stdout);
    let mock_stderr = String::from_utf8_lossy(&mock_login.stderr);
    for secret in [oauth_secret, refresh_secret, account_id] {
        assert!(!mock_stdout.contains(secret), "stdout leaked {secret}");
        assert!(!mock_stderr.contains(secret), "stderr leaked {secret}");
    }

    let list_output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "auth",
            "list",
            "--json",
        ])
        .output()
        .expect("run auth list");
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    for secret in [oauth_secret, refresh_secret, account_id, api_secret] {
        assert!(!list_stdout.contains(secret), "auth list leaked {secret}");
    }
    let list: Value = serde_json::from_slice(&list_output.stdout).expect("auth list json");
    let codex = list
        .as_array()
        .expect("auth list array")
        .iter()
        .find(|status| status["auth_provider"] == "codex")
        .expect("codex status");
    assert_eq!(codex["provider_ids"], serde_json::json!(["codex_route"]));
    assert_eq!(codex["presence"], "stored");
    assert_eq!(codex["source"], "stored_oauth");
    assert_eq!(codex["kind"], "oauth");
    assert_eq!(codex["expires_at"], "2099-01-02T03:04:05Z");
    assert_eq!(codex["account_id"], "<redacted>");
    assert_eq!(codex["usable_without_network_probe"], true);

    // act/assert: an explicitly empty Copilot Enterprise URL is invalid and must
    // not fall back to a public Copilot credential.
    let copilot_secret = "copilot-empty-enterprise-secret-value";
    let empty_enterprise_login = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .args([
            "auth",
            "login",
            "github-copilot",
            "--mock-token",
            copilot_secret,
            "--enterprise-url",
            "",
        ])
        .output()
        .expect("run mocked Copilot login with empty enterprise url");
    assert!(!empty_enterprise_login.status.success());
    let empty_enterprise_stdout = String::from_utf8_lossy(&empty_enterprise_login.stdout);
    let empty_enterprise_stderr = String::from_utf8_lossy(&empty_enterprise_login.stderr);
    assert!(empty_enterprise_stderr.contains("domain is required"));
    assert!(!empty_enterprise_stdout.contains(copilot_secret));
    assert!(!empty_enterprise_stderr.contains(copilot_secret));
    assert!(
        !data_home
            .join("harness/credentials/github-copilot.json")
            .exists(),
        "empty Enterprise URL must not store a public Copilot credential"
    );

    // act: logout deletes only the stored credential, preserving env/config fallbacks.
    let logout = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .env("HARNESS_AUTH_FALLBACK_KEY", "fallback-secret-value")
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "auth",
            "logout",
            "codex",
        ])
        .output()
        .expect("run auth logout");
    assert!(logout.status.success());
    assert!(
        !credential_path.exists(),
        "auth logout should delete only the stored credential"
    );
    assert_eq!(
        fs::read_to_string(&config_path).expect("read config after logout"),
        config_body,
        "auth logout must not edit harness config"
    );

    let fallback_list = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .env("HARNESS_AUTH_FALLBACK_KEY", "fallback-secret-value")
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "auth",
            "list",
            "--json",
        ])
        .output()
        .expect("run auth list after logout");
    assert!(fallback_list.status.success());
    let fallback_stdout = String::from_utf8_lossy(&fallback_list.stdout);
    assert!(!fallback_stdout.contains("fallback-secret-value"));
    let fallback_list: Value =
        serde_json::from_slice(&fallback_list.stdout).expect("fallback auth list json");
    let codex = fallback_list
        .as_array()
        .expect("fallback auth list array")
        .iter()
        .find(|status| status["auth_provider"] == "codex")
        .expect("codex fallback status");
    assert_eq!(codex["presence"], "env");
    assert_eq!(codex["source"], "apiKeyEnv");
    assert_eq!(codex["env_fallback_configured"], true);
    assert_eq!(codex["usable_without_network_probe"], true);
}

#[test]
fn auth_list_and_doctor_report_malformed_stored_credentials_as_error() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let data_home = temp.path().join("data");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            codex_route: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              authProvider: "codex",
              apiKeyEnv: ["HARNESS_AUTH_FALLBACK_KEY"],
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "codex_route/gpt-5.4-mini",
          permission: "ask",
        }
        "#,
    )
    .expect("write auth config");
    let credential_dir = data_home.join("harness/credentials");
    fs::create_dir_all(&credential_dir).expect("create credential dir");
    let corrupt_secret = "corrupt-credential-secret-value";
    fs::write(
        credential_dir.join("codex.json"),
        format!("{{ not valid json: \"{corrupt_secret}\""),
    )
    .expect("write malformed credential");

    // act: auth list reports the store error instead of falling back silently.
    let list_output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .env("HARNESS_AUTH_FALLBACK_KEY", "fallback-secret-value")
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "auth",
            "list",
            "--json",
        ])
        .output()
        .expect("run auth list with malformed credential");

    // assert
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(!list_stdout.contains(corrupt_secret));
    assert!(!list_stdout.contains("fallback-secret-value"));
    let list: Value = serde_json::from_slice(&list_output.stdout).expect("auth list json");
    let codex = list
        .as_array()
        .expect("auth list array")
        .iter()
        .find(|status| status["auth_provider"] == "codex")
        .expect("codex status");
    assert_eq!(codex["presence"], "error");
    assert_eq!(codex["source"], "credential_store_error");
    assert_eq!(codex["env_fallback_configured"], true);
    assert_eq!(codex["usable_without_network_probe"], false);

    // act: doctor surfaces the same error as a warning, not a healthy fallback.
    let doctor_output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .env("HARNESS_AUTH_FALLBACK_KEY", "fallback-secret-value")
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run doctor with malformed credential");

    // assert
    assert!(
        doctor_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&doctor_output.stdout),
        String::from_utf8_lossy(&doctor_output.stderr)
    );
    let doctor_stdout = String::from_utf8_lossy(&doctor_output.stdout);
    assert!(!doctor_stdout.contains(corrupt_secret));
    assert!(!doctor_stdout.contains("fallback-secret-value"));
    let report: Value = serde_json::from_slice(&doctor_output.stdout).expect("doctor json report");
    let credential_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "provider_credentials")
        .expect("provider credential check");
    assert_eq!(credential_check["status"], "warn");
    assert!(credential_check["message"]
        .as_str()
        .expect("message")
        .contains("unreadable stored credentials"));
    let auth = credential_check["details"]["auth"]
        .as_array()
        .expect("auth status array")
        .iter()
        .find(|status| status["auth_provider"] == "codex")
        .expect("codex auth detail");
    assert_eq!(auth["presence"], "error");
    assert_eq!(auth["source"], "credential_store_error");
    assert_eq!(auth["usable_without_network_probe"], false);
}

#[test]
fn doctor_cli_json_reports_redacted_per_provider_auth_status() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let data_home = temp.path().join("data");
    let config_path = temp.path().join("harness.jsonc");
    fs::write(
        &config_path,
        r#"
        {
          provider: {
            codex_route: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              authProvider: "codex",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "codex_route/gpt-5.4-mini",
          permission: "ask",
        }
        "#,
    )
    .expect("write auth doctor config");
    let secret = "doctor-oauth-secret-value";
    let account_id = "acct-doctor-secret";
    let login = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .args([
            "auth",
            "login",
            "codex",
            "--mock-token",
            secret,
            "--account-id",
            account_id,
            "--expires-at",
            "2099-05-06T07:08:09Z",
        ])
        .output()
        .expect("seed stored auth credential");
    assert!(login.status.success());

    // act
    let output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_DATA_HOME", data_home.as_os_str())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run doctor with stored auth");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(secret));
    assert!(!stdout.contains(account_id));
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let credential_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "provider_credentials")
        .expect("provider credential check");
    assert_eq!(credential_check["status"], "pass");
    assert_eq!(credential_check["details"]["redacted"], true);
    assert_eq!(credential_check["details"]["no_network_probes"], true);
    let auth = credential_check["details"]["auth"]
        .as_array()
        .expect("auth status array")
        .iter()
        .find(|status| status["auth_provider"] == "codex")
        .expect("codex auth detail");
    assert_eq!(auth["presence"], "stored");
    assert_eq!(auth["source"], "stored_oauth");
    assert_eq!(auth["kind"], "oauth");
    assert_eq!(auth["expires_at"], "2099-05-06T07:08:09Z");
    assert_eq!(auth["account_id"], "<redacted>");
}
#[test]
fn config_validate_cli_accepts_provider_catalog_reference_config_by_explicit_path() {
    // arrange
    let repo_root = repo_root();
    let config_path = repo_root
        .join("configs")
        .join("provider-catalog.reference.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        // act
        .expect("run harness config validate with reference catalog config");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains("configs/provider-catalog.reference.jsonc"));

    let parsed = load_config_from_file(&config_path).expect("reference catalog should parse");
    assert_eq!(parsed.providers.len(), 1);
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("openai-codex")
        .expect("openai-codex provider present in reference catalog");
    assert!(provider.models.len() > 1);
}
#[test]
fn config_validate_cli_does_not_auto_discover_provider_catalog_reference_config() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let configs_dir = temp.path().join("configs");
    fs::create_dir_all(&configs_dir).expect("create configs dir");
    fs::copy(
        repo_root()
            .join("configs")
            .join("provider-catalog.reference.jsonc"),
        configs_dir.join("provider-catalog.reference.jsonc"),
    )
    .expect("copy reference catalog fixture");

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        // act
        .expect("run harness config validate with only reference catalog present");

    // assert
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(!stderr.contains("provider-catalog.reference.jsonc"));
}
#[test]
fn config_validate_cli_merges_xdg_defaults_with_local_project_override() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");
    let local_config_path = temp.path().join("harness.json");

    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    write_config(&xdg_config_path, &canonical_runtime_config());
    write_config(
        &local_config_path,
        &serde_json::json!({
            "default_agent": "plan"
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        // act
        .expect("run harness config validate with merged discovery");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(xdg_config_path.to_str().expect("xdg path utf-8")));
    assert!(stdout.contains("harness.json"));
}
#[test]
fn load_config_allows_public_agents_without_explicit_description() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["agent"] = serde_json::json!({
        "plan": {
            "use_small_model": true,
            "tools": []
        }
    });
    config["default_agent"] = serde_json::json!("plan");
    write_config(&config_path, &config);

    let parsed = load_config_from_file(&config_path)
        .expect("public agent without explicit description should still load");
    let plan = parsed
        .agents
        .get("plan")
        // act
        .expect("plan profile should be translated from public config");

    // assert
    assert_eq!(
        plan.description,
        "Plan mode. Disallows all edit tools except the active plan file."
    );
    assert_eq!(plan.model_ref, "default/gpt-4o-mini");
}
#[test]
fn config_validate_cli_accepts_legacy_harness_native_shape() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    write_config(
        &config_path,
        &legacy_runtime_config(&temp.path().join("sessions")),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        // act
        .expect("run harness config validate with legacy config");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn config_validate_cli_accepts_legacy_xdg_config_path_for_migration() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let legacy_xdg_config = xdg_root.join("harness/config.jsonc");
    fs::create_dir_all(legacy_xdg_config.parent().expect("legacy xdg parent"))
        .expect("create legacy xdg dir");
    write_config(&legacy_xdg_config, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        // act
        .expect("run harness config validate with legacy xdg path");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config.jsonc"));
}
