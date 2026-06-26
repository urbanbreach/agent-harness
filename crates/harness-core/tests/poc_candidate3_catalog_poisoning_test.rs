//! PoC for Candidate 3: Provider catalog fetch/cache poisoning or arbitrary write.
//!
//! Verifies that:
//! 1. auth_methods are computed from provider id, not from fetched catalog data
//! 2. A malicious catalog entry can't inject OAuth methods
//! 3. Cache write stays at the specified path with 0600 permissions
//! 4. Fetch falls back to embedded on failure

use harness_core::provider_catalog::{CatalogAuthMethod, OAuthFlow, ProviderCatalog};

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
                "models": {}
            }
        }
    }"#;

    let catalog = ProviderCatalog::fetch_from_url("data:text/plain;base64,")
        .or_else(|_| {
            // fetch_from_url will fail with a data: URL, so parse directly
            // by using from_path on a temp file
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("evil-catalog.json");
            std::fs::write(&path, malicious_catalog).expect("write");
            ProviderCatalog::from_path(&path)
        })
        .expect("parse malicious catalog");

    let provider = catalog.provider("302ai").expect("302ai should exist");

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
                "models": {}
            }
        }
    }"#;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("evil-catalog.json");
    std::fs::write(&path, malicious_catalog).expect("write");

    let catalog = ProviderCatalog::from_path(&path).expect("parse");
    let provider = catalog
        .provider("evil-provider")
        .expect("evil-provider exists");

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
                "models": {}
            }
        }
    }"#;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("evil-codex.json");
    std::fs::write(&path, malicious_catalog).expect("write");

    let catalog = ProviderCatalog::from_path(&path).expect("parse");
    let provider = catalog.provider("codex").expect("codex exists");

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
    let dir = tempfile::tempdir().expect("tempdir");
    let cache_path = dir.path().join("cache.json");

    // Use a local mock server to test cache write behavior.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
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

    let _catalog = ProviderCatalog::cached(&cache_path, Some(&url)).expect("should load");

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
            .expect("metadata")
            .permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "cache file should have 0600 permissions"
        );
    }

    // Verify no unexpected files were written in the cache directory
    let parent = cache_path.parent().expect("parent");
    let entries = std::fs::read_dir(parent).expect("read dir");
    for entry in entries {
        let entry = entry.expect("entry");
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
    let dir = tempfile::tempdir().expect("tempdir");
    let cache_path = dir.path().join("cache.json");
    let invalid_url = "http://127.0.0.1:1/nonexistent";

    let catalog =
        ProviderCatalog::cached(&cache_path, Some(invalid_url)).expect("should fall back");

    // Should fall back to embedded catalog with 116 providers
    assert_eq!(
        catalog.providers().len(),
        116,
        "should fall back to embedded catalog on fetch failure"
    );
}
