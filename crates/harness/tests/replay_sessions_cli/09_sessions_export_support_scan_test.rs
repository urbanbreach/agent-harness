#[test]
fn sessions_export_cli_fails_closed_for_resolved_config_credentials_in_events() {
    // arrange
    let workspace = tempdir().expect("workspace tempdir");
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
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_config_credentials",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "config-credentials".to_string(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_config_credentials",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000003".to_string(),
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
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "sessions",
            "export",
            "run_export_config_credentials",
            "--output",
            export_path.to_str().expect("export path utf-8"),
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
    let session_workspace = tempdir().expect("session workspace tempdir");
    let caller_workspace = tempdir().expect("caller workspace tempdir");
    let session_config_path = session_workspace.path().join("harness.jsonc");
    let caller_config_path = caller_workspace.path().join("harness.jsonc");
    write_support_export_config(&session_config_path, "session", "session-inline-secret", "");
    write_support_export_config(&caller_config_path, "caller", "caller-inline-secret", "");

    let session_dir = session_workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_session_rooted_readiness");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_session_rooted_readiness",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "session-rooted-readiness".to_string(),
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
            session_dir.to_str().expect("session dir utf-8"),
            "sessions",
            "export",
            "run_export_session_rooted_readiness",
            "--output",
            export_path.to_str().expect("export path utf-8"),
        ])
        .output();

    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bundle: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).expect("read session-rooted export"),
    )
    .expect("session-rooted export bundle should parse");
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
    let workspace = tempdir().expect("workspace tempdir");
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
  "default_agent": "build",
  "agent": {
    "build": {
      "enable": true,
      "model": "test/gpt-5.4-mini",
      "prompt": "plain-hidden-agent-prompt-secret"
    },
    "general": {
      "enable": true,
      "model": "test/gpt-5.4-mini"
    }
  }
}
"#,
    )
    .expect("write prompt-secret config");

    let session_dir = workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_prompt_secrets");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_prompt_secrets",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt-secrets".to_string(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_prompt_secrets",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000004".to_string(),
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
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "sessions",
            "export",
            "run_export_prompt_secrets",
            "--output",
            export_path.to_str().expect("export path utf-8"),
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
  "default_agent": "build",
  "agent": {{
    "build": {{
      "enable": true,
      "model": "{provider_id}/gpt-5.4-mini"
    }},
    "general": {{
      "enable": true,
      "model": "{provider_id}/gpt-5.4-mini"
    }}
  }}
}}
"#
        ),
    )
    .expect("write support export config");
}
