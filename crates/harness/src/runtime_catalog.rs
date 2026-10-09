use harness_core::{
    auth::{CredentialStore, ProviderId},
    config::*,
    provider_catalog::{ProviderCatalog, ProviderCatalogEntry},
};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) const BUILTIN_CODEX_PROVIDER_ID: &str = "openai-codex";
pub(crate) const BUILTIN_COPILOT_PROVIDER_ID: &str = "github-copilot";
pub(crate) const BUILTIN_ANTHROPIC_SUBSCRIPTION_PROVIDER_ID: &str = "anthropic-subscription";
pub(crate) struct RuntimeCatalogResolution {
    pub(crate) config: HarnessConfig,
    pub(crate) config_digest: String,
    pub(crate) connected_provider_ids: Vec<String>,
    pub(crate) no_provider_connected: bool,
    pub(crate) curated: bool,
}
pub(crate) fn resolve_runtime_catalog(
    base: Option<HarnessConfig>,
    digest: Option<String>,
    directory: Option<PathBuf>,
    context: &ConfigLoadContext,
    store: Option<&CredentialStore>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<RuntimeCatalogResolution, String> {
    let mut config = match base {
        Some(config) => config,
        None => {
            load_config_or_defaults_with_lookup(None, context, lookup)
                .map_err(|e| e.to_string())?
                .config
        }
    };
    let curated = !config.providers.is_empty();
    let mut connected = add_connected_providers(&mut config, curated, store, lookup)?;
    merge_cached_models(&mut config, store, lookup);
    for (name, provider) in &mut config.providers {
        if provider.credential_provider(name) == Some(ProviderId::codex())
            && let ProviderConfig::OpenAiCompatible(provider) = provider
        {
            provider
                .models
                .retain(|model, _| harness_providers::codex_model_allowed(model));
        }
    }
    for (name, provider) in &config.providers {
        if connected.contains(name) {
            continue;
        }
        let (env, key) = match provider {
            ProviderConfig::OpenAiCompatible(p) => (&p.api_key_env, &p.api_key),
            ProviderConfig::Anthropic(p) => (&p.api_key_env, &p.api_key),
            ProviderConfig::AnthropicSubscription(p) => {
                if p.enabled != Some(false)
                    && (p.enabled == Some(true) || subscription_available(store, lookup)?)
                {
                    connected.push(name.clone());
                }
                continue;
            }
        };
        let id = provider.credential_provider(name);
        let subscription = id
            .as_ref()
            .is_some_and(|id| *id == ProviderId::codex() || *id == ProviderId::github_copilot());
        let has_credential = match &id {
            Some(id) => stored(store, id)?,
            None => false,
        };
        if has_credential
            || !key.trim().is_empty()
            || env.is_empty() && !subscription
            || env
                .iter()
                .any(|name| lookup(name).is_some_and(|value| !value.trim().is_empty()))
        {
            connected.push(name.clone());
        }
    }
    connected.retain(|id| {
        !config.disabled_providers.contains(id)
            && (config.enabled_providers.is_empty() || config.enabled_providers.contains(id))
    });
    let default_model = connected
        .first()
        .map(|id| {
            let provider = config
                .providers
                .get(id)
                .ok_or("selected provider missing")?;
            let model =
                default_model_for_provider(id, provider).ok_or("provider has no usable model")?;
            Ok::<_, String>((id.as_str(), model))
        })
        .transpose()?;
    for profile in config.agents.values_mut() {
        let selector = config
            .model_profiles
            .get(&profile.model_ref)
            .map_or(profile.model_ref.as_str(), |model| model.model.as_str());
        let unavailable_pin = !curated
            && selector
                .split_once(':')
                .or_else(|| selector.split_once('/'))
                .is_some_and(|(provider, _)| !connected.iter().any(|id| id == provider));
        if (!profile.model_ref_explicit && profile.model_ref == "mock:default") || unavailable_pin {
            profile.model_ref = match default_model {
                Some((id, model)) => format!("{id}:{model}"),
                None => "mock:default".into(),
            };
            if unavailable_pin {
                profile.model_ref_explicit = false;
                profile.variant = None;
            }
        }
    }
    config.apply_session_dir_override(directory);
    config.validate().map_err(|e| e.to_string())?;
    refresh_profile_model_metadata_registry(&config).map_err(|e| e.to_string())?;
    let config_digest = match digest {
        Some(digest) => digest,
        None => blake3::hash(&serde_json::to_vec(&config).map_err(|e| e.to_string())?)
            .to_hex()
            .to_string(),
    };
    Ok(RuntimeCatalogResolution {
        config,
        config_digest,
        no_provider_connected: connected.is_empty(),
        connected_provider_ids: connected,
        curated,
    })
}
pub(crate) fn default_model_for_provider<'a>(
    id: &str,
    provider: &'a ProviderConfig,
) -> Option<&'a str> {
    let preferred = if provider.credential_provider(id) == Some(ProviderId::codex()) {
        &["gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.5"][..]
    } else if provider.credential_provider(id) == Some(ProviderId::anthropic_subscription()) {
        &["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1"][..]
    } else if id == "google" {
        &[
            "gemini-3.1-pro-preview",
            "gemini-3-pro-preview",
            "gemini-2.5-pro",
        ][..]
    } else if id == "openrouter" {
        &[
            "anthropic/claude-sonnet-4.6",
            "openai/gpt-5.5",
            "anthropic/claude-sonnet-4.5",
        ][..]
    } else {
        &[
            "gpt-5.4-mini",
            "gpt-5.5",
            "claude-sonnet-4-6",
            "claude-sonnet-4-5",
        ][..]
    };
    let models = provider.models();
    preferred
        .iter()
        .find(|name| models.contains_key(**name))
        .copied()
        .or_else(|| {
            models
                .iter()
                .map(|(name, model)| {
                    let excluded = name.ends_with(":free")
                        || name
                            .split(['-', '/', '.', ':', '_'])
                            .any(|token| matches!(token, "nano" | "lite" | "mini" | "alpha"));
                    let missing_tool_support = model.metadata.supports_tool_calls != Some(true);
                    (name, model, missing_tool_support, excluded)
                })
                .min_by(
                    |(left_name, left, left_missing_tools, left_excluded),
                     (right_name, right, right_missing_tools, right_excluded)| {
                        left_missing_tools
                            .cmp(right_missing_tools)
                            .then_with(|| left_excluded.cmp(right_excluded))
                            .then_with(|| {
                                right.catalog_release_date.cmp(&left.catalog_release_date)
                            })
                            .then_with(|| left_name.cmp(right_name))
                    },
                )
                .map(|(name, _, _, _)| name.as_str())
        })
}

