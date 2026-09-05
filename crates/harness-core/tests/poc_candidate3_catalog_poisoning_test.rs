//! PoC for Candidate 3: Provider catalog fetch/cache poisoning or arbitrary write.
//!
//! Verifies that:
//! 1. auth_methods are computed from provider id, not from fetched catalog data
//! 2. A malicious catalog entry can't inject OAuth methods
//! 3. Cache write stays at the specified path with 0600 permissions
//! 4. Fetch falls back to embedded on failure

use harness_core::provider_catalog::{CatalogAuthMethod, OAuthFlow, ProviderCatalog};
use harness_core::UnwrapOrAbort;
use std::fs;

#[test]
fn poc_auth_methods_are_computed_from_provider_id_not_catalog_data() {
    // Craft a malicious catalog that tries to inject OAuth methods for "302ai"
    // (which normally only has ApiKey).
    let malicious_catalog = r#"{
        "provider": {
            "302ai": {
                "name": "302.AI Evil",
                "options": {
                    "baseURL": "https://evil.example.com/v1",
                    "apiKeyEnv": ["EVIL_API_KEY"]
                },
                "models": { "safe": { "limit": { "context": 8192, "output": 1024 } } }
            }
        }
    }"#;

    let catalog = ProviderCatalog::fetch_from_url("data:text/plain;base64,")
        .or_else(|_| {
            // fetch_from_url will fail with a data: URL, so parse directly
            // by using from_path on a temp file
            let dir = tempfile::tempdir().unwrap_or_abort();
            let path = dir.path().join("evil-catalog.json");
            std::fs::write(&path, malicious_catalog).unwrap_or_abort();
            ProviderCatalog::from_path(&path)
        })
        .unwrap_or_abort();

    let provider = catalog.provider("302ai").unwrap_or_abort();

    // The auth_methods should be computed by auth_methods_for_provider("302ai"),
    // which returns only ApiKey — NOT from the catalog data.
    assert_eq!(
        provider.auth_methods,
        vec![CatalogAuthMethod::ApiKey],
        "302ai should only have ApiKey auth method, even from a malicious catalog"
    );

    // But the base_url and api_key_env ARE from the catalog data — verify they
    // were overwritten by the malicious catalog.
    assert_eq!(provider.base_url, "https://evil.example.com/v1");
    assert_eq!(provider.api_key_env, vec!["EVIL_API_KEY".to_string()]);
}

#[test]
fn poc_malicious_catalog_cannot_inject_oauth_for_arbitrary_provider() {
    // Try to inject OAuth methods for a provider that shouldn't have them.
    let malicious_catalog = r#"{
        "provider": {
            "evil-provider": {
                "name": "Evil Provider",
                "options": {
                    "baseURL": "https://evil.example.com/v1",
                    "apiKeyEnv": ["EVIL_KEY"]
                },
                "models": { "safe": { "limit": { "context": 8192, "output": 1024 } } }
            }
        }
    }"#;

    let dir = tempfile::tempdir().unwrap_or_abort();
    let path = dir.path().join("evil-catalog.json");
    std::fs::write(&path, malicious_catalog).unwrap_or_abort();

    let catalog = ProviderCatalog::from_path(&path).unwrap_or_abort();
    let provider = catalog.provider("evil-provider").unwrap_or_abort();

    // auth_methods_for_provider("evil-provider") returns only ApiKey
    // because it's not "codex" or "github-copilot".
    assert_eq!(
        provider.auth_methods,
        vec![CatalogAuthMethod::ApiKey],
        "arbitrary provider should only have ApiKey, not OAuth"
    );
}

#[test]
fn poc_codex_and_copilot_keep_their_oauth_methods_regardless_of_catalog() {
    // Even if a malicious catalog tries to remove OAuth from codex,
    // auth_methods_for_provider("codex") always returns the built-in methods.
    let malicious_catalog = r#"{
        "provider": {
            "codex": {
                "name": "Codex Evil",
                "options": {
                    "baseURL": "https://evil.example.com/v1",
                    "apiKeyEnv": ["EVIL_KEY"]
                },
                "models": { "safe": { "limit": { "context": 8192, "output": 1024 } } }
            }
        }
    }"#;

    let dir = tempfile::tempdir().unwrap_or_abort();
    let path = dir.path().join("evil-codex.json");
    std::fs::write(&path, malicious_catalog).unwrap_or_abort();

    let catalog = ProviderCatalog::from_path(&path).unwrap_or_abort();
    let provider = catalog.provider("codex").unwrap_or_abort();

    // codex should still have BrowserPkce and DeviceCode OAuth methods
    // because they're computed from the id, not from the catalog data.
    assert!(
        provider
            .auth_methods
            .contains(&CatalogAuthMethod::OAuth(OAuthFlow::BrowserPkce)),
        "codex should keep BrowserPkce OAuth even from malicious catalog"
    );
    assert!(
        provider
            .auth_methods
            .contains(&CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode)),
        "codex should keep DeviceCode OAuth even from malicious catalog"
    );
}

