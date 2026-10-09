use super::*;
use harness_core::auth::StoredCredential;

#[test]
fn default_model_preferences_and_catalog_fallback() -> Result<(), Box<dyn std::error::Error>> {
    type Candidate = (&'static str, Option<bool>, Option<&'static str>);
    let cases: &[(&str, &[Candidate], Option<&str>)] = &[
        (
            "preference order wins over release date",
            &[
                ("gpt-5.4-mini", Some(true), Some("2025-01-01")),
                ("gpt-5.5", Some(true), Some("2026-01-01")),
                ("newest", Some(true), Some("2026-09-01")),
            ],
            Some("gpt-5.4-mini"),
        ),
        (
            "newest tool-capable model wins",
            &[
                ("aardvark", Some(true), Some("2025-12-31")),
                ("zeta", Some(true), Some("2026-01-01")),
            ],
            Some("zeta"),
        ),
        (
            "non-tool and unknown-capability models are skipped",
            &[
                ("aardvark", Some(false), Some("2026-09-01")),
                ("beta", None, Some("2026-09-01")),
                ("zeta", Some(true), Some("2025-01-01")),
            ],
            Some("zeta"),
        ),
        (
            "missing dates fall back alphabetically",
            &[("zeta", Some(true), None), ("aardvark", Some(true), None)],
            Some("aardvark"),
        ),
        (
            "dated models precede undated models",
            &[
                ("aardvark", Some(true), None),
                ("zeta", Some(true), Some("2025-01-01")),
            ],
            Some("zeta"),
        ),
        (
            "equal release dates fall back alphabetically",
            &[
                ("zeta", Some(true), Some("2026-01-01")),
                ("aardvark", Some(true), Some("2026-01-01")),
            ],
            Some("aardvark"),
        ),
        (
            "fallback skips free, small and alpha tokens but not substrings",
            &[
                ("family:free", Some(true), Some("2026-09-01")),
                ("family/nano", Some(true), Some("2026-09-01")),
                ("family.lite", Some(true), Some("2026-09-01")),
                ("family:mini", Some(true), Some("2026-09-01")),
                ("family_mini", Some(true), Some("2026-09-01")),
                ("family-mini", Some(true), Some("2026-09-01")),
                ("family-alpha", Some(true), Some("2026-09-01")),
                ("minimax", Some(true), Some("2025-01-01")),
            ],
            Some("minimax"),
        ),
        (
            "fallback uses unfiltered candidates when all are small or free",
            &[
                ("family-lite", Some(true), Some("2025-01-01")),
                ("family/nano:free", Some(true), Some("2026-01-01")),
            ],
            Some("family/nano:free"),
        ),
        (
            "no tool-capable models still use release dates",
            &[
                ("aardvark", Some(false), Some("2025-01-01")),
                ("beta", None, Some("2026-01-01")),
            ],
            Some("beta"),
        ),
        (
            "custom models without capability metadata remain usable",
            &[("my-model", None, None)],
            Some("my-model"),
        ),
    ];
    for (label, candidates, expected) in cases {
        let raw_models: BTreeMap<_, _> = candidates
            .iter()
            .map(|(name, tool_calls, date)| {
                (
                    *name,
                    serde_json::json!({
                        "limit": {"context": 1000, "output": 100},
                        "metadata": {"supportsToolCalls": tool_calls},
                        "options": {"modelsDev": {"model": {"releaseDate": date}}},
                    }),
                )
            })
            .collect();
        let body = serde_json::json!({"provider": {"example": {"models": raw_models}}});
        let catalog = ProviderCatalog::from_json(&body.to_string())?;
        let source = catalog
            .provider("example")
            .ok_or("missing fixture provider")?;
        let provider = ProviderConfig::OpenAiCompatible(compatible(source));
        assert_eq!(
            default_model_for_provider("example", &provider),
            *expected,
            "{label}"
        );
    }
    let empty = ProviderConfig::OpenAiCompatible(Default::default());
    assert_eq!(default_model_for_provider("example", &empty), None);
    Ok(())
}

#[test]
fn embedded_catalog_default_models() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = ProviderCatalog::from_embedded()?;
    for (id, expected) in [
        ("google", "gemini-3.1-pro-preview"),
        ("openrouter", "anthropic/claude-sonnet-4.6"),
        ("mistral", "mistral-medium-2604"),
        ("deepseek", "deepseek-v4-flash"),
        ("groq", "moonshotai/kimi-k2-instruct-0905"),
    ] {
        let source = catalog.provider(id).ok_or("missing embedded provider")?;
        let provider = ProviderConfig::OpenAiCompatible(compatible(source));
        let selected = default_model_for_provider(id, &provider);
        assert_eq!(selected, Some(expected), "{id}");
        eprintln!("{id}: {expected}");
    }
    Ok(())
}

fn load_context(root: &std::path::Path) -> ConfigLoadContext {
    ConfigLoadContext {
        discovery: ConfigDiscoveryContext {
            current_dir: root.join("project"),
            harness_home: None,
            home: None,
            data_dir: Some(root.join("data/harness")),
            runtime_config_path: None,
            tui_config_path: None,
        },
        runtime_content: None,
    }
}

