#[test]
fn doctor_cli_reports_shipped_orchestration_health() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
        ])
        .output()
        .expect("run harness doctor with shipped example config");

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
    assert!(stdout.contains("discipline"));
    assert!(stdout.contains("visual-engineering"));
}
#[test]
fn doctor_cli_emits_json_report() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor --json with shipped example config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    assert!(report["config"]
        .as_str()
        .expect("config display")
        .contains("configs/harness.example.jsonc"));
    assert_eq!(report["no_network_probes"], true);
    assert_eq!(report["provider_execution_proof"], false);
    assert_eq!(report["readiness_scope"], "local_readiness_only");
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "workflow_profiles" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "category_routes" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "provider_credentials" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "model_references" && check["status"] == "pass" }));
    assert!(report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .any(|check| { check["name"] == "native_tool_catalog" && check["status"] == "pass" }));
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
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor --json with shipped example config");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let route_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .expect("resolved route metadata check");

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
        .expect("project roots array")
        .iter()
        .any(|root| root == ".agent-harness/skills"));
    assert_eq!(
        route_check["details"]["skills"]["catalog_source"],
        "harness_tools::skill_catalog"
    );
    assert_eq!(
        route_check["details"]["skills"]["readiness"]["loadable_count"],
        2
    );
    let skill_entries = route_check["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .expect("skill catalog entries");
    assert!(skill_entries.iter().any(|entry| {
        entry["name"] == "rust-best-practices"
            && entry["stable_id"] == "skill:project:rust-best-practices"
            && entry["status"] == "loadable"
            && entry["source_scope"] == "project"
            && entry["body_loaded"] == false
    }));
    assert!(!serde_json::to_string(&route_check["details"]["skills"])
        .expect("serialize compact skill readiness")
        .contains("Use focused diffs."));
    assert_eq!(
        route_check["details"]["routes"]["discipline"]["skills"]["tool_enabled"],
        true
    );
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
fn doctor_cli_json_reports_stable_id_disabled_skill_metadata() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join(".agent-harness/skills/disabled-doctor"))
        .expect("create disabled skill");
    fs::write(
        temp.path()
            .join(".agent-harness/skills/disabled-doctor/SKILL.md"),
        "---\nname: disabled-doctor\ndescription: Disabled doctor skill\n---\n\nDISABLED DOCTOR BODY SENTINEL\n",
    )
    .expect("write disabled skill");
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
    .expect("write config with stable-id disabled skill");

    let output = harness_command()
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "doctor",
            "--json",
        ])
        .output()
        .expect("run harness doctor with stable-id disabled skill");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("DISABLED DOCTOR BODY SENTINEL"));
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor json report");
    let route_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .expect("resolved route metadata check");
    assert_eq!(
        route_check["details"]["skills"]["readiness"]["disabled_count"],
        1
    );
    let skill_entries = route_check["details"]["skills"]["catalog"]["entries"]
        .as_array()
        .expect("skill catalog entries");
    assert!(skill_entries.iter().any(|entry| {
        entry["name"] == "disabled-doctor"
            && entry["stable_id"] == "skill:project:disabled-doctor"
            && entry["status"] == "disabled"
            && entry["source_scope"] == "project"
            && entry["body_loaded"] == false
    }));
}

#[test]
fn doctor_cli_fails_invalid_category_routes_even_when_some_are_missing() {
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
        .expect("run harness doctor with invalid category routes");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor found issues:"));
    assert!(stdout.contains("[FAIL] category_routes"));
    assert!(stdout.contains("visual-engineering"));
    assert!(stdout.contains("artistry"));
}
#[test]
fn doctor_cli_reports_model_profile_fallback_targets() {
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
        .expect("run harness doctor with model profile fallback");

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
        .expect("run harness doctor with missing credentials");

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
                .expect("run harness doctor with env credentials");

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
fn config_validate_cli_accepts_provider_catalog_reference_config_by_explicit_path() {
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
        .expect("run harness config validate with reference catalog config");

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
        .get("default")
        .expect("default provider present in reference catalog");
    assert!(provider.models.len() > 1);
}
#[test]
fn config_validate_cli_does_not_auto_discover_provider_catalog_reference_config() {
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
        .expect("run harness config validate with only reference catalog present");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(!stderr.contains("provider-catalog.reference.jsonc"));
}
#[test]
fn config_validate_cli_merges_xdg_defaults_with_local_project_override() {
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
        .expect("run harness config validate with merged discovery");

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
        .expect("plan profile should be translated from public config");

    assert_eq!(
        plan.description,
        "Plan mode. Disallows all edit tools except the active plan file."
    );
    assert_eq!(plan.model_ref, "default/gpt-4o-mini");
}
#[test]
fn config_validate_cli_accepts_legacy_harness_native_shape() {
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
        .expect("run harness config validate with legacy config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn config_validate_cli_accepts_legacy_xdg_config_path_for_migration() {
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
        .expect("run harness config validate with legacy xdg path");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config.jsonc"));
}
