use super::*;

pub fn resolve_model_selection(
    config: &HarnessConfig,
    selector: &str,
    variant: Option<&str>,
) -> Result<ResolvedModelSelection, ConfigError> {
    let selector = selector.trim();
    if let Some(profile) = config.model_profiles.get(selector) {
        Ok(ResolvedModelSelection {
            selector: selector.into(),
            profile: Some(selector.into()),
            primary: target(
                config,
                &profile.model,
                variant.or(profile.variant.as_deref()),
            )?,
            fallback: profile
                .fallback
                .iter()
                .map(|fallback| target(config, &fallback.model, fallback.variant.as_deref()))
                .collect::<Result<_, _>>()?,
        })
    } else {
        Ok(ResolvedModelSelection {
            selector: selector.into(),
            profile: None,
            primary: target(config, selector, variant)?,
            fallback: Vec::new(),
        })
    }
}

fn target(
    config: &HarnessConfig,
    selector: &str,
    variant: Option<&str>,
) -> Result<ResolvedModelTarget, ConfigError> {
    let (provider, mut model) = selector
        .split_once(':')
        .or_else(|| selector.split_once('/'))
        .ok_or_else(|| ConfigError(format!("unknown model profile: {selector}")))?;
    let mut variant = variant;
    if !config
        .providers
        .get(provider)
        .is_some_and(|p| p.models().contains_key(model))
        && let Some((name, suffix)) = model.rsplit_once('/')
    {
        model = name;
        variant = variant.or(Some(suffix));
    }
    let entry = resolve_configured_model_metadata(config, provider, model, variant)?;
    Ok(ResolvedModelTarget {
        model_ref: format!("{provider}:{model}"),
        provider: provider.into(),
        model: model.into(),
        variant: entry.variant.clone(),
        reasoning_effort: entry.reasoning_effort.clone(),
        text_verbosity: entry.text_verbosity.clone(),
        reasoning_summary: (entry.supports_reasoning_summaries && entry.reasoning_effort.is_some())
            .then(|| "auto".into()),
        thinking: entry.thinking.clone(),
        limits: entry.limits.clone(),
        resolution: entry.resolution.clone(),
        catalog_entry: Some(Box::new(entry)),
    })
}

pub fn resolve_configured_model_metadata(
    config: &HarnessConfig,
    provider: &str,
    model: &str,
    variant: Option<&str>,
) -> Result<ResolvedModelCatalogEntry, ConfigError> {
    if config.disabled_providers.iter().any(|p| p == provider)
        || (!config.enabled_providers.is_empty()
            && !config.enabled_providers.iter().any(|p| p == provider))
    {
        return Err(ConfigError(format!("provider is disabled: {provider}")));
    }
    let backend = config
        .providers
        .get(provider)
        .ok_or_else(|| ConfigError(format!("unknown provider: {provider}")))?;
    let definition = backend
        .models()
        .get(model)
        .ok_or_else(|| ConfigError(format!("unknown model: {provider}:{model}")))?;
    let variant_definition = variant
        .map(|name| {
            definition
                .variants
                .get(name)
                .filter(|v| !v.disabled)
                .ok_or_else(|| ConfigError(format!("unknown or disabled variant: {name}")))
        })
        .transpose()?;
    let limits = model_limits(definition, variant_definition);
    limits
        .validate(&format!("{provider}:{model}"))
        .map_err(normalize::parse_error)?;
    let mut resolution =
        crate::model_resolution::resolve_model(crate::model_resolution::ModelResolutionInput {
            provider,
            model,
            metadata_family: definition.metadata.family.as_deref(),
            input_modalities: &definition.modalities.input,
            supports_tool_calls: definition.metadata.supports_tool_calls,
            supports_reasoning_summaries: definition.metadata.supports_reasoning_summaries,
        });
    resolution.capabilities.variants = definition
        .variants
        .iter()
        .filter(|(_, v)| !v.disabled)
        .map(|(name, _)| name.clone())
        .collect();
    let model_label = if definition.display_name.is_empty() {
        model
    } else {
        &definition.display_name
    };
    let variant_label = variant_definition
        .and_then(|v| v.display_name.as_deref())
        .or(variant);
    let metadata = variant_definition.map(|v| &v.metadata);
    Ok(ResolvedModelCatalogEntry {
        provider: provider.into(),
        provider_display_label: backend.name().unwrap_or(provider).into(),
        provider_backend_label: Some(
            match backend {
                ProviderConfig::OpenAiCompatible(_) => "OpenAI",
                ProviderConfig::Anthropic(_) => "Anthropic",
            }
            .into(),
        ),
        model: model.into(),
        model_display_label: model_label.into(),
        variant: variant.map(str::to_owned),
        variant_display_label: variant_label.map(str::to_owned),
        display_label: variant_label.map_or_else(
            || model_label.into(),
            |label| format!("{model_label} · {label}"),
        ),
        token_window_label: window_label(&limits),
        limits,
        description: metadata.and_then(|m| m.description.clone()),
        reasoning_effort: metadata
            .and_then(|m| m.reasoning_effort)
            .map(|m| m.as_str().into()),
        text_verbosity: metadata
            .and_then(|m| m.text_verbosity)
            .map(|m| m.as_str().into()),
        recommended_for: metadata.and_then(|m| m.recommended_for.clone()),
        thinking: variant_definition
            .and_then(|v| v.options.get("thinking"))
            .or_else(|| definition.options.get("thinking"))
            .cloned(),
        supports_reasoning_summaries: resolution.capabilities.supports_reasoning_summaries,
        resolution,
    })
}

