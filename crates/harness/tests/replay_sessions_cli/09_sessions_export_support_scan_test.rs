use harness::UnwrapOrAbort;
#[test]
fn sessions_export_cli_fails_closed_for_resolved_config_credentials_in_events() {
    // arrange
    let workspace = tempdir().unwrap_or_abort();
    let config_path = workspace.path().join("harness.jsonc");
    write_support_export_config(
        &config_path,
        "test",
        "plain-provider-secret-value",
        r#",
        "apiKeyEnv": ["MY_PROVIDER_KEY"],
        "headers": {
          "x-api-key": "plain-header-secret-value",
          "Authorization": "Custom plain-auth-secret-value"
        }"#,
    );

    let session_dir = workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_config_credentials");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_config_credentials",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "config-credentials".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_config_credentials",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000003".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "plain-provider-secret-value plain-env-config-secret-value plain-header-secret-value plain-auth-secret-value".to_string(),
                    ),
                    output_digest: Some("digest-config-secret".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                "run_export_config_credentials",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("session-export-config-credentials.json");

    // act
    let output = CliHarness::new()
        .current_dir(workspace.path())
        .env("MY_PROVIDER_KEY", "plain-env-config-secret-value")
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_export_config_credentials",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ])
        .output();

    // assert
    assert!(!output.status.success());
    assert!(!export_path.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("redaction scanner found"),
        "stderr should explain config credential fail-closed scan: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sessions_export_cli_uses_session_workspace_for_readiness_when_config_is_implicit() {
    // arrange
    let session_workspace = tempdir().unwrap_or_abort();
    let caller_workspace = tempdir().unwrap_or_abort();
    let session_config_path = session_workspace.path().join("harness.jsonc");
    let caller_config_path = caller_workspace.path().join("harness.jsonc");
    write_support_export_config(&session_config_path, "session", "session-inline-secret", "");
    write_support_export_config(&caller_config_path, "caller", "caller-inline-secret", "");

    let session_dir = session_workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_session_rooted_readiness");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_session_rooted_readiness",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "session-rooted-readiness".into(),
                    workspace_root: session_workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_session_rooted_readiness",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = session_workspace.path().join("session-rooted-export.json");

    // act
    let output = CliHarness::new()
        .current_dir(caller_workspace.path())
        .args([
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_export_session_rooted_readiness",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ])
        .output();

    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).unwrap_or_abort(),
    )
    .unwrap_or_abort();
    assert_eq!(
        bundle["support"]["config_summary"]["config"],
        session_config_path.display().to_string()
    );
    assert_eq!(
        bundle["support"]["provider_summary"]["providers"][0]["id"],
        "session"
    );
}

#[test]
fn sessions_export_cli_fails_closed_for_hidden_prompt_config_values_in_events() {
    // arrange
    let workspace = tempdir().unwrap_or_abort();
    let config_path = workspace.path().join("harness.jsonc");
    std::fs::write(
        &config_path,
        r#"
{
  "provider": {
    "test": {
      "type": "openai_compatible",
      "options": {
        "baseURL": "http://127.0.0.1:8317/v1",
        "apiKey": "safe-placeholder-key"
      },
      "models": {
        "gpt-5.4-mini": { "name": "GPT 5.4 Mini" }
      }
    }
  },
  "model": "test/gpt-5.4-mini",
  "instructions": ["plain-hidden-instruction-secret"],
  "agent": {
    "default": {
      "model": "test/gpt-5.4-mini",
      "system_prompt": "plain-hidden-agent-prompt-secret"
    },
    "general": {
      "model": "test/gpt-5.4-mini"
    }
  }
}
"#,
    )
    .unwrap_or_abort();

    let session_dir = workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_prompt_secrets");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_prompt_secrets",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt-secrets".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_prompt_secrets",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000004".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "plain-hidden-instruction-secret plain-hidden-agent-prompt-secret"
                            .to_string(),
                    ),
                    output_digest: Some("digest-prompt-secret".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                "run_export_prompt_secrets",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("session-export-prompt-secrets.json");

    // act
    let output = CliHarness::new()
        .current_dir(workspace.path())
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_export_prompt_secrets",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ])
        .output();

    // assert
    assert!(!output.status.success());
    assert!(!export_path.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("redaction scanner found"),
        "stderr should explain prompt secret fail-closed scan: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sessions_export_cli_excludes_stored_credentials_and_scans_for_leaks() {
    // arrange
    let workspace = tempdir().unwrap_or_abort();
    let data_home = workspace.path().join("data-home");
    let config_path = workspace.path().join("harness.jsonc");
    write_support_export_config(
        &config_path,
        "codex",
        "safe-placeholder-key",
        r#",
        "authProvider": "codex""#,
    );
    let store = harness_core::auth::CredentialStore::new(data_home.join("harness"));
    store
        .save(&harness_core::auth::StoredCredential::oauth(
            harness_core::auth::AuthProviderId::codex(),
            "stored-access-secret-value",
            "stored-refresh-secret-value",
            Some("2099-01-01T00:00:00Z".to_string()),
            "2026-05-30T00:00:00Z",
        ))
        .unwrap_or_abort();

    let session_dir = workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_stored_credentials");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_stored_credentials",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "stored-credentials".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_stored_credentials",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("session-export-stored-credentials.json");

    // act
    let output = CliHarness::new()
        .current_dir(workspace.path())
        .env("HARNESS_DATA_HOME", &data_home)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_export_stored_credentials",
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ])
        .output();

    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = std::fs::read_to_string(&export_path).unwrap_or_abort();
    assert!(!body.contains("stored-access-secret-value"));
    assert!(!body.contains("stored-refresh-secret-value"));
    let bundle: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_abort();
    assert_eq!(
        bundle["support"]["credential_store_manifest"]["providers"][0]["status"],
        "excluded_stored"
    );
    assert_eq!(
        bundle["support"]["credential_store_manifest"]["providers"][0]["relative_path"],
        "credentials/codex.json"
    );

    let leak_run_dir = session_dir.join("run_export_stored_credential_leak");
    std::fs::create_dir_all(&leak_run_dir).unwrap_or_abort();
    write_events_jsonl(
        &leak_run_dir,
        &[
            envelope(
                "run_export_stored_credential_leak",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "stored-credential-leak".into(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_stored_credential_leak",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000005".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("stored-access-secret-value".to_string()),
                    output_digest: Some("digest-stored-credential-leak".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
        ],
    );
    let leak_export_path = workspace
        .path()
        .join("session-export-stored-credential-leak.json");

    let leak_output = CliHarness::new()
        .current_dir(workspace.path())
        .env("HARNESS_DATA_HOME", &data_home)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "sessions",
            "export",
            "run_export_stored_credential_leak",
            "--output",
            leak_export_path.to_str().unwrap_or_abort(),
        ])
        .output();
    assert!(!leak_output.status.success());
    assert!(!leak_export_path.exists());
    assert!(
        String::from_utf8_lossy(&leak_output.stderr).contains("redaction scanner found"),
        "stderr should explain stored credential fail-closed scan: {}",
        String::from_utf8_lossy(&leak_output.stderr)
    );
}

fn write_support_export_config(
    config_path: &std::path::Path,
    provider_id: &str,
    api_key: &str,
    extra_options: &str,
) {
    std::fs::write(
        config_path,
        format!(
            r#"{{
  "provider": {{
    "{provider_id}": {{
      "type": "openai_compatible",
      "options": {{
        "baseURL": "http://127.0.0.1:8317/{provider_id}",
        "apiKey": "{api_key}"{extra_options}
      }},
      "models": {{
        "gpt-5.4-mini": {{ "name": "GPT 5.4 Mini" }}
      }}
    }}
  }},
  "model": "{provider_id}/gpt-5.4-mini",
  "agent": {{
    "default": {{
      "model": "{provider_id}/gpt-5.4-mini"
    }},
    "general": {{
      "model": "{provider_id}/gpt-5.4-mini"
    }}
  }}
}}
"#
        ),
    )
    .unwrap_or_abort();
}
