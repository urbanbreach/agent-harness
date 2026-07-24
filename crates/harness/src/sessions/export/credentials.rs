use std::collections::BTreeSet;

use harness_core::auth::CredentialStore;
use harness_core::config::{
    HarnessConfig, McpServerConfig, OpenAiCompatibleProviderConfig, ProviderConfig,
};
use serde_json::{json, Value};

use crate::CliDeps;

pub(super) fn session_export_config_credential_values(
    config: &HarnessConfig,
    deps: &CliDeps,
) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(token) = config.integrations.remote_search.auth_token.as_deref() {
        push_credential_value(&mut values, token);
    }
    for instruction in &config.instruction_files {
        push_credential_value(&mut values, &instruction.content);
    }
    for agent in config.agents.values() {
        if let Some(prompt) = agent.system_prompt.as_deref() {
            push_credential_value(&mut values, prompt);
        }
    }
    for provider in config.providers.values() {
        let ProviderConfig::OpenAiCompatible(provider) = provider else {
            continue;
        };
        push_credential_value(&mut values, &provider.api_key);
        if let (Some(auth_provider), Some(store)) = (
            provider.auth_provider.clone(),
            CredentialStore::from_lookup(&|name| deps.env_var_value(name)),
        ) {
            if let Ok(Some(stored)) = store.load(&auth_provider) {
                for value in stored.secret_values() {
                    push_credential_value(&mut values, &value);
                }
            }
        }
        for env_name in &provider.api_key_env {
            if let Some(value) = deps.env_var_value(env_name) {
                push_credential_value(&mut values, &value);
            }
        }
        for (name, value) in &provider.headers {
            if is_credential_name(name) {
                push_credential_value(&mut values, value);
            }
        }
    }
    for server in config.integrations.mcp.servers.values() {
        match server {
            McpServerConfig::Stdio { env, .. } => {
                for (name, value) in env {
                    if is_credential_name(name) {
                        push_credential_value(&mut values, value);
                    }
                }
            }
            McpServerConfig::Http { headers, .. } => {
                for (name, value) in headers {
                    if is_credential_name(name) {
                        push_credential_value(&mut values, value);
                    }
                }
            }
        }
    }
    dedupe_credential_values(values)
}

pub(super) fn session_export_credential_store_manifest(
    config: &HarnessConfig,
    credential_store: Option<&CredentialStore>,
) -> Value {
    let providers = config.providers.values().filter_map(|provider| {
        let ProviderConfig::OpenAiCompatible(provider) = provider else {
            return None;
        };
        provider.auth_provider.clone()
    });
    let Some(store) = credential_store else {
        return json!({
            "available": false,
            "redacted_by_default": true,
            "excluded_from_bundle": true,
            "providers": [],
        });
    };
    let entries = store.manifest_entries(providers);
    json!({
        "available": true,
        "redacted_by_default": true,
        "excluded_from_bundle": true,
        "data_dir": store.data_dir().display().to_string(),
        "providers": entries,
    })
}

fn push_credential_value(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.len() >= 8 && !value.contains("[REDACTED") {
        values.push(value.to_string());
    }
}

pub(super) fn dedupe_credential_values(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_credential_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "CREDENTIAL",
        "AUTHORIZATION",
        "COOKIE",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
}
