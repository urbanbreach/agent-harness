use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use harness_core::auth::{
    AuthProviderId, CredentialStore, CredentialStoreError, StoredCredentialKind,
};
use harness_core::config::{load_resolved_config_with_context, HarnessConfig, ProviderConfig};
use serde::Serialize;

use crate::CliDeps;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProviderAuthStatus {
    pub auth_provider: String,
    pub provider_ids: Vec<String>,
    pub source: String,
    pub presence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub env_fallback_configured: bool,
    pub inline_fallback_configured: bool,
    pub usable_without_network_probe: bool,
}

#[derive(Debug, Clone, Default)]
struct AuthProviderFallbacks {
    provider_ids: BTreeSet<String>,
    api_key_env: BTreeSet<String>,
    inline_configured: bool,
}

pub(crate) fn auth_statuses(
    config: Option<&HarnessConfig>,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<&CredentialStore>,
) -> Vec<ProviderAuthStatus> {
    let fallback_map = configured_auth_provider_fallbacks(config);
    AuthProviderId::ALL
        .into_iter()
        .map(|auth_provider| {
            auth_status(
                auth_provider,
                fallback_map.get(&auth_provider),
                env_var_is_set,
                credential_store,
            )
        })
        .collect()
}

pub(crate) fn onboarding_required_for_config(
    config: Option<&HarnessConfig>,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<&CredentialStore>,
) -> bool {
    let Some(config) = config else {
        return false;
    };
    let fallback_map = configured_auth_provider_fallbacks(Some(config));
    fallback_map.iter().any(|(provider, fallbacks)| {
        let status = auth_status(*provider, Some(fallbacks), env_var_is_set, credential_store);
        !status.usable_without_network_probe
    })
}

fn auth_status(
    auth_provider: AuthProviderId,
    fallbacks: Option<&AuthProviderFallbacks>,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<&CredentialStore>,
) -> ProviderAuthStatus {
    let provider_ids = fallbacks
        .map(|fallbacks| fallbacks.provider_ids.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let env_fallback_configured = fallbacks
        .map(|fallbacks| {
            fallbacks
                .api_key_env
                .iter()
                .any(|name| env_var_is_set(name))
        })
        .unwrap_or(false);
    let inline_fallback_configured = fallbacks
        .map(|fallbacks| fallbacks.inline_configured)
        .unwrap_or(false);

    let stored = match credential_store.map(|store| store.load(auth_provider)) {
        Some(Ok(stored)) => stored,
        Some(Err(err)) => {
            return ProviderAuthStatus {
                auth_provider: auth_provider.to_string(),
                provider_ids,
                source: "credential_store_error".to_string(),
                presence: "error".to_string(),
                kind: None,
                expires_at: None,
                account_id: None,
                enterprise_url: None,
                error: Some(err.to_string()),
                env_fallback_configured,
                inline_fallback_configured,
                usable_without_network_probe: false,
            };
        }
        None => None,
    };

    if let Some(stored) = stored {
        let kind = stored_credential_kind_label(stored.kind).to_string();
        return ProviderAuthStatus {
            auth_provider: auth_provider.to_string(),
            provider_ids,
            source: format!("stored_{kind}"),
            presence: "stored".to_string(),
            kind: Some(kind),
            expires_at: stored.expires_at.clone(),
            account_id: stored
                .account_id
                .as_ref()
                .and_then(|value| redact_present(value)),
            enterprise_url: stored
                .enterprise_url
                .as_ref()
                .and_then(|value| redact_present(value)),
            error: None,
            env_fallback_configured,
            inline_fallback_configured,
            usable_without_network_probe: true,
        };
    }

    let (presence, source, usable) = if env_fallback_configured {
        ("env", "apiKeyEnv", true)
    } else if inline_fallback_configured {
        ("inline", "inline_apiKey", true)
    } else {
        ("missing", "none", false)
    };

    ProviderAuthStatus {
        auth_provider: auth_provider.to_string(),
        provider_ids,
        source: source.to_string(),
        presence: presence.to_string(),
        kind: None,
        expires_at: None,
        account_id: None,
        enterprise_url: None,
        error: None,
        env_fallback_configured,
        inline_fallback_configured,
        usable_without_network_probe: usable,
    }
}

fn configured_auth_provider_fallbacks(
    config: Option<&HarnessConfig>,
) -> BTreeMap<AuthProviderId, AuthProviderFallbacks> {
    let mut map = BTreeMap::<AuthProviderId, AuthProviderFallbacks>::new();
    let Some(config) = config else {
        return map;
    };

    for (provider_id, provider) in &config.providers {
        let ProviderConfig::OpenAiCompatible(provider) = provider;
        let Some(auth_provider) = provider.auth_provider else {
            continue;
        };
        let entry = map.entry(auth_provider).or_default();
        entry.provider_ids.insert(provider_id.clone());
        entry
            .api_key_env
            .extend(provider.api_key_env.iter().cloned());
        entry.inline_configured |= non_empty(&provider.api_key).is_some();
    }
    map
}

pub(super) fn resolve_provider_arg(
    provider: Option<&str>,
    config: Option<&HarnessConfig>,
    stderr: &mut dyn Write,
) -> Option<AuthProviderId> {
    if let Some(provider) = provider {
        if let Some(auth_provider) = AuthProviderId::parse(provider) {
            return Some(auth_provider);
        }
        let _ = writeln!(
            stderr,
            "unknown auth provider `{provider}`; expected codex or github-copilot"
        );
        return None;
    }

    let configured = configured_auth_provider_fallbacks(config)
        .into_keys()
        .collect::<Vec<_>>();
    match configured.as_slice() {
        [only] => Some(*only),
        [] => {
            let _ = writeln!(
                stderr,
                "auth provider is required when config has no provider authProvider; expected codex or github-copilot"
            );
            None
        }
        _ => {
            let _ = writeln!(
                stderr,
                "auth provider is required when multiple auth providers are configured; expected codex or github-copilot"
            );
            None
        }
    }
}

pub(super) fn resolve_login_provider_arg(
    provider: &str,
    stderr: &mut dyn Write,
) -> Option<AuthProviderId> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "codex" | "openai" => Some(AuthProviderId::Codex),
        "github-copilot" | "github copilot" => Some(AuthProviderId::GithubCopilot),
        _ => {
            let _ = writeln!(
                stderr,
                "unknown auth provider `{provider}`; expected codex, openai, or github-copilot"
            );
            None
        }
    }
}

