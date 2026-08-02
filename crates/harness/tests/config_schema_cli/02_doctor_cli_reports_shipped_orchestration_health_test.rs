use harness::UnwrapOrAbort;
#[test]
fn doctor_cli_reports_shipped_orchestration_health() {
    // arrange
    // act
    // assert
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .env("OPENAI_API_KEY", "doctor-shipped-example-test-key")
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor ok:"));
    assert!(stdout.contains("local readiness only"));
    assert!(stdout.contains("not provider execution proof"));
    assert!(stdout.contains("provider_credentials"));
    assert!(stdout.contains("model_references"));
    assert!(stdout.contains("workflow_profiles"));
    assert!(stdout.contains("category_routes"));
    assert!(stdout.contains("visual-engineering"));
}
#[test]
fn doctor_cli_emits_json_report() {
    // arrange
    // act
    // assert
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .env("OPENAI_API_KEY", "doctor-shipped-example-test-key")
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert!(report["config"]
        .as_str()
        .unwrap_or_abort()
        .contains("configs/harness.example.jsonc"));
    assert_eq!(report["no_network_probes"], true);
    assert_eq!(report["provider_execution_proof"], false);
    assert_eq!(report["readiness_scope"], "local_readiness_only");
    assert!(report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|check| { check["name"] == "workflow_profiles" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|check| { check["name"] == "category_routes" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|check| { check["name"] == "provider_credentials" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|check| { check["name"] == "model_references" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|check| { check["name"] == "native_tool_catalog" && check["status"] == "pass" }));
}

#[test]
fn doctor_cli_json_reports_extension_roadmap_readiness_separately() {
    // arrange
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    // act
    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let readiness_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "extension_roadmap_readiness")
        .unwrap_or_abort();
    assert_eq!(readiness_check["status"], "pass");
    assert_eq!(
        readiness_check["details"]["separate_from_runtime_health"],
        true
    );
    assert_eq!(
        readiness_check["details"]["descriptor_seams"]["typed_extension_manifest"]["status"],
        "shipped_descriptor_only"
    );
    assert_eq!(
        readiness_check["details"]["descriptor_seams"]["typed_extension_manifest"]
            ["runtime_effects_scope"],
        "descriptor_only"
    );
    assert_eq!(
        readiness_check["details"]["descriptor_seams"]["typed_extension_manifest"]
            ["runtime_effects"]["registers_tools"],
        false
    );
    assert_eq!(
        readiness_check["details"]["planned_seams"]["desktop_mobile_web_clients"],
        "post_v1"
    );
    assert_eq!(readiness_check["details"]["no_network_probes"], true);
}

#[test]
fn doctor_cli_json_reports_resolved_route_metadata() {
    // arrange
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    // act
    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let route_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .unwrap_or_abort();

    assert_eq!(route_check["status"], "pass");
    assert_eq!(route_check["details"]["routes"]["build"]["role"], "primary");
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["tool_call_support"]["status"],
        "supported"
    );
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["tool_call_support"]
            ["supports_tool_calls"],
        true
    );
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["tool_call_support"]["source"],
        "provider_model_metadata"
    );
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["tool_call_support"]
            ["no_network_probes"],
        true
    );
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["prompt_family_asset"]["status"],
        "builtin"
    );
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["prompt_family_asset"]
            ["no_network_probes"],
        true
    );
    assert_eq!(
        route_check["details"]["routes"]["build"]["prompt"]["status"],
        "available"
    );
    assert_eq!(route_check["details"]["skills"]["status"], "configured");
    assert_eq!(
        route_check["details"]["skills"]["no_network_probes"],
        true
    );
    assert!(route_check["details"]["skills"]["project_roots"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|root| root == ".agent-harness/skills"));
    assert_eq!(
        route_check["details"]["skills"]["catalog_source"],
        "harness_tools::skill_catalog"
    );
    assert_eq!(
        route_check["details"]["skills"]["readiness"]["loadable_count"],
        6
    );
    let skill_entries = route_check["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .unwrap_or_abort();
    assert!(skill_entries.iter().any(|entry| {
        entry["name"] == "rust-best-practices"
            && entry["stable_id"] == "skill:project:rust-best-practices"
            && entry["status"] == "loadable"
            && entry["source_scope"] == "project"
            && entry["body_loaded"] == false
    }));
    for builtin in ["git-master", "review-work", "frontend-ui-ux", "harness-qa"] {
        assert!(skill_entries.iter().any(|entry| {
            entry["name"] == builtin
                && entry["stable_id"] == format!("skill:project:{builtin}")
                && entry["status"] == "loadable"
                && entry["source_scope"] == "project"
                && entry["body_loaded"] == false
        }));
    }
    assert!(!serde_json::to_string(&route_check["details"]["skills"])
        .unwrap_or_abort()
        .contains("Use focused diffs."));
    assert_eq!(
        route_check["details"]["routes"]["general"]["role"],
        "subagent"
    );
    assert_eq!(
        route_check["details"]["routes"]["explore"]["permission_posture"]["edit"],
        "deny"
    );
    assert_eq!(
        route_check["details"]["routes"]["visual-engineering"]["role"],
        "category"
    );
    assert_eq!(
        route_check["details"]["routes"]["visual-engineering"]["permission_posture"]["task"],
        "deny"
    );
    assert_eq!(
        route_check["details"]["category_fallback"]["unknown_category_profile"],
        "general"
    );
    assert_eq!(
        route_check["details"]["category_fallback"]["disabled_for_parent"],
        serde_json::json!(["plan"])
    );
    assert_eq!(
        route_check["details"]["category_fallback"]["policy_source"],
        "harness_core::coord::task_category_fallback_profile"
    );
}

