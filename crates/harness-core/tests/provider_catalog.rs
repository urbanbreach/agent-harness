use harness_core::config::BilledTokens;
use harness_core::provider_catalog::{duplicate_checked_json_value, ProviderCatalog};

#[test]
fn catalog_rejects_ambiguous_limits_and_preserves_unknown_capabilities(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("models.json");
    let raw = serde_json::json!({"provider": {"example": {"name": "Example", "options": {"baseURL": "https://example.test/v1", "apiKeyEnv": ["EXAMPLE_KEY"]}, "models": {
        "valid": {"limit": {"context": 1000, "input": 700, "output": 200}},
        "unknown": {},
        "overflow": {"limit": {"context": 4294967296_u64, "output": 1}},
        "invalid": {"limit": {"context": 10, "output": 20}}
    }}}});
    std::fs::write(&path, raw.to_string())?;
    let catalog = ProviderCatalog::from_path(&path)?;
    let provider = catalog.provider("example").ok_or("provider missing")?;
    assert_eq!(provider.base_url, "https://example.test/v1");
    assert_eq!(provider.api_key_env, ["EXAMPLE_KEY"]);
    assert_eq!(provider.models.len(), 1);
    assert_eq!(catalog.diagnostics().len(), 3);
    assert!(catalog.validated_model("example", "unknown").is_err());
    assert_eq!(
        catalog
            .validated_model("example", "valid")?
            .definition
            .metadata
            .supports_tool_calls,
        None
    );
    assert_eq!(
        catalog
            .validated_model("example", "valid")?
            .limits
            .max_input_tokens(),
        Some(700)
    );
    assert!(catalog.validated_model("example", "overflow").is_err());
    assert!(duplicate_checked_json_value(r#"{"outer":{"x":1,"x":2}}"#).is_err());
    std::fs::write(
        &path,
        r#"{"example":{"models":{"bad":{"limit":{"context":0,"output":1}}}}}"#,
    )?;
    assert!(ProviderCatalog::from_path(&path).is_err());
    Ok(())
}

#[test]
fn catalog_prices_models_from_models_dev_cost() -> Result<(), Box<dyn std::error::Error>> {
    // The generated catalog nests models.dev prices under options; models.dev itself
    // puts them at the top level.
    let limit = serde_json::json!({"context": 1000, "output": 100});
    let tier = serde_json::json!({"type": "context", "size": 272_000});
    let raw = serde_json::json!({"provider": {"example": {"models": {
        "generated": {"limit": limit, "options": {"modelsDev": {"cost": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75}}}},
        "upstream": {"limit": limit, "cost": {"input": 3, "output": 15}},
        "partial": {"limit": limit, "cost": {"input": 3}},
        "tiered": {"limit": limit, "cost": {"input": 2, "output": 10, "cache_read": 0.1, "reasoning": 10,
            "tiers": [{"input": 4, "output": 15, "cache_read": 0.2, "tier": tier}]}},
        "unknown_tier": {"limit": limit, "cost": {"input": 2, "output": 10,
            "tiers": [{"input": 4, "output": 15, "tier": {"type": "time", "size": 1}}]}},
    }}, "deepseek": {"models": {
        // models.dev lists DeepSeek's off-peak price; Flash's no longer matches the checked one.
        "deepseek-v4-pro": {"limit": limit, "cost": {"input": 0.66, "output": 1.98, "cache_read": 0.022}},
        "deepseek-flash": {"limit": limit, "cost": {"input": 0.3, "output": 1.2, "cache_read": 0.006}},
    }}}});
    let catalog = ProviderCatalog::from_json(&raw.to_string())?;
    let million = BilledTokens {
        input: 1_000_000,
        output: 1_000_000,
        cache_read: 1_000_000,
        cache_write: 1_000_000,
        cache_write_1h: 0,
    };
    // A 272,000-token prompt stays on base prices; one token more reprices the whole request.
    let at_threshold = BilledTokens {
        input: 72_000,
        output: 1_000,
        cache_read: 200_000,
        ..BilledTokens::default()
    };
    let over_threshold = BilledTokens {
        input: 72_001,
        ..at_threshold
    };
    let writes = BilledTokens {
        cache_write: 1_000_000,
        cache_write_1h: 400_000,
        ..BilledTokens::default()
    };
    let picodollars = |model: &str, tokens: &BilledTokens, at: Option<&str>| {
        let (provider, model) = model.split_once(':').unwrap_or(("example", model));
        let at = at
            .map(humantime::parse_rfc3339)
            .transpose()?
            .map(|at| at.duration_since(std::time::UNIX_EPOCH))
            .transpose()?
            .map(|since| i64::try_from(since.as_millis()))
            .transpose()?;
        let entry = catalog.validated_model(provider, model)?;
        Ok::<_, Box<dyn std::error::Error>>(
            entry
                .definition
                .cost
                .as_ref()
                .and_then(|cost| cost.picodollars(tokens, at)),
        )
    };
    // $3 + $15 + $0.30 + $3.75; cache tokens without a price bill at the input rate.
    assert_eq!(
        picodollars("generated", &million, None)?,
        Some(22_050_000_000_000)
    );
    assert_eq!(
        picodollars("upstream", &million, None)?,
        Some(24_000_000_000_000)
    );
    assert_eq!(picodollars("partial", &million, None)?, None);
    assert_eq!(picodollars("unknown_tier", &million, None)?, None);
    assert_eq!(
        picodollars("tiered", &at_threshold, None)?,
        Some(174_000_000_000)
    );
    assert_eq!(
        picodollars("tiered", &over_threshold, None)?,
        Some(343_004_000_000)
    );
    // 600K five-minute writes at $3.75 and 400K one-hour writes at twice the $3 input.
    assert_eq!(
        picodollars("generated", &writes, None)?,
        Some(4_650_000_000_000)
    );
    // DeepSeek's peak is $1.32 + $3.96 + $0.044 from 01:00 to 04:00 and 06:00 to 10:00 UTC
    // on weekdays (2026-10-12 is a Monday); other hours bill half, and an unknown start
    // time has no price.
    let uncached_writes = BilledTokens {
        cache_write: 0,
        ..million
    };
    for (at, expected) in [
        (Some("2026-10-12T01:00:00Z"), Some(5_324_000_000_000)),
        (Some("2026-10-12T04:00:00Z"), Some(2_662_000_000_000)),
        (Some("2026-10-11T02:00:00Z"), Some(2_662_000_000_000)),
        (None, None),
    ] {
        assert_eq!(
            picodollars("deepseek:deepseek-v4-pro", &uncached_writes, at)?,
            expected,
            "{at:?}"
        );
    }
    // A changed listed price bills as listed at every hour, with a catalog warning.
    assert_eq!(
        picodollars("deepseek:deepseek-flash", &uncached_writes, None)?,
        Some(1_506_000_000_000)
    );
    assert_eq!(
        catalog
            .diagnostics()
            .iter()
            .map(|d| d.model.as_str())
            .collect::<Vec<_>>(),
        ["deepseek-flash"]
    );
    Ok(())
}

#[tokio::test]
async fn catalog_cache_reuses_valid_data_and_preserves_files_on_bad_refresh(
) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let temp = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!(
        "http://{}/api.json?token=private-source",
        listener.local_addr()?
    );
    let cache = temp.path().join("models.json");
    // Refreshes lock the cache's directory, so the second cache gets its own.
    let invalid = temp.path().join("other/invalid.json");
    std::fs::create_dir_all(temp.path().join("other"))?;
    std::fs::write(&invalid, "previous-invalid-data")?;
    let valid = r#"{"example":{"models":{"small":{"limit":{"context":1000,"output":200}}}}}"#;
    let (requested, request_seen) = std::sync::mpsc::channel();
    let server = async move {
        for body in [valid, "invalid-response"] {
            let (mut socket, _) = listener.accept().await?;
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await?;
            let _ = requested.send(());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await?;
        }
        Ok::<_, std::io::Error>(())
    };
    let root = temp.path().to_owned();
    let client = tokio::task::spawn_blocking(move || -> Result<(), String> {
        // A missing cache answers with the bundled catalog at once and downloads behind it.
        let first = ProviderCatalog::cached(&cache, Some(&url)).map_err(|e| e.to_string())?;
        assert!(first.provider("anthropic").is_some());
        request_seen
            .recv_timeout(Duration::from_secs(4))
            .map_err(|_| "no background refresh")?;
        // The refresh holds its directory's lock from before the request until the write.
        std::fs::File::open(&root)
            .and_then(|dir| dir.lock())
            .map_err(|e| e.to_string())?;
        let catalog = ProviderCatalog::cached(&cache, Some(&url)).map_err(|e| e.to_string())?;
        let entry = catalog
            .validated_model("example", "small")
            .map_err(|e| e.to_string())?;
        assert!(!entry
            .limits
            .primary_provenance()
            .source
            .as_deref()
            .unwrap_or("")
            .contains("private-source"));
        // A forced refresh that gets a bad answer keeps the previous file.
        assert!(ProviderCatalog::refresh(&invalid, &url).is_err());
        assert_eq!(
            std::fs::read_to_string(&invalid).map_err(|e| e.to_string())?,
            "previous-invalid-data"
        );
        Ok(())
    });
    let (server, client) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, client)
    })
    .await?;
    server?;
    client??;
    Ok(())
}

#[test]
fn embedded_catalog_and_auth_registry_populate_existing_connect_flow(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::auth::{
        plugin::{AuthMethodSpec, AuthPluginRegistry},
        ProviderId,
    };
    let catalog = ProviderCatalog::from_embedded()?;
    let registry = AuthPluginRegistry::with_builtins();
    assert!(catalog.providers().len() > registry.providers().len());
    assert!(!catalog
        .provider("anthropic")
        .ok_or("missing Anthropic")?
        .models
        .is_empty());
    assert_eq!(
        catalog
            .sorted_by_priority()
            .first()
            .ok_or("empty catalog")?
            .id,
        "openai"
    );
    let openai = registry
        .get(&ProviderId::codex())
        .ok_or("missing OpenAI authentication")?;
    assert_eq!(openai.label(), "OpenAI");
    assert!(openai
        .auth_methods()
        .iter()
        .any(|method| matches!(method, AuthMethodSpec::OAuthAuto { .. })));
    assert!(openai
        .auth_methods()
        .iter()
        .any(|method| matches!(method, AuthMethodSpec::ApiKey { .. })));
    assert!(registry.get(&ProviderId::github_copilot()).is_some());
    Ok(())
}
