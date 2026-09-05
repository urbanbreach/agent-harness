// allow: SIZE_OK — CLI runtime catalog (model + provider metadata)
use crate::UnwrapOrAbort;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use harness_core::auth::codex::codex_oauth_model_allowed;
use harness_core::auth::copilot::copilot_offline_fallback_models;
use harness_core::auth::{AuthProviderId, CredentialStore};
use harness_core::config::{
    load_config_from_str, HarnessConfig, ModelConfig, ModelLimitConfig, ModelLimitProvenance,
    ModelMetadataConfig, ModelModalitiesConfig, ModelVariantConfig, ModelVariantMetadataConfig,
    ModelVariantReasoningEffort, OpenAiApiMode, OpenAiCompatibleProviderConfig, ProviderConfig,
};
use harness_core::provider_catalog::{ModelCatalogEntry, ProviderCatalog};
use serde_json::Value;

use crate::generated_model_catalog::PROVIDER_CATALOG_JSON;

pub const BUILTIN_CODEX_PROVIDER_ID: &str = "openai-codex";
pub const BUILTIN_CODEX_PROVIDER_LABEL: &str = "OpenAI Codex";
pub const BUILTIN_COPILOT_PROVIDER_ID: &str = "github-copilot";
pub const BUILTIN_COPILOT_PROVIDER_LABEL: &str = "GitHub Copilot";
const DEFAULT_BUILTIN_MODEL: &str = "gpt-5.4-mini";
const CODEX_BASE_URL: &str = "https://api.openai.com/v1";
const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";
const GPT5_6_CODEX_DEFAULT_MAX_INPUT_TOKENS: u32 = 369_384;

#[derive(Debug, Clone)]
pub struct RuntimeCatalogResolution {
    pub config: HarnessConfig,
    pub connected_provider_ids: Vec<String>,
    pub no_provider_connected: bool,
    pub config_digest: String,
}

impl RuntimeCatalogResolution {
    pub fn has_connected_provider(&self) -> bool {
        !self.connected_provider_ids.is_empty()
    }
}