#[test]
fn runtime_defaults_discover_workspace_layers_and_only_curate_configured_providers(
) -> Result<(), Box<dyn std::error::Error>> {
    for (content, api_key, curated, connected) in [
        (None, false, false, false),
        (None, true, false, true),
        (Some(r#"{"permission":{"edit":"ask"}}"#), true, false, true),
        (
            Some(
                r#"{"provider":{"local":{"type":"openai_compatible","apiKeyEnv":["LOCAL_API_KEY"],"models":{"chosen":{}}}}}"#,
            ),
            true,
            true,
            false,
        ),
    ] {
        let root = tempfile::tempdir()?;
        let context = load_context(root.path());
        let project = &context.discovery.current_dir;
        std::fs::create_dir_all(project.join(".git"))?;
        std::fs::write(project.join("AGENTS.md"), "Workspace-specific instructions")?;
        std::fs::write(
            project.join("tui.jsonc"),
            "{keybinds:{copy_selection:'ctrl+y'}}",
        )?;
        if let Some(content) = content {
            let global = root.path().join("data/harness");
            std::fs::create_dir_all(&global)?;
            std::fs::write(global.join("harness.jsonc"), content)?;
        }
        let lookup =
            |name: &str| (api_key && name == "ANTHROPIC_API_KEY").then(|| "fixture-key".into());
        let base = load_resolved_config_with_lookup(None, &context, &lookup)?;
        let store = CredentialStore::new(root.path().join("data/harness"));
        let result = resolve_runtime_catalog(
            base.map(|loaded| loaded.config),
            None,
            None,
            &context,
            Some(&store),
            &lookup,
        )?;
        assert_eq!(result.curated, curated);
        assert_eq!(result.config.providers.contains_key("anthropic"), connected);
        assert_eq!(result.no_provider_connected, !connected);
        assert!(result.config.instruction_files.iter().any(|instruction| {
            instruction.path == project.join("AGENTS.md")
                && instruction.content == "Workspace-specific instructions"
        }));
        assert_eq!(result.config.ui.keybindings["copy_selection"], "ctrl+y");
        if connected {
            assert_eq!(
                result.config.agents["default"].model_ref,
                "anthropic:claude-sonnet-4-6"
            );
        }
    }
    Ok(())
}

#[test]
fn connected_subscriptions_extend_explicit_config_without_replacing_its_model(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = load_context(root.path());
    let store = CredentialStore::new(root.path());
    for id in [ProviderId::codex(), ProviderId::github_copilot()] {
        store.save(&StoredCredential::oauth(
            id,
            "fixture-access",
            "fixture-refresh",
            None,
            "2026-09-26T00:00:00Z",
        ))?;
    }
    let config = load_config_from_str(
        r#"{"provider":{"local":{"type":"openai_compatible","models":{"chosen":{}}}},"model":"local/chosen"}"#,
    )?;
    let result =
        resolve_runtime_catalog(Some(config), None, None, &context, Some(&store), &|_| None)?;
    assert_eq!(result.config.agents["default"].model_ref, "local:chosen");
    assert!(result
        .connected_provider_ids
        .contains(&BUILTIN_COPILOT_PROVIDER_ID.into()));
    let codex = &result.config.providers[BUILTIN_CODEX_PROVIDER_ID];
    assert!(codex.models().contains_key("gpt-6-astra"));
    assert!(codex
        .models()
        .keys()
        .all(|model| harness_providers::codex_model_allowed(model)));
    let selected =
        resolve_model_selection(&result.config, "openai-codex:gpt-6.1-sol", Some("max"))?;
    assert_eq!(selected.primary.reasoning_effort.as_deref(), Some("max"));
    assert_eq!(selected.primary.reasoning_summary.as_deref(), Some("auto"));
    assert!(selected.primary.resolution.capabilities.supports_vision);
    assert!(selected
        .primary
        .resolution
        .capabilities
        .variants
        .contains(&"max".into()));
    let mut config = result.config;
    let ProviderConfig::OpenAiCompatible(codex) = config
        .providers
        .get_mut(BUILTIN_CODEX_PROVIDER_ID)
        .ok_or("missing Codex")?
    else {
        return Err("wrong provider".into());
    };
    let explicit_astra = codex.models["gpt-6-astra"].clone();
    codex.models.remove("gpt-6-sol");
    let cache = root.path().join("catalog.json");
    std::fs::write(
        &cache,
        r#"{"openai":{"models":{
            "gpt-6-astra":{"limit":{"context":10000,"output":200}},
            "gpt-6-sol":{"limit":{"context":22222,"output":4444}}
        }}}"#,
    )?;
    let result =
        resolve_runtime_catalog(Some(config), None, None, &context, Some(&store), &|name| {
            (name == "HARNESS_MODELS_PATH").then(|| cache.to_string_lossy().into_owned())
        })?;
    let models = result.config.providers[BUILTIN_CODEX_PROVIDER_ID].models();
    assert_eq!(models["gpt-6-sol"].limit.context, Some(22222));
    assert_eq!(
        serde_json::to_value(&models["gpt-6-astra"])?,
        serde_json::to_value(explicit_astra)?
    );
    assert_eq!(result.config.agents["default"].model_ref, "local:chosen");
    Ok(())
}
