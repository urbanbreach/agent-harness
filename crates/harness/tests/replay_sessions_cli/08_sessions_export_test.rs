fn expanded_task_route_fixture() -> serde_json::Value {
    serde_json::from_str(r#"{"requested_category":"quick","requested_profile":"quick","resolved_profile":"general","profile_id":"general","role":"subagent","hidden":false,"prompt":{"source":"runtime_profile","status":"resolved_by_coordinator","profile":"general"},"model":{"model_ref":"default/gpt-5.4-mini","provider":"default","model":"gpt-5.4-mini","fallback_chain":[]},"toolset":["read","edit"],"permission_posture":{"spawn":"checked_before_child_turn","edit":"available_subject_to_runtime_permission","bash":"deny_by_toolset","question":"deny_by_toolset","task":"deny_by_toolset","webfetch":"deny_by_toolset","websearch":"deny_by_toolset","codesearch":"deny_by_toolset","lsp":"deny_by_toolset","background_output":"deny_by_toolset"},"permissions":{"spawn_permission_kind":"task","parent_scope":"build","child_scope":"general","scope_relation":"isolated_by_requested_profile"},"loaded_skills":[],"fallback_chain":["quick","general"],"category_fallback_chain":["quick","general"],"fallback":{"applied":true,"fallback_profile":"general","policy_source":"harness_core::coord::task_category_fallback_profile","disabled_parent_profiles":["plan"]}}"#)
        .expect("expanded task route fixture")
}

#[test]
fn sessions_export_cli_writes_json_bundle() {
    // arrange
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "exportable".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_export",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "build".to_string(),
                    parent_agent_id: None,
                }),
            ),
            agent_envelope(
                "run_export",
                3,
                "agent_1",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "trusted edit".to_string(),
                    request_digest: "digest-request".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_export",
                4,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000000".to_string(),
                    tool_id: "edit".to_string(),
                    args_summary: "edit demo.txt".to_string(),
                    args_digest: "digest-edit-args".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_export",
                5,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_1".to_string(),
                    kind: "edit".to_string(),
                    tool_call_id: Some("toolcall_000000".to_string()),
                    summary: "edit demo.txt".to_string(),
                    request_digest: "digest-permission".to_string(),
                    timeout_ms: 30_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                "run_export",
                6,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_1".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("manual allow".to_string()),
                }),
            ),
            envelope(
                "run_export",
                7,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000000".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("edit complete".to_string()),
                    output_digest: Some("digest-edit-output".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                "run_export",
                8,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("task scheduled".to_string()),
                    output_digest: Some("digest-route-output".to_string()),
                    output_json: Some(serde_json::json!({
                        "child_session_id": "agent_child",
                        "child_request_id": "req_child",
                        "route": expanded_task_route_fixture()
                    })),
                    metadata: None,
                }),
            ),
            envelope(
                "run_export",
                9,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let export_path = session_dir.path().join("session-export.json");

    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().expect("session dir utf-8"),
        "sessions",
        "export",
        "run_export",
        "--output",
        export_path.to_str().expect("export path utf-8"),
    ]);

    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&export_path).expect("read exported session bundle"))
            .expect("export bundle should parse");
    assert_eq!(bundle["catalog"]["run_id"], "run_export");
    assert_eq!(bundle["replay"]["run_name"], "exportable");
    assert_eq!(bundle["events"].as_array().map(Vec::len), Some(9));
    let route_metadata = bundle["support"]["route_metadata"]
        .as_array()
        .expect("route metadata array");
    let session_route = route_metadata
        .iter()
        .find(|entry| entry["source"] == "session_replay")
        .expect("session-derived route metadata");
    assert_eq!(session_route["route"]["run_id"], "run_export");
    assert_eq!(session_route["route"]["status"], "finished");
    assert_eq!(session_route["route"]["profiles"], serde_json::json!(["build"]));
    assert_eq!(
        session_route["route"]["provider_models"],
        serde_json::json!(["mock/model-1"])
    );
    assert!(session_route["route"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .any(|tool| tool["tool_id"] == "edit"));
    assert!(session_route["route"]["permissions"]
        .as_array()
        .expect("permissions array")
        .iter()
        .any(|permission| permission["decision"] == "allow"));
    let task_route = route_metadata
        .iter()
        .find(|entry| entry["source"] == "task_output")
        .expect("task-output route metadata");
    assert_eq!(task_route["route"]["requested_category"], "quick");
    assert_eq!(task_route["route"]["resolved_profile"], "general");
    assert_eq!(task_route["route"]["prompt"]["status"], "resolved_by_coordinator");
    assert_eq!(task_route["route"]["model"]["provider"], "default");
    assert_eq!(task_route["route"]["toolset"], serde_json::json!(["read", "edit"]));
    assert_eq!(
        task_route["route"]["permission_posture"]["task"],
        "deny_by_toolset"
    );
    assert_eq!(
        task_route["route"]["fallback_chain"],
        serde_json::json!(["quick", "general"])
    );
    assert_eq!(
        task_route["route"]["category_fallback_chain"],
        serde_json::json!(["quick", "general"])
    );
    assert_eq!(task_route["route"]["fallback"]["applied"], true);
    assert_eq!(
        task_route["route"]["fallback"]["policy_source"],
        "harness_core::coord::task_category_fallback_profile"
    );
    assert_eq!(
        task_route["child_request_id"],
        "req_child"
    );
}

