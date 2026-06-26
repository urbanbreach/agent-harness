use harness_core::auth::{CredentialStore, ProviderId, StoredCredential};

fn provider_id(value: &str) -> ProviderId {
    ProviderId::parse(value).expect("valid provider id")
}

#[test]
fn poc_parse_rejects_all_traversal_payloads() {
    let payloads = [
        "../etc/passwd",
        "..\\..\\windows",
        "foo/bar",
        "foo\\bar",
        "codex\x00evil",
        "codex\nevil",
        "codex\revil",
        "codex\u{1b}]52;c;SGFja2Vk\u{7}",
        "..",
        "foo..bar",
        "",
        "   ",
        "/etc/passwd",
        "a/../../../b",
    ];

    for payload in &payloads {
        assert!(
            ProviderId::parse(payload).is_none(),
            "parse() should reject {:?}",
            payload
        );
    }
}

#[test]
fn poc_credential_path_for_valid_provider_stays_in_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let credentials_dir = temp.path().join("credentials");
    let provider = provider_id("safe-provider");

    let path = store.credential_path(&provider);

    assert!(
        path.starts_with(&credentials_dir),
        "credential path {path:?} must stay inside {credentials_dir:?}"
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("safe-provider.json")
    );
}

#[test]
fn poc_save_and_load_valid_custom_provider_stays_in_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let credentials_dir = temp.path().join("credentials");
    let provider = provider_id("custom-provider");
    let credential =
        StoredCredential::api_key(provider.clone(), "sk-test-secret", "2026-06-26T00:00:00Z");

    store
        .save(&credential)
        .expect("save valid provider credential");
    let loaded = store
        .load(&provider)
        .expect("load valid provider credential")
        .expect("credential exists");
    let path = store.credential_path(&provider);

    assert!(
        path.starts_with(&credentials_dir),
        "credential path {path:?} must stay inside {credentials_dir:?}"
    );
    assert_eq!(loaded.provider, provider);
    assert_eq!(loaded.api_key.as_deref(), Some("sk-test-secret"));
}
