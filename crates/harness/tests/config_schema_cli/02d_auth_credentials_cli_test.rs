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
