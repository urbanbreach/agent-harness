#[test]
fn config_validate_cli_loads_separate_tui_config() {
    let temp = tempdir().expect("tempdir");
    write_config(
        &temp.path().join("harness.jsonc"),
        &canonical_runtime_config(),
    );
    write_config(
        &temp.path().join("tui.jsonc"),
        &serde_json::json!({
            "keybinds": {
                "variant_cycle": "tab"
            }
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with separate tui config");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tui.jsonc"));
}
#[test]
fn config_validate_cli_accepts_harness_config_env_path() {
    let temp = tempdir().expect("tempdir");
    let env_config_path = temp.path().join("env/custom-harness.jsonc");
    fs::create_dir_all(env_config_path.parent().expect("env config parent"))
        .expect("create env config dir");
    write_config(&env_config_path, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_CONFIG", &env_config_path)
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with HARNESS_CONFIG");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains(env_config_path.to_str().expect("env config path utf-8")));
}
#[test]
fn config_validate_cli_applies_harness_config_content_last() {
    let temp = tempdir().expect("tempdir");
    write_config(
        &temp.path().join("harness.jsonc"),
        &canonical_runtime_config(),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env(
            "HARNESS_CONFIG_CONTENT",
            r#"{ default_agent: "plan", permission: { bash: "deny" } }"#,
        )
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with HARNESS_CONFIG_CONTENT");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn config_validate_cli_explicit_path_bypasses_discovery_layers() {
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let explicit_path = temp.path().join("explicit.jsonc");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");

    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    write_config(&xdg_config_path, &canonical_runtime_config());
    write_config(
        &explicit_path,
        &serde_json::json!({
            "provider": {
                "default": {
                    "type": "openai_compatible",
                    "baseURL": "http://127.0.0.1:1/v1",
                    "apiKey": "DUMMY",
                    "models": {
                        "gpt-4o": {
                            "name": "GPT-4o"
                        }
                    }
                }
            },
            "mystery": true
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args([
            "--config",
            explicit_path.to_str().expect("explicit config utf-8"),
            "config",
            "validate",
        ])
        .output()
        .expect("run harness config validate with explicit path");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys"));
    assert!(stderr.contains("`mystery`"));
}
#[test]
fn config_validate_cli_rejects_unknown_top_level_keys() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["mystery"] = serde_json::json!({"enabled": true});
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with unknown key");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys:"));
    assert!(stderr.contains("`mystery`"));
}
#[test]
fn config_validate_cli_rejects_unknown_provider_reference() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["agent"] = serde_json::json!({
        "deep": {
            "description": "Deep profile",
            "model": "missing/gpt-4o-mini",
            "tools": []
        }
    });
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate with invalid provider ref");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agent `deep` has invalid model selection `missing/gpt-4o-mini`"));
    assert!(stderr.contains("unknown provider `missing`"));
}
#[test]
fn config_validate_cli_rejects_unsupported_upstream_top_level_keys() {
    for key in [
        "server",
        "command",
        "plugin",
        "share",
        "autoshare",
        "autoupdate",
        "enterprise",
    ] {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("harness.jsonc");
        let mut config = canonical_runtime_config();
        config[key] = serde_json::json!({ "enabled": true });
        write_config(&config_path, &config);

        let output = harness_command()
            .current_dir(temp.path())
            .args(["config", "validate"])
            .output()
            .unwrap_or_else(|err| {
                panic!("run harness config validate with unsupported key {key}: {err}")
            });

        assert!(
            !output.status.success(),
            "unsupported key {key} unexpectedly validated"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unsupported active OpenCode config keys:"),
            "unsupported key {key} stderr did not mention unsupported active OpenCode keys:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("`{key}`")),
            "unsupported key {key} stderr did not name the key:\n{stderr}"
        );
    }
}
#[test]
fn opencode_config_shape_accepts_subagents_and_safe_inert_keys() {
    let parsed = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:1/v1",
                apiKey: "DUMMY"
              },
              models: {
                "gpt-4o": { name: "GPT-4o" }
              }
            }
          },
          model: "default/gpt-4o",
          default_agent: "build",
          shell: "bash",
          logLevel: "INFO",
          username: "operator",
          watcher: { ignore: ["target/**"] },
          snapshot: false,
          disabled_providers: ["unused"],
          enabled_providers: ["default"],
          formatter: false,
          lsp: false,
          layout: "stretch",
          tools: { bash: true },
          tool_output: { max_lines: 20 },
          compaction: { auto: true, tail_turns: 2 },
          experimental: { batch_tool: true },
          skills: { paths: [".opencode/skill"], urls: ["https://example.test/skills"] },
          mcp: {
            local_docs: {
              type: "local",
              command: ["docs-mcp"],
              enabled: false
            },
            disabled_only: { enabled: false }
          },
          mode: {
            legacy: {
              description: "Legacy mode alias",
              mode: "subagent"
            }
          },
          agent: {
            general: {
              description: "Configured general subagent",
              mode: "subagent",
              reasoningEffort: "high"
            },
            reviewer: {
              description: "Review work",
              mode: "subagent",
              customFlag: true
            },
            summary: { enable: false },
            compaction: { enabled: false }
          },
          permission: "allow"
        }
        "#,
    )
    .expect("OpenCode-compatible config shape should parse");

    assert!(parsed.agents.contains_key("build"));
    assert_eq!(
        parsed.agents["general"].mode,
        harness_core::config::AgentMode::Subagent
    );
    assert_eq!(
        parsed.agents["general"].options.get("reasoningEffort"),
        Some(&serde_json::json!("high"))
    );
    assert_eq!(
        parsed.agents["reviewer"].options.get("customFlag"),
        Some(&serde_json::json!(true))
    );
    assert!(parsed.agents.contains_key("legacy"));
    assert!(!parsed.agents.contains_key("summary"));
    assert!(!parsed.agents.contains_key("compaction"));
    assert!(parsed.lsp.disabled);
    assert!(parsed
        .skills
        .project_roots
        .contains(&Path::new(".opencode/skill").to_path_buf()));
    assert!(parsed.integrations.mcp.servers.contains_key("local_docs"));
    assert!(!parsed
        .integrations
        .mcp
        .servers
        .contains_key("disabled_only"));
}
#[test]
fn shipped_example_config_uses_placeholder_safe_provider_without_openai_api_key() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let parsed = load_config_from_file(&config_path)
        .expect("shipped example config should use its explicit placeholder key");
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("default")
        .expect("default provider present in shipped example config");

    assert_eq!(
        provider.name.as_deref(),
        Some("Local OpenAI-Compatible Provider")
    );
    assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
    assert_eq!(provider.api_key, "placeholder-api-key");
    assert_eq!(provider.timeout_ms, 1_800_000);
    assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
}
#[test]
fn shipped_example_config_keeps_placeholder_safe_provider_even_when_openai_api_key_is_set() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let parsed = load_config_from_file(&config_path)
        .expect("shipped example config should keep its explicit placeholder key");
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("default")
        .expect("default provider present in shipped example config");

    assert_eq!(
        provider.name.as_deref(),
        Some("Local OpenAI-Compatible Provider")
    );
    assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
    assert_eq!(provider.api_key, "placeholder-api-key");
    assert_eq!(provider.timeout_ms, 1_800_000);
    assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
}
#[test]
fn checked_in_tui_schema_matches_generated_schema() {
    let generated = harness_tui_schema_pretty_json().expect("generate tui schema");
    let checked_in = fs::read_to_string(repo_root().join("configs").join("tui.json"))
        .expect("read checked-in tui schema");

    assert_eq!(checked_in.trim(), generated.trim());
}
#[test]
fn checked_in_runtime_schema_matches_generated_schema() {
    let generated = harness_schema_pretty_json().expect("generate runtime schema");
    let checked_in = fs::read_to_string(repo_root().join("configs").join("config.json"))
        .expect("read checked-in runtime schema");

    assert_eq!(checked_in.trim(), generated.trim());
}
#[test]
fn shipped_tui_example_parses_as_public_tui_config() {
    let shipped = fs::read_to_string(repo_root().join("configs").join("tui.example.jsonc"))
        .expect("read shipped tui example");

    let parsed: PublicTuiConfig = json5::from_str(&shipped).expect("parse shipped tui example");

    assert_eq!(
        parsed.keybindings.get("variant_cycle"),
        Some(&"ctrl+t".to_string())
    );
    assert_eq!(
        parsed.keybindings.get("session_child_first"),
        Some(&"ctrl+]".to_string())
    );
}
#[test]
fn shipped_runtime_example_parses_as_public_runtime_config() {
    let shipped = fs::read_to_string(repo_root().join("configs").join("harness.example.jsonc"))
        .expect("read shipped runtime example");

    let parsed: PublicRuntimeConfig =
        json5::from_str(&shipped).expect("parse shipped runtime example");

    assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    assert_eq!(parsed.model.as_deref(), Some("default/gpt-5.4-mini"));
    assert_eq!(parsed.small_model.as_deref(), None);
    assert_eq!(parsed.provider.len(), 1);
    assert!(parsed.provider.contains_key("default"));
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .provider
        .get("default")
        .expect("default provider present in public example");
    assert_eq!(provider.models.len(), 2);
    assert!(provider.models.contains_key("gpt-5.5"));
    assert!(provider.models.contains_key("gpt-5.4-mini"));
    assert!(parsed.agent.build.is_some());
    assert!(parsed.agent.plan.is_some());
    assert!(parsed.agent.general.is_some());
    assert!(parsed.agent.explore.is_some());
    assert!(parsed.agent.visual_engineering.is_some());
    assert!(parsed.agent.artistry.is_some());
    assert!(parsed.agent.ultrabrain.is_some());
    assert!(parsed.agent.deep.is_some());
    assert!(parsed.agent.quick.is_some());
    assert!(parsed.agent.unspecified_low.is_some());
    assert!(parsed.agent.unspecified_high.is_some());
    assert!(parsed.agent.writing.is_some());
    assert!(parsed.agent.title.is_some());
    assert!(parsed.agent.summary.is_some());
    assert!(parsed.agent.compaction.is_some());
    assert_eq!(
        parsed.agent.general.as_ref().and_then(|agent| agent.enable),
        Some(true)
    );
    assert_eq!(
        parsed.agent.title.as_ref().and_then(|agent| agent.hidden),
        Some(true)
    );
    let mut variants = provider.models["gpt-5.4-mini"]
        .variants
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    variants.sort_unstable();
    assert_eq!(variants, vec!["high", "low", "medium"]);
    assert!(!parsed.provider.contains_key("providers"));
    assert!(!shipped.contains("\"base_url\""));
    assert!(!shipped.contains("\"api_key\""));
    assert!(!shipped.contains("sk-zerolimit"));
    assert!(!shipped.contains("\"api_mode\""));
    assert!(!shipped.contains("\"timeout_ms\""));
    assert!(!shipped.contains("\"model_backed\""));
    assert!(!shipped.contains("\"split_oversized_turns\""));
    assert!(!shipped.contains("\"auto_retry_overflow\""));
    assert!(!shipped.contains("\"modelBacked\""));
    assert!(!shipped.contains("\"splitOversizedTurns\""));
    assert!(!shipped.contains("\"autoRetryOverflow\""));
}
#[test]
fn public_runtime_config_accepts_top_level_skills() {
    let parsed: PublicRuntimeConfig = json5::from_str(
        r#"
        {
          skills: {
            walkToGitRoot: false,
            permissions: {
              "*": "allow"
            }
          }
        }
        "#,
    )
    .expect("parse runtime config with top-level skills");

    assert!(!parsed.skills.walk_to_git_root);
    assert_eq!(
        parsed.skills.permissions.get("*"),
        Some(&PermissionMode::Allow)
    );
}