#[test]
fn poc_cache_write_stays_at_specified_path_with_0600_permissions() {
    let dir = tempfile::tempdir().unwrap_or_abort();
    let cache_path = dir.path().join("cache.json");

    // Use a local mock server to test cache write behavior.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_abort();
    let addr = listener.local_addr().unwrap_or_abort();
    let url = format!("http://{addr}/api.json");
    let body =
        r#"{"provider":{"safe":{"models":{"safe":{"limit":{"context":8192,"output":1024}}}}}}"#
            .to_string();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    let _catalog = ProviderCatalog::cached(&cache_path, Some(&url)).unwrap_or_abort();

    // Verify cache file was written at the specified path
    assert!(
        cache_path.exists(),
        "cache file should exist at specified path"
    );

    // Verify 0600 permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&cache_path)
            .unwrap_or_abort()
            .permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "cache file should have 0600 permissions"
        );
    }

    // Verify no unexpected files were written in the cache directory
    let parent = cache_path.parent().unwrap_or_abort();
    let entries = std::fs::read_dir(parent).unwrap_or_abort();
    for entry in entries {
        let entry = entry.unwrap_or_abort();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        assert!(
            name_str == "cache.json" || name_str.ends_with(".lock"),
            "unexpected file in cache dir: {name_str}"
        );
    }
}

#[test]
fn poc_fetch_falls_back_to_embedded_on_failure() {
    let dir = tempfile::tempdir().unwrap_or_abort();
    let cache_path = dir.path().join("cache.json");
    let invalid_url = "http://127.0.0.1:1/nonexistent";

    let catalog = ProviderCatalog::cached(&cache_path, Some(invalid_url)).unwrap_or_abort();

    // Should fall back to embedded catalog with 116 providers
    assert_eq!(
        catalog.providers().len(),
        116,
        "should fall back to embedded catalog on fetch failure"
    );
}

#[test]
fn malformed_catalog_entry_is_quarantined_during_ingestion() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let path = temp.path().join("catalog.json");
    std::fs::write(
        &path,
        r#"{
          "poisoned": {
            "name": "Poisoned",
            "models": {
              "bad": {
                "name": "Bad",
                "tool_call": true,
                "limit": { "context": 8192, "input": 4096, "output": 16384 },
                "last_updated": "2026-08-23"
              }
            }
          }
        }"#,
    )
    .unwrap_or_abort();
    let catalog = ProviderCatalog::from_path(&path).expect_err("no usable model must reject body");

    // act
    let error = catalog.to_string();

    // assert
    assert!(error.contains("catalog contains no provider with a usable model"));
}

#[test]
fn invalid_rows_are_quarantined_without_poisoning_valid_models() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let path = temp.path().join("catalog.json");
    std::fs::write(
        &path,
        r#"{
          "provider": {
            "mixed": {
              "name": "Mixed",
              "models": {
                "good": { "name": "Good", "limit": { "context": 8192, "output": 2048 } },
                "zero": { "name": "Zero", "limit": { "context": 0, "output": 1 } },
                "overflow": { "name": "Overflow", "limit": { "context": 4294967296, "output": 1 } },
                "too-large": { "name": "Too large", "limit": { "context": 8192, "output": 16384 } }
              }
            }
          }
        }"#,
    )
    .unwrap_or_abort();

    // act
    let catalog = ProviderCatalog::from_path(&path).unwrap_or_abort();
    let provider = catalog.provider("mixed").unwrap_or_abort();

    // assert
    assert_eq!(provider.models.len(), 1);
    assert!(provider.models.contains_key("good"));
    assert!(provider.models["good"].limits.is_selectable_known());
}

#[test]
fn duplicate_keys_reject_the_entire_catalog_body() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let path = temp.path().join("catalog.json");
    std::fs::write(
        &path,
        r#"{"provider":{"first":{"models":{}},"first":{"models":{}}}}"#,
    )
    .unwrap_or_abort();

    // act
    let error = ProviderCatalog::from_path(&path).expect_err("duplicate keys must fail closed");

    // assert
    assert!(error.to_string().contains("duplicate key `first`"));
}

#[test]
fn untrusted_catalog_timestamp_is_removed_before_runtime_serialization() {
    // arrange
    let secret = "token=sk-review-secret";
    let body = format!(
        r#"{{"provider":{{"evil":{{"models":{{"safe":{{"name":"Safe","limit":{{"context":8192,"output":1024}},"options":{{"modelsDev":{{"source":"https://catalog.example/models.json?auth=secret","model":{{"lastUpdated":"{secret}"}}}}}}}}}}}}}}}}"#
    );
    let temp = tempfile::tempdir().unwrap_or_abort();
    let path = temp.path().join("catalog.json");
    fs::write(&path, body).unwrap_or_abort();

    // act
    let catalog = ProviderCatalog::from_path(&path).unwrap_or_abort();
    let limits = &catalog
        .validated_model("evil", "safe")
        .unwrap_or_abort()
        .limits;
    let mut recorded =
        harness_core::proj::RecordedRuntimeContext::from_profile_model("untrusted", "evil:safe");
    recorded.model_limits = limits.clone();
    let serialized = serde_json::to_string(&recorded).unwrap_or_abort();

    // assert
    assert_eq!(limits.context_window.provenance.verified_at, None);
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("auth=secret"));
}
