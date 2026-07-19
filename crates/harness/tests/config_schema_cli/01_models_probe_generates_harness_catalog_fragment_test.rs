use harness::UnwrapOrAbort;
#[test]
fn models_probe_generates_harness_catalog_fragment_from_models_dev_json() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let source_path = temp.path().join("models-dev.json");
    write_json(
        &source_path,
        &serde_json::json!({
            "openai": {
                "id": "openai",
                "name": "OpenAI",
                "env": ["OPENAI_API_KEY"],
                "npm": "@ai-sdk/openai",
                "doc": "https://platform.openai.com/docs",
                "api": "https://api.openai.com/v1",
                "models": {
                    "gpt-5-mini": {
                        "id": "gpt-5-mini",
                        "name": "GPT-5 mini",
                        "attachment": true,
                        "reasoning": true,
                        "temperature": false,
                        "tool_call": true,
                        "structured_output": true,
                        "release_date": "2026-01-15",
                        "last_updated": "2026-02-01",
                        "cost": {
                            "input": 0.25,
                            "output": 2.0,
                            "cache_read": 0.025,
                            "cache_write": 0.25
                        },
                        "limit": {
                            "context": 128000,
                            "input": 120000,
                            "output": 16000
                        },
                        "modalities": {
                            "input": ["text", "image"],
                            "output": ["text"]
                        }
                    },
                    "gpt-old": {
                        "id": "gpt-old",
                        "name": "GPT old",
                        "reasoning": false,
                        "tool_call": true,
                        "status": "deprecated",
                        "limit": { "context": 4096, "output": 1024 }
                    },
                    "gpt-text-only": {
                        "id": "gpt-text-only",
                        "name": "GPT text only",
                        "reasoning": false,
                        "tool_call": false,
                        "limit": { "context": 8192, "output": 2048 }
                    }
                }
            },
            "anthropic": {
                "id": "anthropic",
                "name": "Anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "models": {
                    "claude-sonnet": {
                        "id": "claude-sonnet",
                        "name": "Claude Sonnet",
                        "reasoning": true,
                        "tool_call": true,
                        "limit": { "context": 200000, "output": 8192 }
                    }
                }
            }
        }),
    );

    let output = harness_command()
        .args([
            "models",
            "probe",
            "--input",
            source_path.to_str().unwrap_or_abort(),
            "--provider",
            "openai",
            "--emit-reasoning-variants",
        ])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let catalog: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(catalog["$schema"], "./config.json");
    assert!(catalog["provider"].get("anthropic").is_none());

    let openai = &catalog["provider"]["openai"];
    assert_eq!(openai["type"], "openai_compatible");
    assert_eq!(openai["name"], "OpenAI");
    assert_eq!(openai["options"]["baseURL"], "https://api.openai.com/v1");
    assert_eq!(openai["options"]["apiKeyEnv"][0], "OPENAI_API_KEY");

    let models = openai["models"].as_object().unwrap_or_abort();
    assert!(models.contains_key("gpt-5-mini"));
    assert!(!models.contains_key("gpt-old"));
    assert!(!models.contains_key("gpt-text-only"));

    let model = &models["gpt-5-mini"];
    assert_eq!(model["metadata"]["contextWindowTokens"], 128000);
    assert_eq!(model["metadata"]["supportsToolCalls"], true);
    assert_eq!(model["limit"]["input"], 120000);
    assert_eq!(model["limit"]["output"], 16000);
    assert_eq!(model["modalities"]["input"][1], "image");
    assert_eq!(
        model["options"]["modelsDev"]["source"],
        format!("file://{}", source_path.display())
    );
    assert_eq!(
        model["options"]["modelsDev"]["capabilities"]["reasoning"],
        true
    );
    assert_eq!(model["options"]["modelsDev"]["cost"]["cache_read"], 0.025);
    assert_eq!(
        model["variants"]["high"]["metadata"]["reasoningEffort"],
        "high"
    );
}
#[test]
fn models_generate_updates_static_catalog_artifact_from_models_dev_json() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let source_path = temp.path().join("models-dev.json");
    let output_path = temp.path().join("provider-catalog.generated.json");
    write_json(
        &source_path,
        &serde_json::json!({
            "openai": {
                "id": "openai",
                "name": "OpenAI",
                "env": ["OPENAI_API_KEY"],
                "api": "https://api.openai.com/v1",
                "models": {
                    "gpt-5-mini": {
                        "id": "gpt-5-mini",
                        "name": "GPT-5 mini",
                        "reasoning": true,
                        "tool_call": true,
                        "limit": { "context": 128000, "input": 120000, "output": 16000 },
                        "modalities": { "input": ["text"], "output": ["text"] }
                    }
                }
            }
        }),
    );

    let output = harness_command()
        .args([
            "models",
            "generate",
            "--input",
            source_path.to_str().unwrap_or_abort(),
            "--output",
            output_path.to_str().unwrap_or_abort(),
            "--emit-reasoning-variants",
        ])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "generate should write the artifact instead of stdout"
    );

    let generated = fs::read_to_string(&output_path).unwrap_or_abort();
    let compact: Value = serde_json::from_str(&generated).unwrap_or_abort();
    assert_eq!(
        generated.matches('\n').count(),
        1,
        "generate should produce one compact JSON line"
    );
    assert!(
        generated.ends_with('\n'),
        "generate should preserve a trailing newline"
    );
    assert_eq!(
        compact["provider"]["openai"]["models"]["gpt-5-mini"]["name"],
        "GPT-5 mini"
    );
    assert_eq!(
        compact["provider"]["openai"]["models"]["gpt-5-mini"]["variants"]["medium"]["metadata"]
            ["reasoningEffort"],
        "medium"
    );
}
#[test]
fn models_generated_prints_embedded_static_catalog() {
    // arrange
    // act
    // assert
    let output = harness_command()
        .args(["models", "generated"])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let catalog: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(catalog["$schema"], "./config.json");
    assert!(catalog["provider"]
        .as_object()
        .unwrap_or_abort()
        .contains_key("openai"));
    serde_json::from_slice::<PublicRuntimeConfig>(&output.stdout)
        .unwrap_or_abort();
}
#[test]
fn schema_cli_prints_runtime_json_schema() {
    // arrange
    // act
    // assert
    let output = harness_command()
        .arg("schema")
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let schema: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let root_properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(root_properties.contains_key("provider"));
    assert!(root_properties.contains_key("model"));
    assert!(root_properties.contains_key("small_model"));
    assert!(root_properties.contains_key("model_profile"));
    assert!(root_properties.contains_key("permission"));
    assert!(root_properties.contains_key("mcp"));
    assert!(root_properties.contains_key("runtime"));
    assert!(root_properties.contains_key("mode"));
    assert!(root_properties.contains_key("server"));
    assert!(root_properties.contains_key("command"));
    assert!(root_properties.contains_key("formatter"));
    assert!(!root_properties.contains_key("tool_output"));
    assert!(!root_properties.contains_key("integrations"));

    let definitions = schema
        .get("definitions")
        .and_then(Value::as_object)
        .unwrap_or_abort();

    let agent_map_properties = definitions["PublicAgentMap"]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    for agent in [
        "build",
        "plan",
        "general",
        "explore",
        "visual-engineering",
        "artistry",
        "ultrabrain",
        "deep",
        "quick",
        "unspecified-low",
        "unspecified-high",
        "writing",
        "title",
        "summary",
        "compaction",
    ] {
        assert!(
            agent_map_properties.contains_key(agent),
            "agent schema should expose built-in agent `{agent}`"
        );
    }
    assert_eq!(
        definitions["PublicAgentMap"]
            .get("additionalProperties")
            .and_then(|value| value.get("$ref"))
            .and_then(Value::as_str),
        Some("#/definitions/PublicAgentConfig"),
        "agent schema should type custom agent names as PublicAgentConfig"
    );

    let model_properties = definitions["ModelConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(model_properties.contains_key("name"));
    assert!(model_properties.contains_key("limit"));
    assert!(model_properties.contains_key("modalities"));
    assert!(model_properties.contains_key("options"));
    assert!(model_properties.contains_key("variants"));
    assert!(!model_properties.contains_key("reasoningEfforts"));
    assert!(!model_properties.contains_key("display_name"));

    let variant_properties = definitions["ModelVariantConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(variant_properties.contains_key("name"));
    assert!(variant_properties.contains_key("limit"));
    assert!(variant_properties.contains_key("modalities"));
    assert!(variant_properties.contains_key("options"));
    assert!(!variant_properties.contains_key("display_name"));

    let provider_properties = definitions["ProviderConfig"]["oneOf"][0]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(provider_properties.contains_key("baseURL"));
    assert!(provider_properties.contains_key("apiKey"));
    assert!(provider_properties.contains_key("apiKeyEnv"));
    assert!(provider_properties.contains_key("authProvider"));
    assert!(provider_properties.contains_key("apiMode"));
    assert!(provider_properties.contains_key("cacheRetention"));
    assert!(provider_properties.contains_key("timeoutMs"));
    assert!(!provider_properties.contains_key("base_url"));
    assert!(!provider_properties.contains_key("api_key"));

    let options_properties = definitions["OpenAiCompatibleProviderOptions"]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(options_properties.contains_key("baseURL"));
    assert!(options_properties.contains_key("apiKey"));
    assert!(options_properties.contains_key("apiKeyEnv"));
    assert!(options_properties.contains_key("authProvider"));
    assert!(options_properties.contains_key("apiMode"));
    assert!(options_properties.contains_key("cacheRetention"));
    assert!(options_properties.contains_key("timeoutMs"));
    assert!(!options_properties.contains_key("base_url"));
    assert!(!options_properties.contains_key("api_key"));

    let permission_properties = definitions["PublicPermissionConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(permission_properties.contains_key("bash"));
    assert!(permission_properties.contains_key("task"));
    assert!(!permission_properties.contains_key("shell"));
    assert!(!permission_properties.contains_key("network"));

    let agent_properties = definitions["PublicAgentConfig"]
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    for key in [
        "name",
        "description",
        "system_prompt",
        "model",
        "top_p",
        "mode",
        "hidden",
        "color",
        "options",
        "enable",
        "disable",
        "max_iters",
        "tools",
    ] {
        assert!(
            agent_properties.contains_key(key),
            "missing agent schema key {key}"
        );
    }
}
#[test]
fn schema_cli_prints_tui_json_schema() {
    // arrange
    // act
    // assert
    let output = harness_command()
        .args(["schema", "--tui"])
        .output()
        .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let schema: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_abort();
    assert!(properties.contains_key("keybinds"));
    assert!(!properties.contains_key("parity"));
    assert!(!properties.contains_key("default_profile"));
}
#[test]
fn config_validate_cli_reports_missing_config() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        .unwrap_or_abort();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(stderr.contains("./harness.jsonc"));
    assert!(stderr.contains("./harness.json"));
    assert!(stderr.contains("$XDG_CONFIG_HOME/harness/harness.jsonc"));
    assert!(stderr.contains("configs/harness.example.jsonc"));
}
#[test]
fn config_validate_cli_accepts_shipped_example_config() {
    // arrange
    // act
    // assert
    let repo_root = repo_root();
    let config_path = repo_root.join("configs").join("harness.example.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
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
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains("configs/harness.example.jsonc"));

    let parsed = load_config_from_file(&config_path).unwrap_or_abort();
    assert_eq!(parsed.providers.len(), 1);
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("openai-codex")
        .unwrap_or_abort();
    assert_eq!(provider.models.len(), 2);
    assert!(provider.models.contains_key("gpt-5.5"));
    assert!(provider.models.contains_key("gpt-5.4-mini"));
    assert!(parsed.agents.contains_key("build"));
    for category in [
        "visual-engineering",
        "artistry",
        "ultrabrain",
        "deep",
        "quick",
        "unspecified-low",
        "unspecified-high",
        "writing",
    ] {
        let agent = parsed
            .agents
            .get(category)
            .unwrap_or_else(|| panic!("missing category route profile {category}"));
        assert_eq!(agent.mode, AgentMode::Subagent);
        assert!(!agent.hidden);
        assert_eq!(
            agent
                .permissions
                .as_ref()
                .and_then(|permissions| permissions.task.as_ref()),
            Some(&PermissionMode::Deny),
            "category route {category} should not recursively redelegate by default"
        );
    }
    assert!(parsed.runtime.compaction.enabled);
    assert!(parsed.runtime.compaction.auto_retry_overflow);
    assert!(parsed.runtime.compaction.structured_summary_contract);
    assert!(parsed.runtime.compaction.estimated_token_triggers);
    assert_eq!(parsed.runtime.compaction.fallback_input_tokens, 32_768);
}
