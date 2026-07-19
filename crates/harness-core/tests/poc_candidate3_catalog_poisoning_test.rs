//! PoC for Candidate 3: Provider catalog fetch/cache poisoning or arbitrary write.
//!
//! Verifies that:
//! 1. auth_methods are computed from provider id, not from fetched catalog data
//! 2. A malicious catalog entry can't inject OAuth methods
//! 3. Cache write stays at the specified path with 0600 permissions
//! 4. Fetch falls back to embedded on failure

use harness_core::provider_catalog::{CatalogAuthMethod, OAuthFlow, ProviderCatalog};
use harness_core::UnwrapOrAbort;

#[test]
fn poc_auth_methods_are_computed_from_provider_id_not_catalog_data() {
    // arrange
    // act
    // assert
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
                "models": {}
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
    // arrange
    // act
    // assert
    // Try to inject OAuth methods for a provider that shouldn't have them.
    let malicious_catalog = r#"{
        "provider": {
            "evil-provider": {
                "name": "Evil Provider",
                "options": {
                    "baseURL": "https://evil.example.com/v1",
                    "apiKeyEnv": ["EVIL_KEY"]
                },
                "models": {}
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
    // arrange
    // act
    // assert
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
                "models": {}
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
    // arrange
    // act
    // assert
    let dir = tempfile::tempdir().unwrap_or_abort();
    let cache_path = dir.path().join("cache.json");

    // Use a local mock server to test cache write behavior.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_abort();
    let addr = listener.local_addr().unwrap_or_abort();
    let url = format!("http://{addr}/api.json");
    let body = r#"{"provider":{}}"#.to_string();
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
    std::thread::sleep(std::time::Duration::from_millis(100));

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
    // arrange
    // act
    // assert
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