pub(super) fn load_optional_config(
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stderr: &mut dyn Write,
    deps: &CliDeps,
) -> Option<HarnessConfig> {
    let context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "auth warning: failed to resolve config context: {err}"
            );
            return None;
        }
    };
    let mut config = match load_resolved_config_with_context(config_path.as_deref(), &context) {
        Ok(Some(loaded)) => loaded.config,
        Ok(None) => return None,
        Err(err) => {
            let _ = writeln!(stderr, "auth warning: failed to load config: {err}");
            return None;
        }
    };
    config.apply_session_dir_override(session_dir);
    Some(config)
}

pub(super) fn credential_store_from_deps(deps: &CliDeps) -> Option<CredentialStore> {
    CredentialStore::from_lookup(&|name| deps.env_var_value(name))
}

pub(super) fn credential_store_or_error(
    stderr: &mut dyn Write,
    deps: &CliDeps,
) -> Option<CredentialStore> {
    let store = credential_store_from_deps(deps);
    if store.is_none() {
        let _ = writeln!(
            stderr,
            "auth failed: could not resolve a Harness data directory; set HARNESS_DATA_HOME, XDG_DATA_HOME, or HOME"
        );
    }
    store
}

fn stored_credential_kind_label(kind: StoredCredentialKind) -> &'static str {
    match kind {
        StoredCredentialKind::Oauth => "oauth",
        StoredCredentialKind::ApiKey => "api_key",
    }
}

fn redact_present(value: &str) -> Option<String> {
    non_empty(value).map(|_| "<redacted>".to_string())
}

pub(super) fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(super) fn credential_store_error(
    prefix: &str,
    err: CredentialStoreError,
    stderr: &mut dyn Write,
) -> i32 {
    let _ = writeln!(stderr, "{prefix}: {err}");
    1
}

pub(super) fn auth_oauth_error<E: std::fmt::Display>(
    prefix: &str,
    err: E,
    stderr: &mut dyn Write,
) -> i32 {
    let _ = writeln!(stderr, "{prefix}: {err}");
    1
}
