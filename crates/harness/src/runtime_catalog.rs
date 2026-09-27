use harness_core::{
    auth::{CredentialStore, ProviderId},
    config::*,
    provider_catalog::{ProviderCatalog, ProviderCatalogEntry},
};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) const BUILTIN_CODEX_PROVIDER_ID: &str = "openai-codex";
pub(crate) const BUILTIN_COPILOT_PROVIDER_ID: &str = "github-copilot";
pub(crate) struct RuntimeCatalogResolution {
    pub(crate) config: HarnessConfig,
    pub(crate) config_digest: String,
    pub(crate) connected_provider_ids: Vec<String>,
    pub(crate) no_provider_connected: bool,
}
pub(crate) fn resolve_runtime_catalog(
    base: Option<HarnessConfig>,
    digest: Option<String>,
    directory: Option<PathBuf>,
    store: Option<&CredentialStore>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<RuntimeCatalogResolution, String> {
    let explicit = base.is_some();
    let mut config = match base {
        Some(config) => config,
        None => load_config_from_str("{}").map_err(|e| e.to_string())?,
    };
    let mut connected = add_connected_providers(&mut config, explicit, store, lookup)?;
    merge_cached_models(&mut config, store, lookup);
    for (name, provider) in &mut config.providers {
        if provider.credential_provider(name) == Some(ProviderId::codex()) {
            if let ProviderConfig::OpenAiCompatible(provider) = provider {
                provider
                    .models
                    .retain(|model, _| harness_providers::codex_model_allowed(model));
            }
        }
    }
    for (name, provider) in &config.providers {
        if connected.contains(name) {
            continue;
        }
        let (env, key) = match provider {
            ProviderConfig::OpenAiCompatible(p) => (&p.api_key_env, &p.api_key),
            ProviderConfig::Anthropic(p) => (&p.api_key_env, &p.api_key),
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
    if let Some(id) = connected.first() {
        let provider = config
            .providers
            .get(id)
            .ok_or("selected provider missing")?;
        let preferred = if provider.credential_provider(id) == Some(ProviderId::codex()) {
            &["gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.5"][..]
        } else {
            &["gpt-5.4-mini", "gpt-5.5", "claude-sonnet-4.5"][..]
        };
        let models = provider.models();
        let model = preferred
            .iter()
            .find(|name| models.contains_key(**name))
            .copied()
            .or_else(|| models.keys().next().map(String::as_str))
            .ok_or("provider has no usable model")?;
        for profile in config.agents.values_mut() {
            if !profile.model_ref_explicit && profile.model_ref == "mock:default" {
                profile.model_ref = format!("{id}:{model}");
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
    explicit: bool,
    store: Option<&CredentialStore>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<String>, String> {
    let mut connected = Vec::new();
    let codex = stored(store, &ProviderId::codex())?;
    let copilot = stored(store, &ProviderId::github_copilot())?;
    if !explicit || codex || copilot {
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
                || (explicit && source.id != BUILTIN_COPILOT_PROVIDER_ID)
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
mod tests {
    use super::*;
    use harness_core::auth::StoredCredential;

    #[test]
    fn connected_subscriptions_extend_explicit_config_without_replacing_its_model(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let store = CredentialStore::new(root.path());
        for id in [ProviderId::codex(), ProviderId::github_copilot()] {
            store.save(&StoredCredential::oauth(
                id,
                "fixture-access",
                "fixture-refresh",
                None,
                "2026-09-26T00:00:00Z",
            ))?;
        }
        let config = load_config_from_str(
            r#"{"provider":{"local":{"type":"openai_compatible","models":{"chosen":{}}}},"model":"local/chosen"}"#,
        )?;
        let result = resolve_runtime_catalog(Some(config), None, None, Some(&store), &|_| None)?;
        assert_eq!(result.config.agents["default"].model_ref, "local:chosen");
        assert!(result
            .connected_provider_ids
            .contains(&BUILTIN_COPILOT_PROVIDER_ID.into()));
        let codex = &result.config.providers[BUILTIN_CODEX_PROVIDER_ID];
        assert!(codex.models().contains_key("gpt-6-astra"));
        assert!(codex
            .models()
            .keys()
            .all(|model| harness_providers::codex_model_allowed(model)));
        let selected =
            resolve_model_selection(&result.config, "openai-codex:gpt-6-astra", Some("high"))?;
        assert_eq!(selected.primary.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(selected.primary.reasoning_summary.as_deref(), Some("auto"));
        assert!(selected.primary.resolution.capabilities.supports_vision);
        assert!(selected
            .primary
            .resolution
            .capabilities
            .variants
            .contains(&"max".into()));
        let mut config = result.config;
        let ProviderConfig::OpenAiCompatible(codex) = config
            .providers
            .get_mut(BUILTIN_CODEX_PROVIDER_ID)
            .ok_or("missing Codex")?
        else {
            return Err("wrong provider".into());
        };
        let explicit_astra = codex.models["gpt-6-astra"].clone();
        codex.models.remove("gpt-6-sol");
        let cache = root.path().join("catalog.json");
        std::fs::write(
            &cache,
            r#"{"openai":{"models":{
            "gpt-6-astra":{"limit":{"context":10000,"output":200}},
            "gpt-6-sol":{"limit":{"context":22222,"output":4444}}
        }}}"#,
        )?;
        let result = resolve_runtime_catalog(Some(config), None, None, Some(&store), &|name| {
            (name == "HARNESS_MODELS_PATH").then(|| cache.to_string_lossy().into_owned())
        })?;
        let models = result.config.providers[BUILTIN_CODEX_PROVIDER_ID].models();
        assert_eq!(models["gpt-6-sol"].limit.context, Some(22222));
        assert_eq!(
            serde_json::to_value(&models["gpt-6-astra"])?,
            serde_json::to_value(explicit_astra)?
        );
        assert_eq!(result.config.agents["default"].model_ref, "local:chosen");
        Ok(())
    }
}