pub fn resolve_runtime_catalog(
    base_config: Option<HarnessConfig>,
    base_digest: Option<String>,
    session_dir_override: Option<PathBuf>,
    credential_store: Option<&CredentialStore>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<RuntimeCatalogResolution, String> {
    let connected = connected_builtin_providers(credential_store, env_lookup)?;
    let no_project_config = base_config.is_none();
    let mut config = match base_config {
        Some(config) => config,
        None => shipped_builtin_base_config()?,
    };

    merge_builtin_providers(&mut config, &connected, no_project_config)?;
    if let Ok(catalog) = ProviderCatalog::from_env() {
        merge_live_codex_models(&mut config, &catalog);
    }
    apply_provider_filters(&mut config);

    if base_digest.is_none() {
        let primary = connected
            .iter()
            .find(|provider| provider.as_str() == BUILTIN_CODEX_PROVIDER_ID)
            .or_else(|| connected.first())
            .cloned()
            .unwrap_or_else(|| BUILTIN_CODEX_PROVIDER_ID.to_string());
        retarget_default_model_refs(&mut config, &primary, DEFAULT_BUILTIN_MODEL);
        normalize_builtin_default_variants(&mut config);
    }

    config.apply_session_dir_override(session_dir_override);

    let connected_provider_ids = connected
        .into_iter()
        .filter(|provider| config.providers.contains_key(provider))
        .collect::<Vec<_>>();
    let no_provider_connected = connected_provider_ids.is_empty() && base_digest.is_none();

    Ok(RuntimeCatalogResolution {
        config,
        connected_provider_ids,
        no_provider_connected,
        config_digest: base_digest.unwrap_or_else(|| "builtin-auth-runtime".to_string()),
    })
}

fn connected_builtin_providers(
    credential_store: Option<&CredentialStore>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<String>, String> {
    let mut connected = Vec::new();
    if credential_present(credential_store, AuthProviderId::codex())?
        || env_present(env_lookup, "OPENAI_API_KEY")
    {
        connected.push(BUILTIN_CODEX_PROVIDER_ID.to_string());
    }
    if credential_present(credential_store, AuthProviderId::github_copilot())? {
        connected.push(BUILTIN_COPILOT_PROVIDER_ID.to_string());
    }
    Ok(connected)
}

fn credential_present(
    credential_store: Option<&CredentialStore>,
    provider: AuthProviderId,
) -> Result<bool, String> {
    let Some(store) = credential_store else {
        return Ok(false);
    };
    store
        .load(&provider)
        .map(|credential| credential.is_some())
        .map_err(|err| format!("failed to inspect stored {provider} credential: {err}"))
}

fn env_present(env_lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> bool {
    env_lookup(name).is_some_and(|value| !value.trim().is_empty())
}

fn shipped_builtin_base_config() -> Result<HarnessConfig, String> {
    let mut config =
        load_config_from_str(include_str!("../../../configs/harness.example.jsonc"))
            .map_err(|err| format!("failed to load shipped built-in runtime config: {err}"))?;
    config.providers.clear();
    Ok(config)
}

fn merge_builtin_providers(
    config: &mut HarnessConfig,
    connected: &[String],
    no_project_config: bool,
) -> Result<(), String> {
    let include_codex = no_project_config
        || connected
            .iter()
            .any(|provider| provider == BUILTIN_CODEX_PROVIDER_ID);
    if include_codex && !config.providers.contains_key(BUILTIN_CODEX_PROVIDER_ID) {
        config.providers.insert(
            BUILTIN_CODEX_PROVIDER_ID.to_string(),
            builtin_codex_provider()?,
        );
    }

    let include_copilot = no_project_config
        || connected
            .iter()
            .any(|provider| provider == BUILTIN_COPILOT_PROVIDER_ID);
    if include_copilot && !config.providers.contains_key(BUILTIN_COPILOT_PROVIDER_ID) {
        config.providers.insert(
            BUILTIN_COPILOT_PROVIDER_ID.to_string(),
            builtin_copilot_provider()?,
        );
    }

    Ok(())
}

fn merge_live_codex_models(config: &mut HarnessConfig, catalog: &ProviderCatalog) {
    let Some(source) = catalog.provider("openai") else {
        return;
    };
    let Some(ProviderConfig::OpenAiCompatible(codex)) =
        config.providers.get_mut(BUILTIN_CODEX_PROVIDER_ID)
    else {
        return;
    };
    if codex.auth_provider != Some(AuthProviderId::codex()) {
        return;
    }

    let discovered = source
        .models
        .iter()
        .filter(|(model_id, _)| codex_catalog_model_allowed(model_id))
        .filter(|(model_id, _)| !codex.models.contains_key(*model_id))
        .map(|(model_id, metadata)| {
            (
                model_id.clone(),
                normalize_codex_model_variants(model_id, model_config_from_catalog(metadata)),
            )
        })
        .collect::<Vec<_>>();
    codex.models.extend(discovered);
}

fn model_config_from_catalog(metadata: &ModelCatalogEntry) -> ModelConfig {
    let context = metadata.limits.context_window_tokens();
    let max_input = metadata.limits.max_input_tokens();
    let max_output = metadata.limits.max_output_tokens();
    ModelConfig {
        display_name: metadata.name.clone(),
        metadata: ModelMetadataConfig {
            context_window_tokens: context,
            supports_tool_calls: metadata.supports_tool_calls,
            ..Default::default()
        },
        limit: ModelLimitConfig {
            context,
            input: max_input,
            output: max_output,
        },
        modalities: ModelModalitiesConfig {
            input: vec!["text".to_string()],
            output: vec!["text".to_string()],
        },
        options: BTreeMap::new(),
        max_input_tokens: max_input,
        max_output_tokens: max_output,
        limit_provenance: metadata.limits.primary_provenance().clone(),
        variants: BTreeMap::new(),
    }
}

fn apply_provider_filters(config: &mut HarnessConfig) {
    let disabled = config
        .disabled_providers
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    for provider in disabled {
        config.providers.remove(&provider);
    }

    let enabled = config
        .enabled_providers
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    if !enabled.is_empty() {
        config
            .providers
            .retain(|provider, _| enabled.contains(provider.as_str()));
    }
}

fn builtin_codex_provider() -> Result<ProviderConfig, String> {
    let models = generated_provider_models("openai")?
        .into_iter()
        .filter(|(model, _)| codex_catalog_model_allowed(model))
        .map(|(model_id, cfg)| {
            (
                model_id.clone(),
                normalize_codex_model_variants(&model_id, cfg),
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(openai_provider(
        BUILTIN_CODEX_PROVIDER_LABEL,
        AuthProviderId::codex(),
        CODEX_BASE_URL,
        vec!["OPENAI_API_KEY".to_string()],
        models,
    ))
}

fn codex_catalog_model_allowed(model_id: &str) -> bool {
    model_id != "gpt-5.6" && codex_oauth_model_allowed(model_id)
}

fn normalize_codex_model_variants(model_id: &str, mut cfg: ModelConfig) -> ModelConfig {
    if model_id.starts_with("gpt-5.6-") {
        cfg.limit.input = Some(GPT5_6_CODEX_DEFAULT_MAX_INPUT_TOKENS);
        cfg.max_input_tokens = Some(GPT5_6_CODEX_DEFAULT_MAX_INPUT_TOKENS);
        cfg.limit_provenance =
            ModelLimitProvenance::compatibility("GPT-5.6 Codex default context profile");
    }
    if let Some(efforts) = codex_reasoning_efforts(model_id) {
        retain_codex_variants(&mut cfg.variants, efforts);
        insert_missing_codex_variants(&mut cfg.variants, efforts);
    }
    cfg
}

fn codex_reasoning_efforts(model_id: &str) -> Option<&'static [ModelVariantReasoningEffort]> {
    let minor = gpt5_minor_version(model_id);
    if model_id.contains("-chat") {
        return minor.is_some().then_some(&GPT5_CHAT_EFFORTS[..]);
    }
    if is_versioned_gpt5_pro(model_id) {
        return Some(&GPT5_PRO_2_PLUS_EFFORTS);
    }
    if model_id.contains("codex") {
        if minor.is_some_and(|minor| minor >= 3) {
            return Some(&CODEX_GPT5_3_PLUS_EFFORTS);
        }
        if model_id.contains("codex-max") || minor.is_some_and(|minor| minor >= 2) {
            return Some(&CODEX_GPT5_XHIGH_EFFORTS);
        }
        return None;
    }

    if minor.is_some_and(|minor| minor >= 2) {
        return Some(&GPT5_2_PLUS_EFFORTS);
    }
    if minor == Some(1) {
        return Some(&GPT5_1_EFFORTS);
    }
    None
}

const GPT5_1_EFFORTS: [ModelVariantReasoningEffort; 4] = [
    ModelVariantReasoningEffort::None,
    ModelVariantReasoningEffort::Low,
    ModelVariantReasoningEffort::Medium,
    ModelVariantReasoningEffort::High,
];

const GPT5_2_PLUS_EFFORTS: [ModelVariantReasoningEffort; 5] = [
    ModelVariantReasoningEffort::None,
    ModelVariantReasoningEffort::Low,
    ModelVariantReasoningEffort::Medium,
    ModelVariantReasoningEffort::High,
    ModelVariantReasoningEffort::Xhigh,
];

const GPT5_PRO_2_PLUS_EFFORTS: [ModelVariantReasoningEffort; 3] = [
    ModelVariantReasoningEffort::Medium,
    ModelVariantReasoningEffort::High,
    ModelVariantReasoningEffort::Xhigh,
];

const GPT5_CHAT_EFFORTS: [ModelVariantReasoningEffort; 1] = [ModelVariantReasoningEffort::Medium];

const CODEX_GPT5_XHIGH_EFFORTS: [ModelVariantReasoningEffort; 4] = [
    ModelVariantReasoningEffort::Low,
    ModelVariantReasoningEffort::Medium,
    ModelVariantReasoningEffort::High,
    ModelVariantReasoningEffort::Xhigh,
];

const CODEX_GPT5_3_PLUS_EFFORTS: [ModelVariantReasoningEffort; 5] = [
    ModelVariantReasoningEffort::None,
    ModelVariantReasoningEffort::Low,
    ModelVariantReasoningEffort::Medium,
    ModelVariantReasoningEffort::High,
    ModelVariantReasoningEffort::Xhigh,
];

fn gpt5_minor_version(model_id: &str) -> Option<u32> {
    let rest = model_id.strip_prefix("gpt-5.")?;
    let minor = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (!minor.is_empty()).then(|| minor.parse().ok()).flatten()
}

fn is_versioned_gpt5_pro(model_id: &str) -> bool {
    let Some(rest) = model_id.strip_prefix("gpt-5.") else {
        return false;
    };
    rest.chars().take_while(|ch| ch.is_ascii_digit()).count() > 0 && rest.contains("-pro")
}

fn reasoning_effort_label(effort: ModelVariantReasoningEffort) -> &'static str {
    match effort {
        ModelVariantReasoningEffort::None => "none",
        ModelVariantReasoningEffort::Minimal => "minimal",
        ModelVariantReasoningEffort::Low => "low",
        ModelVariantReasoningEffort::Medium => "medium",
        ModelVariantReasoningEffort::High => "high",
        ModelVariantReasoningEffort::Max => "max",
        ModelVariantReasoningEffort::Xhigh => "xhigh",
    }
}

fn reasoning_display_name(effort: ModelVariantReasoningEffort) -> &'static str {
    match effort {
        ModelVariantReasoningEffort::None => "None",
        ModelVariantReasoningEffort::Minimal => "Minimal",
        ModelVariantReasoningEffort::Low => "Low",
        ModelVariantReasoningEffort::Medium => "Medium",
        ModelVariantReasoningEffort::High => "High",
        ModelVariantReasoningEffort::Max => "Max",
        ModelVariantReasoningEffort::Xhigh => "Xhigh",
    }
}

fn insert_missing_codex_variants(
    variants: &mut BTreeMap<String, ModelVariantConfig>,
    efforts: &[ModelVariantReasoningEffort],
) {
    for effort in efforts {
        let key = reasoning_effort_label(*effort);
        if variants.contains_key(key) {
            continue;
        }
        variants.insert(
            key.to_string(),
            ModelVariantConfig {
                display_name: Some(reasoning_display_name(*effort).to_string()),
                metadata: ModelVariantMetadataConfig {
                    reasoning_effort: Some(*effort),
                    recommended_for: Some(format!("Codex OAuth reasoning preset: {key}")),
                    ..Default::default()
                },
                limit: ModelLimitConfig::default(),
                modalities: Default::default(),
                options: BTreeMap::new(),
                disabled: false,
                context_window_tokens: None,
                max_input_tokens: None,
                max_output_tokens: None,
            },
        );
    }
}

fn retain_codex_variants(
    variants: &mut BTreeMap<String, ModelVariantConfig>,
    efforts: &[ModelVariantReasoningEffort],
) {
    variants.retain(|variant, _| {
        efforts
            .iter()
            .any(|effort| reasoning_effort_label(*effort) == variant)
    });
}

fn builtin_copilot_provider() -> Result<ProviderConfig, String> {
    let mut models = generated_provider_models("github-copilot")?;
    if models.is_empty() {
        models = copilot_offline_fallback_models()
            .iter()
            .map(|model| {
                let mut cfg = ModelConfig {
                    display_name: model.id.to_string(),
                    metadata: ModelMetadataConfig {
                        family: Some(model.family.to_string()),
                        context_window_tokens: Some(model.context_window_tokens),
                        supports_tool_calls: Some(true),
                        ..Default::default()
                    },
                    limit: ModelLimitConfig {
                        context: Some(model.context_window_tokens),
                        input: Some(model.context_window_tokens),
                        output: Some(128_000),
                    },
                    modalities: Default::default(),
                    options: BTreeMap::new(),
                    max_input_tokens: None,
                    max_output_tokens: None,
                    limit_provenance: ModelLimitProvenance::compatibility(
                        "GitHub Copilot offline fallback",
                    ),
                    variants: BTreeMap::new(),
                };
                cfg.options.insert(
                    "catalogSource".to_string(),
                    Value::String("offline-fallback".to_string()),
                );
                (model.id.to_string(), cfg)
            })
            .collect();
    }
    Ok(openai_provider(
        BUILTIN_COPILOT_PROVIDER_LABEL,
        AuthProviderId::github_copilot(),
        COPILOT_BASE_URL,
        Vec::new(),
        models,
    ))
}

fn openai_provider(
    label: &str,
    auth_provider: AuthProviderId,
    base_url: &str,
    api_key_env: Vec<String>,
    models: BTreeMap<String, ModelConfig>,
) -> ProviderConfig {
    ProviderConfig::OpenAiCompatible(OpenAiCompatibleProviderConfig {
        name: Some(label.to_string()),
        auth_provider: Some(auth_provider),
        base_url: base_url.to_string(),
        api_key: String::new(),
        api_key_env,
        timeout_ms: 60_000,
        api_mode: OpenAiApiMode::Auto,
        cache_retention: Default::default(),
        headers: BTreeMap::new(),
        options: Default::default(),
        models,
    })
}

fn generated_provider_models(provider_id: &str) -> Result<BTreeMap<String, ModelConfig>, String> {
    let validated_catalog = ProviderCatalog::from_embedded().map_err(|err| err.to_string())?;
    let root = serde_json::from_str::<Value>(PROVIDER_CATALOG_JSON)
        .map_err(|err| format!("failed to parse generated provider catalog: {err}"))?;
    let Some(models) = root
        .get("provider")
        .and_then(|providers| providers.get(provider_id))
        .and_then(|provider| provider.get("models"))
        .and_then(Value::as_object)
    else {
        return Ok(BTreeMap::new());
    };

    let mut projected = BTreeMap::new();
    for (id, model) in models {
        let Ok(validated) = validated_catalog.validated_model(provider_id, id) else {
            continue;
        };
        let mut config = serde_json::from_value::<ModelConfig>(model.clone()).map_err(|err| {
            format!("failed to parse generated model `{provider_id}:{id}`: {err}")
        })?;
        config.metadata.context_window_tokens = validated.limits.context_window_tokens();
        config.limit.context = validated.limits.context_window_tokens();
        config.limit.input = validated.limits.max_input_tokens();
        config.limit.output = validated.limits.max_output_tokens();
        config.limit_provenance = validated.limits.context_window.provenance.clone();
        projected.insert(id.clone(), config);
    }
    Ok(projected)
}

fn normalize_builtin_default_variants(config: &mut HarnessConfig) {
    for profile in config.agents.values_mut() {
        normalize_xhigh_variant(&mut profile.variant);
    }
    for profile in config.model_profiles.values_mut() {
        normalize_xhigh_variant(&mut profile.variant);
        for fallback in &mut profile.fallback {
            normalize_xhigh_variant(&mut fallback.variant);
        }
    }
}

fn normalize_xhigh_variant(variant: &mut Option<String>) {
    if variant.as_deref() == Some("xhigh") {
        *variant = Some("high".to_string());
    }
}

fn retarget_default_model_refs(config: &mut HarnessConfig, provider: &str, default_model: &str) {
    let rewrite = |value: &mut String| {
        if let Some(model) = value
            .strip_prefix("default/")
            .or_else(|| value.strip_prefix("default:"))
        {
            *value = format!("{provider}/{model}");
        } else if value.trim().is_empty() {
            *value = format!("{provider}/{default_model}");
        }
    };

    for agent in config.agents.values_mut() {
        rewrite(&mut agent.model_ref);
    }
    for profile in config.model_profiles.values_mut() {
        rewrite(&mut profile.model);
        for fallback in &mut profile.fallback {
            rewrite(&mut fallback.model);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use tempfile::tempdir;

    use harness_core::auth::{CredentialClock, StoredCredential, SystemCredentialClock};
    use harness_core::config::configured_model_catalog;
    use harness_core::context_budget::{compute_request_budget, RequestBudgetInput};
    use harness_providers::{ProviderOutputCapDisposition, ProviderRequestCost};

    use super::*;

    fn store_with(provider: AuthProviderId) -> (tempfile::TempDir, CredentialStore) {
        let temp = tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        store
            .save(&StoredCredential::api_key(
                provider,
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .unwrap_or_abort();
        (temp, store)
    }

    #[test]
    fn builtin_generated_model_projection_matches_validated_catalog() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        for provider_id in ["openai", "github-copilot"] {
            let projected = generated_provider_models(provider_id).unwrap_or_abort();
            let validated = catalog.provider(provider_id).unwrap_or_abort();

            // assert
            assert_eq!(projected.len(), validated.models.len());
            for (model_id, config) in projected {
                let entry = validated.models.get(&model_id).unwrap_or_abort();
                assert_eq!(
                    config.metadata.context_window_tokens,
                    entry.limits.context_window_tokens()
                );
                assert_eq!(config.limit.input, entry.limits.max_input_tokens());
                assert_eq!(config.limit.output, entry.limits.max_output_tokens());
                assert_eq!(
                    config.limit_provenance,
                    entry.limits.context_window.provenance
                );
            }
        }
    }

    #[test]
    fn no_config_stored_codex_activates_filtered_codex_catalog() {
        let (_temp, store) = store_with(AuthProviderId::codex());
        let resolved =
            resolve_runtime_catalog(None, None, None, Some(&store), &|_| None).unwrap_or_abort();
        assert!(resolved
            .connected_provider_ids
            .contains(&BUILTIN_CODEX_PROVIDER_ID.to_string()));
        let entries = configured_model_catalog(&resolved.config);
        assert!(entries.iter().any(|entry| {
            entry.provider == BUILTIN_CODEX_PROVIDER_ID && entry.model == DEFAULT_BUILTIN_MODEL
        }));
        assert!(!entries.iter().any(|entry| {
            entry.provider == BUILTIN_CODEX_PROVIDER_ID && entry.model == "gpt-4.1"
        }));
    }

    #[test]
    fn no_config_stored_copilot_activates_copilot_catalog() {
        let (_temp, store) = store_with(AuthProviderId::github_copilot());
        let resolved =
            resolve_runtime_catalog(None, None, None, Some(&store), &|_| None).unwrap_or_abort();
        assert!(resolved
            .connected_provider_ids
            .contains(&BUILTIN_COPILOT_PROVIDER_ID.to_string()));
        assert!(configured_model_catalog(&resolved.config)
            .iter()
            .any(|entry| {
                entry.provider == BUILTIN_COPILOT_PROVIDER_ID
                    && entry.provider_display_label == "GitHub Copilot"
            }));
    }

    #[test]
    fn explicit_config_provider_wins_over_matching_builtin_id() {
        let raw = r#"{
          provider: {
            "github-copilot": {
              type: "openai_compatible",
              name: "Custom Copilot",
              options: { baseURL: "http://127.0.0.1:9/v1", apiKey: "placeholder" },
              models: { custom: { name: "Custom" } }
            }
          },
          model: "github-copilot/custom",
          agent: { default: { model: "github-copilot/custom" } },
          permission: { "*": "deny" }
        }"#;
        let config = load_config_from_str(raw).unwrap_or_abort();
        let (_temp, store) = store_with(AuthProviderId::github_copilot());
        let resolved = resolve_runtime_catalog(
            Some(config),
            Some("explicit".to_string()),
            None,
            Some(&store),
            &|_| None,
        )
        .unwrap_or_abort();
        let entries = configured_model_catalog(&resolved.config);
        assert!(entries.iter().any(|entry| {
            entry.provider == BUILTIN_COPILOT_PROVIDER_ID
                && entry.provider_display_label == "Custom Copilot"
                && entry.model == "custom"
        }));
        assert!(!entries.iter().any(|entry| {
            entry.provider == BUILTIN_COPILOT_PROVIDER_ID && entry.model == DEFAULT_BUILTIN_MODEL
        }));
    }

    #[test]
    fn no_config_without_credentials_reports_connect_state() {
        let resolved = resolve_runtime_catalog(None, None, None, None, &|_| None).unwrap_or_abort();

        assert!(resolved.no_provider_connected);
        assert!(resolved.connected_provider_ids.is_empty());
        assert!(resolved
            .config
            .providers
            .contains_key(BUILTIN_CODEX_PROVIDER_ID));
        assert!(resolved
            .config
            .providers
            .contains_key(BUILTIN_COPILOT_PROVIDER_ID));
    }

    #[test]
    fn openai_env_key_activates_codex_without_copying_secret() {
        let resolved = resolve_runtime_catalog(None, None, None, None, &|name| {
            (name == "OPENAI_API_KEY").then_some("sk-test-secret".to_string())
        })
        .unwrap_or_abort();

        assert!(resolved
            .connected_provider_ids
            .contains(&BUILTIN_CODEX_PROVIDER_ID.to_string()));
        let ProviderConfig::OpenAiCompatible(provider) = &resolved
            .config
            .providers
            .get(BUILTIN_CODEX_PROVIDER_ID)
            .unwrap_or_abort()
        else {
            panic!("expected OpenAiCompatible for codex");
        };
        assert_eq!(provider.auth_provider, Some(AuthProviderId::codex()));
        assert!(provider.api_key.is_empty());
        assert_eq!(provider.api_key_env, ["OPENAI_API_KEY".to_string()]);
    }

    #[test]
    fn explicit_config_without_credentials_does_not_add_builtins() {
        let raw = r#"{
          provider: {
            default: {
              type: "openai_compatible",
              options: { baseURL: "http://127.0.0.1:9/v1", apiKey: "placeholder" },
              models: { custom: { name: "Custom" } }
            }
          },
          model: "default/custom",
          agent: { default: { model: "default/custom" } },
          permission: { "*": "deny" }
        }"#;
        let config = load_config_from_str(raw).unwrap_or_abort();
        let resolved = resolve_runtime_catalog(
            Some(config),
            Some("explicit".to_string()),
            None,
            None,
            &|_| None,
        )
        .unwrap_or_abort();

        assert!(resolved.config.providers.contains_key("default"));
        assert!(!resolved
            .config
            .providers
            .contains_key(BUILTIN_CODEX_PROVIDER_ID));
        assert!(!resolved
            .config
            .providers
            .contains_key(BUILTIN_COPILOT_PROVIDER_ID));
    }

    #[test]
    fn copilot_offline_fallback_models_are_available_for_deterministic_catalogs() {
        let fallback = copilot_offline_fallback_models();
        assert!(!fallback.is_empty());
        assert!(fallback
            .iter()
            .any(|model| model.id.starts_with("gpt-") && model.family == "gpt"));
        assert!(fallback
            .iter()
            .any(|model| model.id.starts_with("claude") && model.supports_vision));
    }

    #[test]
    fn builtin_provider_configs_carry_auth_profiles_for_router() {
        let (temp, codex_store) = store_with(AuthProviderId::codex());
        let copilot_store = CredentialStore::new(temp.path());
        copilot_store
            .save(&StoredCredential::api_key(
                AuthProviderId::github_copilot(),
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .unwrap_or_abort();
        drop(codex_store);

        let resolved = resolve_runtime_catalog(None, None, None, Some(&copilot_store), &|_| None)
            .unwrap_or_abort();
        let ProviderConfig::OpenAiCompatible(codex) = &resolved
            .config
            .providers
            .get(BUILTIN_CODEX_PROVIDER_ID)
            .unwrap_or_abort()
        else {
            panic!("expected OpenAiCompatible for codex");
        };
        let ProviderConfig::OpenAiCompatible(copilot) = &resolved
            .config
            .providers
            .get(BUILTIN_COPILOT_PROVIDER_ID)
            .unwrap_or_abort()
        else {
            panic!("expected OpenAiCompatible for copilot");
        };

        assert_eq!(codex.auth_provider, Some(AuthProviderId::codex()));
        assert_eq!(
            copilot.auth_provider,
            Some(AuthProviderId::github_copilot())
        );
        assert!(codex.api_key.is_empty());
        assert!(copilot.api_key.is_empty());
    }

    #[test]
    fn live_models_dev_catalog_adds_only_named_gpt_5_6_tiers_with_default_context() {
        // arrange
        let dir = tempdir().unwrap_or_abort();
        let catalog_path = dir.path().join("models.json");
        std::fs::write(
            &catalog_path,
            r#"{
              "openai": {
                "name": "OpenAI",
                "env": ["OPENAI_API_KEY"],
                "models": {
                  "gpt-5.6": {
                    "id": "gpt-5.6",
                    "name": "GPT-5.6",
                    "reasoning": true,
                    "tool_call": true,
                    "limit": { "context": 1050000, "input": 922000, "output": 128000 }
                  },
                  "gpt-5.6-luna": {
                    "id": "gpt-5.6-luna",
                    "name": "GPT-5.6 Luna",
                    "reasoning": true,
                    "tool_call": true,
                    "limit": { "context": 1050000, "input": 922000, "output": 128000 }
                  },
                  "gpt-5.6-terra": {
                    "id": "gpt-5.6-terra",
                    "name": "GPT-5.6 Terra",
                    "reasoning": true,
                    "tool_call": true,
                    "limit": { "context": 1050000, "input": 922000, "output": 128000 }
                  },
                  "gpt-5.6-sol": {
                    "id": "gpt-5.6-sol",
                    "name": "GPT-5.6 Sol",
                    "reasoning": true,
                    "tool_call": true,
                    "limit": { "context": 1050000, "input": 922000, "output": 128000 }
                  }
                }
              }
            }"#,
        )
        .unwrap_or_abort();
        let catalog = ProviderCatalog::from_path(&catalog_path).unwrap_or_abort();
        let mut config = load_config_from_str(
            r#"{
              "provider": {
                "openai-codex": {
                  "type": "openai_compatible",
                  "options": {
                    "authProvider": "codex",
                    "baseURL": "https://api.openai.com/v1"
                  },
                  "models": {
                    "gpt-5.5": { "name": "GPT-5.5" }
                  }
                }
              },
              "model": "openai-codex/gpt-5.5",
              "permission": { "*": "deny" }
            }"#,
        )
        .unwrap_or_abort();

        // act
        merge_live_codex_models(&mut config, &catalog);

        // assert
        let ProviderConfig::OpenAiCompatible(codex) = config
            .providers
            .get(BUILTIN_CODEX_PROVIDER_ID)
            .unwrap_or_abort()
        else {
            panic!("expected OpenAI-compatible Codex provider");
        };
        assert!(!codex.models.contains_key("gpt-5.6"));
        for model_id in ["gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"] {
            assert_eq!(
                codex.models[model_id].metadata.context_window_tokens,
                Some(1_050_000)
            );
            assert_eq!(codex.models[model_id].limit.context, Some(1_050_000));
            assert_eq!(codex.models[model_id].limit.input, Some(369_384));
            assert_eq!(codex.models[model_id].max_input_tokens, Some(369_384));
            assert!(codex.models[model_id].variants.contains_key("xhigh"));
        }

        let luna = configured_model_catalog(&config)
            .into_iter()
            .find(|entry| entry.model == "gpt-5.6-luna" && entry.variant.is_none())
            .unwrap_or_abort();
        let budget = compute_request_budget(RequestBudgetInput {
            model_limits: &luna.limits,
            request_cost: ProviderRequestCost::default(),
            requested_output_tokens: None,
            safety_margin_tokens: 16_384,
            estimated_token_triggers: true,
            fallback_input_tokens: 32_768,
            output_cap_disposition: ProviderOutputCapDisposition::ProviderDefaulted(128_000),
        })
        .unwrap_or_abort();
        assert_eq!(budget.compaction_threshold_tokens, Some(353_000));
    }

    #[test]
    fn provider_filters_hide_builtins() {
        let raw = r#"{
          disabled_providers: ["openai-codex"],
          enabled_providers: ["github-copilot"],
          provider: {
            default: {
              type: "openai_compatible",
              options: { baseURL: "http://127.0.0.1:9/v1", apiKey: "placeholder" },
              models: { "gpt-5.4-mini": { name: "GPT-5.4 Mini" } }
            }
          },
          model: "default/gpt-5.4-mini",
          agent: { default: { model: "default/gpt-5.4-mini" } },
          permission: { "*": "deny" }
        }"#;
        let config = load_config_from_str(raw).unwrap_or_abort();
        let (temp_codex, codex_store) = store_with(AuthProviderId::codex());
        let copilot_store = CredentialStore::new(temp_codex.path());
        copilot_store
            .save(&StoredCredential::api_key(
                AuthProviderId::github_copilot(),
                "test-token",
                SystemCredentialClock.now_rfc3339(),
            ))
            .unwrap_or_abort();
        drop(codex_store);
        let resolved = resolve_runtime_catalog(
            Some(config),
            Some("explicit".to_string()),
            None,
            Some(&copilot_store),
            &|_| None,
        )
        .unwrap_or_abort();
        assert!(!resolved
            .config
            .providers
            .contains_key(BUILTIN_CODEX_PROVIDER_ID));
        assert!(resolved
            .config
            .providers
            .contains_key(BUILTIN_COPILOT_PROVIDER_ID));
    }

    #[test]
    fn codex_oauth_models_get_complete_reasoning_variants() {
        let (_temp, store) = store_with(AuthProviderId::codex());
        let resolved =
            resolve_runtime_catalog(None, None, None, Some(&store), &|_| None).unwrap_or_abort();
        let entries = configured_model_catalog(&resolved.config);

        let gpt54_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.provider == BUILTIN_CODEX_PROVIDER_ID && e.model == "gpt-5.4")
            .collect();
        assert!(
            gpt54_entries.iter().any(|e| {
                e.variant.as_deref() == Some("none")
                    && e.reasoning_effort.as_deref() == Some("none")
            }),
            "gpt-5.4 should have 'none' variant with reasoning_effort=none"
        );
        assert!(
            gpt54_entries.iter().any(|e| {
                e.variant.as_deref() == Some("xhigh")
                    && e.reasoning_effort.as_deref() == Some("xhigh")
            }),
            "gpt-5.4 should have 'xhigh' variant with reasoning_effort=xhigh"
        );

        let spark_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.provider == BUILTIN_CODEX_PROVIDER_ID && e.model == "gpt-5.3-codex-spark")
            .collect();
        assert!(
            spark_entries.iter().any(|e| {
                e.variant.as_deref() == Some("xhigh")
                    && e.reasoning_effort.as_deref() == Some("xhigh")
            }),
            "gpt-5.3-codex-spark should have 'xhigh' variant"
        );
        assert!(
            spark_entries.iter().any(|e| {
                e.variant.as_deref() == Some("none")
                    && e.reasoning_effort.as_deref() == Some("none")
            }),
            "gpt-5.3-codex-spark should have 'none' variant with reasoning_effort=none"
        );

        let gpt55 = entries
            .iter()
            .find(|e| {
                e.provider == BUILTIN_CODEX_PROVIDER_ID
                    && e.model == "gpt-5.5"
                    && e.variant.is_none()
            })
            .unwrap_or_abort();
        assert_eq!(gpt55.limits.context_window_tokens(), Some(1_050_000));
        assert_eq!(gpt55.limits.max_input_tokens(), Some(922_000));
        assert_eq!(gpt55.limits.max_output_tokens(), Some(128_000));

        assert!(
            !entries
                .iter()
                .any(|e| e.provider == BUILTIN_CODEX_PROVIDER_ID && e.model == "gpt-5.5-pro"),
            "Codex subscriptions should not expose Pro models"
        );
    }
}