fn merge_cached_models(
    config: &mut HarnessConfig,
    store: Option<&CredentialStore>,
    lookup: &dyn Fn(&str) -> Option<String>,
) {
    if !config
        .providers
        .iter()
        .any(|(id, p)| p.credential_provider(id) == Some(ProviderId::codex()))
    {
        return;
    }
    if lookup("HARNESS_DISABLE_MODELS_FETCH")
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"))
    {
        return;
    }
    let path = lookup("HARNESS_MODELS_PATH")
        .map(PathBuf::from)
        .or_else(|| store.map(|s| s.data_dir().join("models-cache.json")));
    // Readiness and startup read the last catalog; explicit catalog operations refresh it.
    let Some(catalog) = path.and_then(|p| ProviderCatalog::from_path(&p).ok()) else {
        return;
    };
    let Some(source) = catalog.provider("openai") else {
        return;
    };
    for (id, provider) in &mut config.providers {
        if provider.credential_provider(id) != Some(ProviderId::codex()) {
            continue;
        }
        if let ProviderConfig::OpenAiCompatible(provider) = provider {
            for (id, model) in &source.models {
                if harness_providers::codex_model_allowed(id) {
                    provider
                        .models
                        .entry(id.clone())
                        .or_insert_with(|| model.definition.clone());
                }
            }
        }
    }
}
fn add_connected_providers(
    config: &mut HarnessConfig,
    curated: bool,
    store: Option<&CredentialStore>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<String>, String> {
    let mut connected = Vec::new();
    let codex = stored(store, &ProviderId::codex())?;
    let copilot = stored(store, &ProviderId::github_copilot())?;
    let subscription = subscription_available(store, lookup)?;
    if subscription
        && !config
            .providers
            .values()
            .any(|p| matches!(p, ProviderConfig::AnthropicSubscription(_)))
    {
        config.providers.insert(
            BUILTIN_ANTHROPIC_SUBSCRIPTION_PROVIDER_ID.into(),
            ProviderConfig::AnthropicSubscription(AnthropicSubscriptionProviderConfig {
                name: Some("Anthropic Subscription".into()),
                models: anthropic_subscription_models(),
                ..Default::default()
            }),
        );
        connected.push(BUILTIN_ANTHROPIC_SUBSCRIPTION_PROVIDER_ID.into());
    }
    if !curated || codex || copilot {
        let catalog = ProviderCatalog::from_embedded().map_err(|e| e.to_string())?;
        if codex && !config.providers.contains_key(BUILTIN_CODEX_PROVIDER_ID) {
            let mut provider =
                compatible(catalog.provider("openai").ok_or("OpenAI catalog missing")?);
            provider.name = Some("OpenAI Codex".into());
            provider.auth_provider = Some(ProviderId::codex());
            provider.api_key_env.clear();
            provider.base_url = "https://chatgpt.com/backend-api/codex".into();
            provider.api_mode = OpenAiApiMode::Responses;
            config.providers.insert(
                BUILTIN_CODEX_PROVIDER_ID.into(),
                ProviderConfig::OpenAiCompatible(provider),
            );
            connected.push(BUILTIN_CODEX_PROVIDER_ID.into());
        }
        for source in catalog.sorted_by_priority() {
            if config.providers.contains_key(&source.id)
                || (curated && source.id != BUILTIN_COPILOT_PROVIDER_ID)
            {
                continue;
            }
            let id = ProviderId::parse(&source.id).ok_or("invalid catalog provider ID")?;
            if !(stored(store, &id)?
                || source
                    .api_key_env
                    .iter()
                    .any(|name| lookup(name).is_some_and(|v| !v.trim().is_empty())))
            {
                continue;
            }
            let provider = if source.id == "anthropic" {
                ProviderConfig::Anthropic(AnthropicProviderConfig {
                    name: Some(source.name.clone()),
                    base_url: source.base_url.clone(),
                    api_key_env: source.api_key_env.clone(),
                    models: models(source),
                    ..Default::default()
                })
            } else {
                let provider = compatible(source);
                if provider.base_url.is_empty() {
                    continue;
                }
                ProviderConfig::OpenAiCompatible(provider)
            };
            config.providers.insert(source.id.clone(), provider);
            connected.push(source.id.clone());
        }
    }
    Ok(connected)
}
/// A pooled login or `CLAUDE_CODE_OAUTH_TOKEN*` makes the subscription lane usable; a host
/// `claude` login alone needs the explicit `enabled: true` opt-in.
fn subscription_available(
    store: Option<&CredentialStore>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<bool, String> {
    let env_token = std::iter::once("CLAUDE_CODE_OAUTH_TOKEN".to_owned())
        .chain((2..=16).map(|n| format!("CLAUDE_CODE_OAUTH_TOKEN_{n}")))
        .any(|name| lookup(&name).is_some_and(|v| !v.is_empty()));
    if env_token {
        return Ok(true);
    }
    Ok(store
        .map(|store| store.load(&ProviderId::anthropic_subscription()))
        .transpose()
        .map_err(|e| e.to_string())?
        .flatten()
        .is_some_and(|credential| {
            harness_core::auth::anthropic_subscription::is_usable_pool(
                &harness_core::auth::anthropic_subscription::pool_from_credential(&credential),
            )
        }))
}
fn stored(store: Option<&CredentialStore>, id: &ProviderId) -> Result<bool, String> {
    store
        .map(|store| store.load(id).map(|value| value.is_some()))
        .transpose()
        .map(|value| value.unwrap_or(false))
        .map_err(|e| e.to_string())
}
fn compatible(source: &ProviderCatalogEntry) -> OpenAiCompatibleProviderConfig {
    OpenAiCompatibleProviderConfig {
        name: Some(source.name.clone()),
        base_url: source.base_url.clone(),
        api_key_env: source.api_key_env.clone(),
        models: models(source),
        ..Default::default()
    }
}
fn models(source: &ProviderCatalogEntry) -> BTreeMap<String, ModelConfig> {
    source
        .models
        .iter()
        .map(|(id, model)| (id.clone(), model.definition.clone()))
        .collect()
}

#[cfg(test)]
#[path = "runtime_catalog/tests.rs"]
mod tests;
