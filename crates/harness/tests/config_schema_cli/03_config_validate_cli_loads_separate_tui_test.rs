use harness::UnwrapOrAbort;
#[test]
fn config_validate_cli_loads_separate_tui_config() {
    let temp = tempdir().unwrap_or_abort();
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
        .unwrap_or_abort();

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
    let temp = tempdir().unwrap_or_abort();
    let env_config_path = temp.path().join("env/custom-harness.jsonc");
    fs::create_dir_all(env_config_path.parent().unwrap_or_abort())
        .unwrap_or_abort();
    write_config(&env_config_path, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_CONFIG", &env_config_path)
        .args(["config", "validate"])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains(env_config_path.to_str().unwrap_or_abort()));
}
#[test]
fn config_validate_cli_applies_harness_config_content_last() {
    let temp = tempdir().unwrap_or_abort();
    write_config(
        &temp.path().join("harness.jsonc"),
        &canonical_runtime_config(),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env(
            "HARNESS_CONFIG_CONTENT",
            r#"{ model: "default/gpt-4o", permission: { bash: "deny" } }"#,
        )
        .args(["config", "validate"])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn config_validate_cli_explicit_path_bypasses_discovery_layers() {
    let temp = tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let explicit_path = temp.path().join("explicit.jsonc");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");

    fs::create_dir_all(xdg_config_path.parent().unwrap_or_abort())
        .unwrap_or_abort();
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
            explicit_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
        ])
        .output()
        .unwrap_or_abort();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys"));
    assert!(stderr.contains("`mystery`"));
}
#[test]
fn config_validate_cli_rejects_unknown_top_level_keys() {
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["mystery"] = serde_json::json!({"enabled": true});
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .unwrap_or_abort();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown top-level config keys:"));
    assert!(stderr.contains("`mystery`"));
}
#[test]
fn config_validate_cli_rejects_unknown_provider_reference() {
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["agent"] = serde_json::json!({
        "default": {
            "model": "missing/gpt-4o-mini",
            "tools": []
        }
    });
    write_config(&config_path, &config);

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .unwrap_or_abort();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agent `default` has invalid model selection `missing/gpt-4o-mini`"));
    assert!(stderr.contains("unknown provider `missing`"));
}
#[test]
fn config_validate_cli_rejects_unsupported_upstream_top_level_keys() {
    for key in [
        "server",
        "command",
        "share",
        "autoshare",
        "autoupdate",
        "enterprise",
    ] {
        let temp = tempdir().unwrap_or_abort();
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
            stderr.contains("unsupported active retired config keys:"),
            "unsupported key {key} stderr did not mention unsupported active retired config keys:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("`{key}`")),
            "unsupported key {key} stderr did not name the key:\n{stderr}"
        );
    }
}
#[test]
fn compatibility_config_shape_accepts_subagents_and_safe_inert_keys() {
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
          skills: { paths: [".harness/skill"], urls: ["https://example.test/skills"] },
          mcp: {
            local_docs: {
              type: "local",
              command: ["docs-mcp"],
              enabled: false
            },
            disabled_only: { enabled: false }
          },
          agent: {
            default: {},
            general: {
              options: { reasoningEffort: "high" }
            },
            explore: {},
            librarian: {}
          },
          permission: "allow"
        }
        "#,
    )
    .unwrap_or_abort();

    assert!(parsed.agents.contains_key("default"));
    assert_eq!(
        parsed.agents["general"].mode,
        harness_core::config::AgentMode::Subagent
    );
    assert_eq!(
        parsed.agents["general"].options.get("reasoningEffort"),
        Some(&serde_json::json!("high"))
    );
    assert_eq!(parsed.agents.len(), 4);
    assert!(parsed.lsp.disabled);
    assert!(parsed
        .skills
        .project_roots
        .contains(&Path::new(".harness/skill").to_path_buf()));
    assert!(parsed.integrations.mcp.servers.contains_key("local_docs"));
    assert!(!parsed
        .integrations
        .mcp
        .servers
        .contains_key("disabled_only"));
}
#[test]
fn shipped_example_config_uses_codex_oauth_provider_without_openai_api_key() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let parsed = load_config_from_file(&config_path)
        .unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("openai-codex")
        .unwrap_or_abort() else {
        panic!("expected openai-codex provider to be OpenAiCompatible")
    };

    assert_eq!(provider.name.as_deref(), Some("OpenAI Codex"));
    assert_eq!(provider.base_url, "https://api.openai.com/v1");
    assert_eq!(provider.api_key, "");
    assert_eq!(provider.api_key_env, vec!["OPENAI_API_KEY".to_string()]);
    assert_eq!(
        format!("{:?}", provider.auth_provider),
        "Some(ProviderId(\"codex\"))"
    );
    assert_eq!(provider.timeout_ms, 1_800_000);
    assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
}
#[test]
fn shipped_example_config_keeps_codex_oauth_provider_even_when_openai_api_key_is_set() {
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let parsed = load_config_from_file(&config_path)
        .unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("openai-codex")
        .unwrap_or_abort() else {
        panic!("expected openai-codex provider to be OpenAiCompatible")
    };

    assert_eq!(provider.name.as_deref(), Some("OpenAI Codex"));
    assert_eq!(provider.base_url, "https://api.openai.com/v1");
    assert_eq!(provider.api_key, "");
    assert_eq!(provider.api_key_env, vec!["OPENAI_API_KEY".to_string()]);
    assert_eq!(
        format!("{:?}", provider.auth_provider),
        "Some(ProviderId(\"codex\"))"
    );
    assert_eq!(provider.timeout_ms, 1_800_000);
    assert!(matches!(provider.api_mode, OpenAiApiMode::Auto));
}
#[test]
fn checked_in_tui_schema_matches_generated_schema() {
    let generated = harness_tui_schema_pretty_json().unwrap_or_abort();
    let checked_in = fs::read_to_string(repo_root().join("configs").join("tui.json"))
        .unwrap_or_abort();

    assert_eq!(checked_in.trim(), generated.trim());
}
#[test]
fn checked_in_runtime_schema_matches_generated_schema() {
    let generated = harness_schema_pretty_json().unwrap_or_abort();
    let checked_in = fs::read_to_string(repo_root().join("configs").join("config.json"))
        .unwrap_or_abort();

    assert_eq!(checked_in.trim(), generated.trim());
}
#[test]
fn shipped_tui_example_parses_as_public_tui_config() {
    let shipped = fs::read_to_string(repo_root().join("configs").join("tui.example.jsonc"))
        .unwrap_or_abort();

    let parsed: PublicTuiConfig = json5::from_str(&shipped).unwrap_or_abort();

    assert_eq!(
        parsed.keybindings.get("variant_cycle"),
        Some(&"ctrl+t".to_string())
    );
    assert_eq!(
        parsed.keybindings.get("session_child_first"),
        Some(&"<leader>down".to_string())
    );
}
#[test]
fn shipped_runtime_example_parses_as_public_runtime_config() {
    let shipped = fs::read_to_string(repo_root().join("configs").join("harness.example.jsonc"))
        .unwrap_or_abort();

    let parsed: PublicRuntimeConfig =
        json5::from_str(&shipped).unwrap_or_abort();

    assert_eq!(parsed.model.as_deref(), Some("openai-codex/gpt-5.4-mini"));
    assert_eq!(parsed.small_model.as_deref(), None);
    assert_eq!(parsed.provider.len(), 1);
    assert!(parsed.provider.contains_key("openai-codex"));
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .provider
        .get("openai-codex")
        .unwrap_or_abort() else {
        panic!("expected openai-codex provider to be OpenAiCompatible")
    };
    assert_eq!(provider.models.len(), 2);
    assert!(provider.models.contains_key("gpt-5.5"));
    assert!(provider.models.contains_key("gpt-5.4-mini"));
    assert_eq!(parsed.agent.default.variant.as_deref(), Some("high"));
    assert!(parsed.agent.explore.model.is_none());
    assert!(parsed.agent.general.model.is_none());
    assert!(parsed.agent.librarian.model.is_none());
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
    .unwrap_or_abort();

    assert!(!parsed.skills.walk_to_git_root);
    assert_eq!(
        parsed.skills.permissions.get("*"),
        Some(&PermissionMode::Allow)
    );
}