pub(super) fn model_limits(
    model: &ModelConfig,
    variant: Option<&ModelVariantConfig>,
) -> ResolvedModelLimits {
    let provenance = if model.limit_provenance.kind == ModelLimitProvenanceKind::Unknown {
        ModelLimitProvenance::explicit("model configuration")
    } else {
        model.limit_provenance.clone()
    };
    let mut limits = ResolvedModelLimits::from_values(
        model.limit.context.or(model.metadata.context_window_tokens),
        model.limit.input.or(model.max_input_tokens),
        model.limit.output.or(model.max_output_tokens),
        provenance,
    );
    if let Some(variant) = variant {
        for (limit, value) in [
            (
                &mut limits.context_window,
                variant.limit.context.or(variant.context_window_tokens),
            ),
            (
                &mut limits.max_input,
                variant.limit.input.or(variant.max_input_tokens),
            ),
            (
                &mut limits.max_output,
                variant.limit.output.or(variant.max_output_tokens),
            ),
        ] {
            if let Some(value) = value {
                *limit = ResolvedModelLimit {
                    tokens: Some(value),
                    provenance: ModelLimitProvenance::explicit("model variant"),
                };
            }
        }
    }
    limits
}

fn window_label(limits: &ResolvedModelLimits) -> Option<String> {
    let parts: Vec<_> = [
        (limits.context_window_tokens(), "ctx"),
        (limits.max_input_tokens(), "in"),
        (limits.max_output_tokens(), "out"),
    ]
    .into_iter()
    .filter_map(|(value, suffix)| {
        value.map(|n| {
            let count = if n >= 1000 && n.is_multiple_of(1000) {
                format!("{}k", n / 1000)
            } else if n >= 1024 && n.is_multiple_of(1024) {
                format!("{}k", n / 1024)
            } else {
                n.to_string()
            };
            format!("{count} {suffix}")
        })
    })
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub fn resolve_profile_model_metadata(
    config: &HarnessConfig,
    profile: &str,
) -> Result<ResolvedProfileModelMetadata, ConfigError> {
    let agent = config
        .agents
        .get(profile)
        .ok_or_else(|| ConfigError(format!("unknown agent profile: {profile}")))?;
    let selected = resolve_model_selection(config, &agent.model_ref, agent.variant.as_deref())?;
    let entry = selected
        .primary
        .catalog_entry
        .ok_or_else(|| ConfigError("model metadata missing".into()))?;
    Ok(ResolvedProfileModelMetadata {
        profile: profile.into(),
        profile_description: Some(agent.description.clone()),
        provider: entry.provider,
        provider_display_label: entry.provider_display_label,
        provider_backend_label: entry.provider_backend_label,
        model: entry.model,
        model_display_label: entry.model_display_label,
        variant: entry.variant,
        variant_display_label: entry.variant_display_label,
        display_label: entry.display_label,
        token_window_label: entry.token_window_label,
        limits: entry.limits,
        description: entry.description,
        reasoning_effort: entry.reasoning_effort,
        text_verbosity: entry.text_verbosity,
        recommended_for: entry.recommended_for,
        thinking: entry.thinking,
        resolution: entry.resolution,
    })
}

pub fn configured_model_catalog(config: &HarnessConfig) -> Vec<ResolvedModelCatalogEntry> {
    let mut entries = Vec::new();
    for (provider, backend) in &config.providers {
        for (model, definition) in backend.models() {
            for variant in std::iter::once(None)
                .chain(definition.variants.keys().map(|key| Some(key.as_str())))
            {
                if let Ok(entry) =
                    resolve_configured_model_metadata(config, provider, model, variant)
                {
                    entries.push(entry);
                }
            }
        }
    }
    entries
}
pub fn configured_model_profile_catalog(
    config: &HarnessConfig,
) -> Result<Vec<ResolvedModelProfileCatalogEntry>, ConfigError> {
    config
        .model_profiles
        .keys()
        .map(|name| {
            resolve_model_selection(config, name, None).map(|s| ResolvedModelProfileCatalogEntry {
                name: name.clone(),
                primary: s.primary,
                fallback: s.fallback,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_selection_preserves_variant_limits_and_explicit_fallbacks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config = load_config_from_str(
            r#"{
            provider:{test:{type:'openai_compatible',models:{
                opaque:{name:'Opaque',metadata:{family:'gemini',supportsReasoningSummaries:true},limit:{context:8000,input:6000,output:2000},variants:{
                    high:{metadata:{reasoningEffort:'high'},limit:{input:5000}}, retired:{disabled:true}
                }}, unknown:{name:'Unknown'}
            }}},model:'test/opaque',model_profile:{quality:{model:'test/opaque',variant:'high',fallback:[{model:'test:unknown'}]}}
        }"#,
        )?;
        let selected = resolve_model_selection(&config, "quality", None)?;
        assert_eq!(selected.primary.model_ref, "test:opaque");
        assert_eq!(selected.primary.limits.max_input_tokens(), Some(5000));
        assert_eq!(selected.primary.reasoning_summary.as_deref(), Some("auto"));
        assert_eq!(
            selected.primary.resolution.family,
            crate::model_resolution::ModelFamily::Gemini
        );
        assert_eq!(selected.fallback.len(), 1);
        assert_eq!(selected.fallback[0].limits.context_window_tokens(), None);
        use crate::auto_fallback::{orchestrate_fallback_chain, remaining_fallback_model_refs};
        assert_eq!(
            remaining_fallback_model_refs(&selected, "unlisted"),
            ["test:unknown"]
        );
        let mut repeated = selected.clone();
        repeated.fallback.push(repeated.fallback[0].clone());
        let walk = orchestrate_fallback_chain(&repeated, &repeated.primary.model_ref);
        assert!(walk.exhausted());
        assert_eq!(walk.remaining_counts(), [1, 0, 0]);
        for (selector, variant) in [
            ("missing", None),
            ("test:opaque", Some("retired")),
            ("test:opaque", Some("missing")),
            ("other:opaque", None),
        ] {
            assert!(resolve_model_selection(&config, selector, variant).is_err());
        }
        Ok(())
    }
}
