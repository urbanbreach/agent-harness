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
    let invalid = temp.path().join("invalid.json");
    std::fs::write(&invalid, "previous-invalid-data")?;
    let valid = r#"{"example":{"models":{"small":{"limit":{"context":1000,"output":200}}}}}"#;
    let server = async {
        for body in [valid, "invalid-response"] {
            let (mut socket, _) = listener.accept().await?;
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await?;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await?;
        }
        Ok::<_, std::io::Error>(())
    };
    let client = tokio::task::spawn_blocking(move || -> Result<(), String> {
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
        assert!(ProviderCatalog::cached(&cache, Some(&url))
            .map_err(|e| e.to_string())?
            .provider("example")
            .is_some());
        assert!(ProviderCatalog::cached(&invalid, Some(&url))
            .map_err(|e| e.to_string())?
            .provider("anthropic")
            .is_some());
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