#[test]
fn doctor_cli_json_reports_prompt_family_asset_fallback_warning() {
    // arrange
    // act
    // assert
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
                "claude-sonnet-fixture": {
                  name: "Claude Sonnet Fixture",
                  metadata: { family: "claude", supportsToolCalls: true },
                },
              },
            },
          },
          model: "default/claude-sonnet-fixture",
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
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let route_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .unwrap_or_abort();
    let prompt_asset = &route_check["details"]["routes"]["build"]["model"]["prompt_family_asset"];

    assert_eq!(prompt_asset["family"], "anthropic");
    assert_eq!(prompt_asset["status"], "fallback");
    assert_eq!(prompt_asset["source"], "default_prompt_fallback");
    assert_eq!(
        prompt_asset["path"],
        ".agent-harness/prompt-families/anthropic.md"
    );
    assert!(prompt_asset["warning"]
        .as_str()
        .unwrap_or_abort()
        .contains("using default prompt"));
    assert_eq!(prompt_asset["no_network_probes"], true);
}

#[test]
fn doctor_cli_json_reports_stable_id_disabled_skill_metadata() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    fs::create_dir_all(temp.path().join(".agent-harness/skills/disabled-doctor"))
        .unwrap_or_abort();
    fs::write(
        temp.path()
            .join(".agent-harness/skills/disabled-doctor/SKILL.md"),
        "---\nname: disabled-doctor\ndescription: Disabled doctor skill\n---\n\nDISABLED DOCTOR BODY SENTINEL\n",
    )
    .unwrap_or_abort();
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
          default_agent: "build",
          agent: {
            build: { enable: true, model: "default/gpt-5.4-mini" },
            general: { enable: true, model: "default/gpt-5.4-mini" },
          },
          permission: "ask",
          skills: {
            disabled: ["skill:project:disabled-doctor"],
          },
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("DISABLED DOCTOR BODY SENTINEL"));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let route_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .unwrap_or_abort();
    assert_eq!(
        route_check["details"]["skills"]["readiness"]["disabled_count"],
        1
    );
    let skill_entries = route_check["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .unwrap_or_abort();
    assert!(skill_entries.iter().any(|entry| {
        entry["name"] == "disabled-doctor"
            && entry["stable_id"] == "skill:project:disabled-doctor"
            && entry["status"] == "disabled"
            && entry["source_scope"] == "project"
            && entry["body_loaded"] == false
    }));
}

#[test]
fn doctor_cli_json_reports_disabled_builtin_skill_metadata() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
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
          default_agent: "build",
          agent: {
            build: { enable: true, model: "default/gpt-5.4-mini" },
            general: { enable: true, model: "default/gpt-5.4-mini" },
          },
          permission: "ask",
          skills: {
            disabled: ["skill:project:git-master"],
          },
        }
        "#,
    )
    .unwrap_or_abort();

    let repo_root = repo_root();
    // act
    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Use git with atomic, reviewable intent"));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let route_check = report["checks"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .unwrap_or_abort();
    assert_eq!(
        route_check["details"]["skills"]["readiness"]["disabled_count"],
        1
    );
    let skill_entries = route_check["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .unwrap_or_abort();
    assert!(skill_entries.iter().any(|entry| {
        entry["name"] == "git-master"
            && entry["stable_id"] == "skill:project:git-master"
            && entry["status"] == "disabled"
            && entry["source_scope"] == "project"
            && entry["body_loaded"] == false
            && entry["reason"] == "disabled by skills.disabled"
    }));
}