#[test]
fn sessions_export_cli_support_includes_readiness_and_config_summaries() {
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
        "apiKey": "sk-AbCdEf0123456789"
      },
      "models": {
        "gpt-5.4-mini": { "name": "GPT 5.4 Mini" }
      }
    }
  },
  "model": "test/gpt-5.4-mini",
  "default_agent": "build",
  "permission": "ask",
  "agent": {
    "build": {
      "enable": true,
      "model": "test/gpt-5.4-mini"
    },
    "general": {
      "enable": true,
      "model": "test/gpt-5.4-mini"
    }
  },
  "skills": {
    "disabled": ["skill:project:disabled-support"]
  }
}
"#,
    )
    .expect("write config");
    let skill_dir = workspace.path().join(".agent-harness/skills/support-skill");
    std::fs::create_dir_all(&skill_dir).expect("create support skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: support-skill\ndescription: Support export compact metadata\n---\n\nSUPPORT SKILL BODY SENTINEL\n",
    )
    .expect("write support skill");
    let disabled_skill_dir = workspace
        .path()
        .join(".agent-harness/skills/disabled-support");
    std::fs::create_dir_all(&disabled_skill_dir).expect("create disabled support skill dir");
    std::fs::write(
        disabled_skill_dir.join("SKILL.md"),
        "---\nname: disabled-support\ndescription: Disabled support export metadata\n---\n\nDISABLED SUPPORT BODY SENTINEL\n",
    )
    .expect("write disabled support skill");

    let session_dir = workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_support_readiness");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_support_readiness",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "support-readiness".to_string(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_support_readiness",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let export_path = workspace.path().join("session-export-support.json");

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
            "run_export_support_readiness",
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

    let export_text = std::fs::read_to_string(&export_path).expect("read support export");
    assert!(!export_text.contains("sk-AbCdEf0123456789"));
    assert!(!export_text.contains("SUPPORT SKILL BODY SENTINEL"));
    assert!(!export_text.contains("DISABLED SUPPORT BODY SENTINEL"));
    let bundle: serde_json::Value =
        serde_json::from_str(&export_text).expect("support export bundle should parse");

    assert_eq!(bundle["support"]["doctor_json"]["no_network_probes"], true);
    assert_eq!(
        bundle["support"]["doctor_json"]["config"],
        config_path.display().to_string()
    );
    let resolved_routes = bundle["support"]["doctor_json"]["checks"]
        .as_array()
        .expect("doctor checks")
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .expect("resolved route check");
    assert_eq!(
        resolved_routes["details"]["skills"]["no_network_probes"],
        true
    );
    assert_eq!(
        resolved_routes["details"]["skills"]["catalog_source"],
        "harness_tools::skill_catalog"
    );
    assert_eq!(
        resolved_routes["details"]["skills"]["readiness"]["disabled_count"],
        1
    );
    assert!(resolved_routes["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .expect("doctor skill catalog entries")
        .iter()
        .any(|entry| entry["name"] == "support-skill" && entry["body_loaded"] == false));
    assert!(resolved_routes["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .expect("doctor skill catalog entries")
        .iter()
        .any(|entry| {
            entry["name"] == "disabled-support"
                && entry["stable_id"] == "skill:project:disabled-support"
                && entry["status"] == "disabled"
                && entry["body_loaded"] == false
        }));
    assert_eq!(
        bundle["support"]["skill_catalog_summary"]["source"],
        "harness_tools::skill_catalog"
    );
    assert_eq!(
        bundle["support"]["skill_catalog_summary"]["entry_count"],
        2
    );
    assert_eq!(
        bundle["support"]["skill_catalog_summary"]["disabled_count"],
        1
    );
    assert!(bundle["support"]["skill_catalog_summary"]["entries"]
        .as_array()
        .expect("support skill catalog entries")
        .iter()
        .any(|entry| {
            entry["name"] == "support-skill"
                && entry["stable_id"] == "skill:project:support-skill"
                && entry["status"] == "loadable"
                && entry["body_loaded"] == false
        }));
    assert!(bundle["support"]["skill_catalog_summary"]["entries"]
        .as_array()
        .expect("support skill catalog entries")
        .iter()
        .any(|entry| {
            entry["name"] == "disabled-support"
                && entry["stable_id"] == "skill:project:disabled-support"
                && entry["status"] == "disabled"
                && entry["source_scope"] == "project"
                && entry["body_loaded"] == false
        }));
    assert_eq!(bundle["support"]["config_summary"]["loaded"], true);
    assert_eq!(bundle["support"]["config_summary"]["default_agent"], "build");
    assert_eq!(
        bundle["support"]["provider_summary"]["providers"][0]["id"],
        "test"
    );
    assert_eq!(
        bundle["support"]["provider_summary"]["providers"][0]["credentials"],
        "inline_redacted"
    );
    assert_eq!(
        bundle["support"]["provider_summary"]["providers"][0]["base_url"],
        "http://127.0.0.1:8317/v1"
    );
    assert_eq!(
        bundle["support"]["provider_summary"]["providers"][0]["model_count"],
        1
    );
    assert_support_export_catalog_metadata(&bundle);
}

#[test]
fn sessions_export_cli_redacts_support_bundle_secret_shapes() {
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
        "baseURL": "https://user:pass@example.test/v1?api_key=AIzaSyA1234567890abcdefghi",
        "apiKey": "sk-proj-config_secret_0123456789abcdef"
      },
      "models": {
        "gpt-5.4-mini": { "name": "GPT 5.4 Mini" }
      }
    }
  },
  "model": "test/gpt-5.4-mini",
  "default_agent": "build"
}
"#,
    )
    .expect("write config");

    let session_dir = workspace.path().join(".agent-harness/sessions");
    let run_dir = session_dir.join("run_export_secret_shapes");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_secret_shapes",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "secret-shapes".to_string(),
                    workspace_root: workspace.path().display().to_string(),
                }),
            ),
            envelope(
                "run_export_secret_shapes",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "sk-proj-output_secret_0123456789abcdef Cookie: sid=sessionid-abc123; theme=light\n-----BEGIN PRIVATE KEY-----\nprivate-key-material\n-----END PRIVATE KEY-----\nAKIA1234567890ABCDEF\nAuthorization: bearer abc+/def==~\nghp_1234567890ABCDEFGHIJ github_pat_1234567890ABCDEFGHIJ".to_string(),
                    ),
                    output_digest: Some("digest-secret-shapes".to_string()),
                    output_json: Some(serde_json::json!({
                        "authorization": "Bearer abc.def-ghi_123",
                        "token": "plain-token-value",
                        "password": "hunter2",
                        "sk-proj-key_name_0123456789abcdef": "secret value in key name"
                    })),
                    metadata: None,
                }),
            ),
            envelope(
                "run_export_secret_shapes",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let export_path = workspace.path().join("session-export-secret-shapes.json");

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
            "run_export_secret_shapes",
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
    let export_text = std::fs::read_to_string(&export_path).expect("read secret-shapes export");
    for forbidden in [
        "sk-proj-config_secret_0123456789abcdef",
        "sk-proj-output_secret_0123456789abcdef",
        "sk-proj-key_name_0123456789abcdef",
        "sessionid-abc123",
        "BEGIN PRIVATE KEY",
        "private-key-material",
        "AKIA1234567890ABCDEF",
        "user:pass@",
        "api_key=AIzaSyA1234567890abcdefghi",
        "Bearer abc.def-ghi_123",
        "bearer abc+/def==~",
        "abc+/def==~",
        "ghp_1234567890ABCDEFGHIJ",
        "github_pat_1234567890ABCDEFGHIJ",
        "plain-token-value",
        "hunter2",
    ] {
        assert!(!export_text.contains(forbidden), "export leaked {forbidden}");
    }

    let bundle: serde_json::Value =
        serde_json::from_str(&export_text).expect("secret-shapes export should parse");
    assert_eq!(bundle["support"]["redaction_manifest"]["status"], "clean");
    assert_eq!(bundle["support"]["secret_scan_status"]["status"], "clean");
    assert_eq!(
        bundle["support"]["secret_scan_status"]["secret_finding_count"],
        0
    );
    assert!(
        bundle["support"]["redaction_manifest"]["redacted_marker_count"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn sessions_export_cli_redacts_secret_payloads_and_reports_manifest() {
    // arrange
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export_redaction");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export_redaction",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "export-redaction".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_export_redaction",
                2,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "raw token sk-AbCdEf0123456789 and Authorization: Bearer abc.def-ghi_123"
                            .to_string(),
                    ),
                    output_digest: Some("digest-secret-output".to_string()),
                    output_json: Some(serde_json::json!({
                        "secret": "sk-AbCdEf0123456789",
                        "authorization": "Bearer abc.def-ghi_123"
                    })),
                    metadata: None,
                }),
            ),
            envelope(
                "run_export_redaction",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let export_path = session_dir.path().join("session-export-redacted.json");
    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().expect("session dir utf-8"),
        "sessions",
        "export",
        "run_export_redaction",
        "--output",
        export_path.to_str().expect("export path utf-8"),
    ]);
    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let export_text = std::fs::read_to_string(&export_path).expect("read redacted export");
    assert!(!export_text.contains("sk-AbCdEf0123456789"));
    assert!(!export_text.contains("Bearer abc.def-ghi_123"));
    assert!(export_text.contains("[REDACTED_API_KEY]"));
    assert!(export_text.contains("Bearer [REDACTED]"));

    let bundle: serde_json::Value =
        serde_json::from_str(&export_text).expect("redacted export bundle should parse");
    assert_eq!(bundle["support"]["redaction_manifest"]["status"], "clean");
    assert_eq!(
        bundle["support"]["secret_scan_status"]["secret_finding_count"],
        0
    );
}

#[test]
fn sessions_export_cli_support_includes_artifact_index() {
    // arrange
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export_artifacts");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    write_events_jsonl(
        &run_dir,
        &delegated_recovery_events("run_export_artifacts"),
    );

    let export_path = session_dir.path().join("session-export-artifacts.json");
    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().expect("session dir utf-8"),
        "sessions",
        "export",
        "run_export_artifacts",
        "--output",
        export_path.to_str().expect("export path utf-8"),
    ]);
    // assert
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&export_path).expect("read artifact-index export"),
    )
    .expect("artifact-index export bundle should parse");
    assert_eq!(
        bundle["support"]["artifact_index"][0]["path"],
        "artifacts/delegated/task-output.json"
    );
    assert_eq!(
        bundle["support"]["artifact_index"][0]["child_session_id"],
        "child-run-001"
    );
    assert_eq!(
        bundle["support"]["artifact_index"][0]["canonical_tool_id"],
        "agent.spawn"
    );
}
