use std::collections::BTreeMap;

use serde::Serialize;

use super::types::{OpenAiHttpCassette, ProviderCassette};
use super::CassetteError;

pub fn assert_cassette_is_safe(cassette: &ProviderCassette) -> Result<(), CassetteError> {
    let body = serde_json::to_string(cassette).map_err(CassetteError::Serialize)?;
    assert_serialized_cassette_is_safe(&body)
}

pub fn assert_http_cassette_is_safe(cassette: &OpenAiHttpCassette) -> Result<(), CassetteError> {
    let body = serde_json::to_string(cassette).map_err(CassetteError::Serialize)?;
    assert_serialized_cassette_is_safe(&body)
}

fn assert_serialized_cassette_is_safe(body: &str) -> Result<(), CassetteError> {
    if let Some(kind) = detect_secret(body) {
        return Err(CassetteError::UnsafeSecret { kind });
    }
    for (name, value) in std::env::vars() {
        if !is_credential_env_name(&name) || value.len() < 8 {
            continue;
        }
        if body.contains(&value) {
            return Err(CassetteError::UnsafeSecret {
                kind: format!("env:{name}"),
            });
        }
    }
    Ok(())
}

fn detect_secret(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    if lower.contains("bearer ") {
        return Some("authorization_bearer".to_string());
    }
    for (needle, kind) in [
        ("sk-ant-", "anthropic_api_key"),
        ("sk-", "openai_api_key"),
        ("AIza", "google_api_key"),
        ("AKIA", "aws_access_key_id"),
        ("github_pat_", "github_pat"),
        ("ghp_", "github_token"),
        ("-----BEGIN ", "pem_private_key"),
    ] {
        if body.contains(needle) {
            return Some(kind.to_string());
        }
    }
    None
}

fn is_credential_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "CREDENTIAL", "PASSWORD"]
        .iter()
        .any(|part| upper.contains(part))
}

pub fn compact_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string())
}
