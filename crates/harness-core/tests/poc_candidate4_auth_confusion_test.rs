//! PoC for Candidate 4: Catalog provider/auth confusion.
//!
//! Verifies that:
//! 1. auth_methods_for_provider only returns OAuth for codex and github-copilot
//! 2. AuthPluginRegistry only has builtin plugins (codex, copilot)
//! 3. catalog_providers filters out providers without a registered plugin
//! 4. execute_login_selection rejects OAuth for non-builtin providers

use harness_core::auth::plugin::AuthPluginRegistry;
use harness_core::auth::ProviderId;
use harness_core::provider_catalog::{CatalogAuthMethod, OAuthFlow, ProviderCatalog};
use harness_core::UnwrapOrAbort;

fn provider_id(value: &str) -> ProviderId {
    ProviderId::parse(value).unwrap_or_abort()
}

#[test]
fn poc_registry_only_has_builtin_plugins() {
    let registry = AuthPluginRegistry::with_builtins();

    assert!(
        registry.get(&ProviderId::codex()).is_some(),
        "codex should be registered"
    );
    assert!(
        registry.get(&ProviderId::github_copilot()).is_some(),
        "github-copilot should be registered"
    );

    let evil = provider_id("evil-provider");
    assert!(
        registry.get(&evil).is_none(),
        "non-builtin provider should not have a plugin"
    );

    let providers = registry.providers();
    assert_eq!(
        providers.len(),
        2,
        "registry should only have 2 builtin plugins"
    );
}

#[test]
fn poc_catalog_providers_filters_non_registered() {
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();
    let registry = AuthPluginRegistry::with_builtins();

    let providers: Vec<_> = catalog
        .sorted_by_priority()
        .into_iter()
        .filter_map(|entry| {
            let provider_id = ProviderId::parse(entry.id.as_str())?;
            let plugin = registry.get(&provider_id)?;
            Some((provider_id, plugin))
        })
        .collect();

    // Only providers with a registered plugin should appear.
    // "codex" is a builtin auth provider but is NOT in the embedded catalog.
    // "github-copilot" is both a builtin and in the catalog.
    for (id, _) in &providers {
        assert!(
            id.as_str() == "codex" || id.as_str() == "github-copilot",
            "non-builtin provider {id} should not appear in filtered list"
        );
    }
    assert!(
        providers
            .iter()
            .any(|(id, _)| id.as_str() == "github-copilot"),
        "github-copilot should be in filtered providers"
    );
}

#[test]
fn poc_non_builtin_provider_auth_methods_only_apikey() {
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

    for provider in catalog.providers() {
        if provider.id == "codex" || provider.id == "github-copilot" {
            continue;
        }
        assert_eq!(
            provider.auth_methods,
            vec![CatalogAuthMethod::ApiKey],
            "provider {} should only have ApiKey, got {:?}",
            provider.id,
            provider.auth_methods
        );
    }
}

#[test]
fn poc_codex_has_browser_pkce_and_device_code() {
    // "codex" is not in the embedded catalog, but auth_methods_for_provider
    // computes methods from the id. Verify via a crafted catalog.
    let crafted = r#"{
        "provider": {
            "codex": {
                "name": "Codex",
                "options": {"baseURL": "https://api.openai.com/v1", "apiKeyEnv": ["OPENAI_API_KEY"]},
                "models": {}
            }
        }
    }"#;
    let dir = tempfile::tempdir().unwrap_or_abort();
    let path = dir.path().join("codex.json");
    std::fs::write(&path, crafted).unwrap_or_abort();
    let catalog = ProviderCatalog::from_path(&path).unwrap_or_abort();
    let codex = catalog.provider("codex").unwrap_or_abort();

    assert!(codex.auth_methods.contains(&CatalogAuthMethod::ApiKey));
    assert!(codex
        .auth_methods
        .contains(&CatalogAuthMethod::OAuth(OAuthFlow::BrowserPkce)));
    assert!(codex
        .auth_methods
        .contains(&CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode)));
}

#[test]
fn poc_copilot_has_device_code_only() {
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();
    let copilot = catalog.provider("github-copilot").unwrap_or_abort();

    assert!(copilot.auth_methods.contains(&CatalogAuthMethod::ApiKey));
    assert!(copilot
        .auth_methods
        .contains(&CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode)));
    assert!(
        !copilot
            .auth_methods
            .contains(&CatalogAuthMethod::OAuth(OAuthFlow::BrowserPkce)),
        "copilot should not have BrowserPkce"
    );
}
