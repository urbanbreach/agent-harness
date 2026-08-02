// allow: SIZE_OK — model catalog (merge thinking + variant resolution + capability inference + fallback metadata)
use crate::model_resolution::{resolve_model, ModelResolution, ModelResolutionInput};

use super::model_selection::{resolve_agent_model_selection, resolve_named_model_profile};
use super::*;
use serde_json::Value;

fn merge_thinking_option(
    model_options: &std::collections::BTreeMap<String, Value>,
    variant_options: Option<&std::collections::BTreeMap<String, Value>>,
) -> Option<Value> {
    variant_options
        .and_then(|options| options.get("thinking"))
        .or_else(|| model_options.get("thinking"))
        .cloned()
}

pub fn resolve_profile_model_metadata(
    cfg: &HarnessConfig,
    profile_name: &str,
) -> Result<ResolvedProfileModelMetadata, ConfigError> {
    let profile = cfg.agents.get(profile_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown agent `{profile_name}`; available agents: {}",
            format_name_list(cfg.agents.keys().map(|name| name.as_str()))
        ))
    })?;

    let selection = resolve_agent_model_selection(cfg, profile_name, profile)?;
    let provider_name = selection.primary.provider.as_str();
    let model_name = selection.primary.model.as_str();

    let provider = cfg.providers.get(provider_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "agent `{profile_name}` references unknown provider `{provider_name}` in model selection `{}`; available providers: {}",
            profile.model_ref,
            format_name_list(cfg.providers.keys().map(|name| name.as_str()))
        ))
    })?;

    let models = provider.models();
    let model = models.get(model_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references unknown model `{model_name}` in model selection `{}`; available models for provider `{provider_name}`: {}",
                profile.model_ref,
                format_name_list(models.keys().map(|name| name.as_str()))
            ))
        })?;

    let variant = selection.primary.variant.as_deref().map(|variant_name| {
        let variant = model.variants.get(variant_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references unknown variant `{variant_name}` for model `{}`; available variants: {}",
                selection.primary.model_ref,
                format_name_list(model.variants.keys().map(|name| name.as_str()))
            ))
        })?;

        if variant.disabled {
            return Err(ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references disabled variant `{variant_name}` for model `{}`; choose an enabled variant",
                selection.primary.model_ref
            )));
        }

        Ok((variant_name, variant))
    });
    let variant = variant.transpose()?;

    let display_label = build_model_display_label(model, variant);
    let variant_display_label = variant.map(|(variant_name, variant_cfg)| {
        variant_cfg
            .display_name
            .clone()
            .unwrap_or_else(|| variant_name.to_string())
    });
    let context_window_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.context_window_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.context))
        .or(model.metadata.context_window_tokens)
        .or(model.limit.context);
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.input))
        .or(model.max_input_tokens)
        .or(model.limit.input);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.output))
        .or(model.max_output_tokens)
        .or(model.limit.output);
    let resolution = resolve_model_catalog_metadata(
        provider_name,
        model_name,
        model,
        context_window_tokens,
        max_input_tokens,
        max_output_tokens,
    );
    let thinking = merge_thinking_option(&model.options, variant.map(|(_, cfg)| &cfg.options));

    Ok(ResolvedProfileModelMetadata {
        profile: profile_name.to_string(),
        profile_description: Some(profile.description.clone()),
        provider: provider_name.to_string(),
        provider_display_label: provider.display_label(provider_name),
        provider_backend_label: provider_backend_label(provider).map(str::to_string),
        model: model_name.to_string(),
        model_display_label: model.display_name.clone(),
        variant: variant.map(|(variant_name, _)| variant_name.to_string()),
        variant_display_label,
        display_label,
        token_window_label: build_token_window_label(
            context_window_tokens,
            max_input_tokens,
            max_output_tokens,
        ),
        context_window_tokens,
        max_input_tokens,
        max_output_tokens,
        description: variant.and_then(|(_, variant_cfg)| variant_cfg.metadata.description.clone()),
        reasoning_effort: variant.and_then(|(_, variant_cfg)| {
            variant_cfg
                .metadata
                .reasoning_effort
                .map(model_variant_reasoning_effort_label)
                .map(str::to_string)
        }),
        text_verbosity: variant.and_then(|(_, variant_cfg)| {
            variant_cfg
                .metadata
                .text_verbosity
                .map(model_variant_text_verbosity_label)
                .map(str::to_string)
        }),
        recommended_for: variant
            .and_then(|(_, variant_cfg)| variant_cfg.metadata.recommended_for.clone()),
        thinking,
        resolution,
    })
}

pub fn resolve_configured_model_metadata(
    cfg: &HarnessConfig,
    provider_name: &str,
    model_name: &str,
    variant_name: Option<&str>,
) -> Result<ResolvedModelCatalogEntry, ConfigError> {
    let provider = cfg.providers.get(provider_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown provider `{provider_name}`; available providers: {}",
            format_name_list(cfg.providers.keys().map(|name| name.as_str()))
        ))
    })?;

    let model = provider.models().get(model_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown model `{model_name}` for provider `{provider_name}`; available models: {}",
            format_name_list(provider.models().keys().map(|name| name.as_str()))
        ))
    })?;

    let variant = variant_name.map(|variant_name| {
        let variant = model.variants.get(variant_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "unknown variant `{variant_name}` for model `{provider_name}:{model_name}`; available variants: {}",
                format_name_list(model.variants.keys().map(|name| name.as_str()))
            ))
        })?;

        if variant.disabled {
            return Err(ConfigError::InvalidReference(format!(
                "variant `{variant_name}` for model `{provider_name}:{model_name}` is disabled"
            )));
        }

        Ok((variant_name, variant))
    });
    let variant = variant.transpose()?;

    Ok(build_resolved_model_catalog_entry(
        provider_name,
        model_name,
        model,
        provider,
        variant,
    ))
}

pub fn configured_model_catalog(cfg: &HarnessConfig) -> Vec<ResolvedModelCatalogEntry> {
    let mut entries = Vec::new();

    for (provider_name, provider) in &cfg.providers {
        for (model_name, model) in provider.models() {
            entries.push(build_resolved_model_catalog_entry(
                provider_name,
                model_name,
                model,
                provider,
                None,
            ));

            for (variant_name, variant_cfg) in &model.variants {
                if variant_cfg.disabled {
                    continue;
                }

                entries.push(build_resolved_model_catalog_entry(
                    provider_name,
                    model_name,
                    model,
                    provider,
                    Some((variant_name.as_str(), variant_cfg)),
                ));
            }
        }
    }

    entries
}

pub fn configured_model_profile_catalog(
    cfg: &HarnessConfig,
) -> Result<Vec<ResolvedModelProfileCatalogEntry>, ConfigError> {
    cfg.model_profiles
        .keys()
        .map(|name| {
            resolve_named_model_profile(cfg, name, None).map(|selection| {
                ResolvedModelProfileCatalogEntry {
                    name: name.clone(),
                    primary: selection.primary,
                    fallback: selection.fallback,
                }
            })
        })
        .collect()
}

fn build_resolved_model_catalog_entry(
    provider_name: &str,
    model_name: &str,
    model: &ModelConfig,
    provider: &ProviderConfig,
    variant: Option<(&str, &ModelVariantConfig)>,
) -> ResolvedModelCatalogEntry {
    let context_window_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.context_window_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.context))
        .or(model.metadata.context_window_tokens)
        .or(model.limit.context);
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.input))
        .or(model.max_input_tokens)
        .or(model.limit.input);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.output))
        .or(model.max_output_tokens)
        .or(model.limit.output);
    let resolution = resolve_model_catalog_metadata(
        provider_name,
        model_name,
        model,
        context_window_tokens,
        max_input_tokens,
        max_output_tokens,
    );
    let thinking = merge_thinking_option(&model.options, variant.map(|(_, cfg)| &cfg.options));

    ResolvedModelCatalogEntry {
        provider: provider_name.to_string(),
        provider_display_label: provider.display_label(provider_name),
        provider_backend_label: provider_backend_label(provider).map(str::to_string),
        model: model_name.to_string(),
        model_display_label: model.display_name.clone(),
        variant: variant.map(|(variant_name, _)| variant_name.to_string()),
        variant_display_label: variant.map(|(variant_name, variant_cfg)| {
            variant_cfg
                .display_name
                .clone()
                .unwrap_or_else(|| variant_name.to_string())
        }),
        display_label: build_model_display_label(model, variant),
        token_window_label: build_token_window_label(
            context_window_tokens,
            max_input_tokens,
            max_output_tokens,
        ),
        context_window_tokens,
        max_input_tokens,
        max_output_tokens,
        description: variant.and_then(|(_, variant_cfg)| variant_cfg.metadata.description.clone()),
        reasoning_effort: variant.and_then(|(_, variant_cfg)| {
            variant_cfg
                .metadata
                .reasoning_effort
                .map(model_variant_reasoning_effort_label)
                .map(str::to_string)
        }),
        text_verbosity: variant.and_then(|(_, variant_cfg)| {
            variant_cfg
                .metadata
                .text_verbosity
                .map(model_variant_text_verbosity_label)
                .map(str::to_string)
        }),
        recommended_for: variant
            .and_then(|(_, variant_cfg)| variant_cfg.metadata.recommended_for.clone()),
        thinking,
        supports_reasoning_summaries: model.metadata.supports_reasoning_summaries.unwrap_or(false),
        resolution,
    }
}

fn resolve_model_catalog_metadata(
    provider_name: &str,
    model_name: &str,
    model: &ModelConfig,
    context_window_tokens: Option<u32>,
    max_input_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
) -> ModelResolution {
    resolve_model(ModelResolutionInput {
        provider: provider_name,
        model: model_name,
        metadata_family: model.metadata.family.as_deref(),
        input_modalities: &model.modalities.input,
        context_window_tokens,
        max_input_tokens,
        max_output_tokens,
        supports_tool_calls: model.metadata.supports_tool_calls,
        supports_reasoning_summaries: model.metadata.supports_reasoning_summaries,
    })
}

fn provider_backend_label(provider: &ProviderConfig) -> Option<&'static str> {
    match provider {
        ProviderConfig::OpenAiCompatible(_) => Some("OpenAI"),
        ProviderConfig::Anthropic(_) => Some("Anthropic"),
    }
}

fn build_model_display_label(
    model: &ModelConfig,
    variant: Option<(&str, &ModelVariantConfig)>,
) -> String {
    let Some((variant_name, variant_cfg)) = variant else {
        return model.display_name.clone();
    };

    let variant_label = variant_cfg.display_name.as_deref().unwrap_or(variant_name);
    format!("{} · {}", model.display_name, variant_label)
}

fn build_token_window_label(
    context_window_tokens: Option<u32>,
    max_input_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
) -> Option<String> {
    let mut segments = Vec::new();

    if let Some(tokens) = context_window_tokens {
        segments.push(format!("{} ctx", compact_token_count(tokens)));
    }
    if let Some(tokens) = max_input_tokens {
        segments.push(format!("{} in", compact_token_count(tokens)));
    }
    if let Some(tokens) = max_output_tokens {
        segments.push(format!("{} out", compact_token_count(tokens)));
    }

    (!segments.is_empty()).then(|| segments.join(" · "))
}

fn compact_token_count(tokens: u32) -> String {
    if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}k", tokens / 1_000)
    } else if tokens >= 1_024 && tokens.is_multiple_of(1_024) {
        format!("{}k", tokens / 1_024)
    } else {
        tokens.to_string()
    }
}

fn model_variant_reasoning_effort_label(effort: ModelVariantReasoningEffort) -> &'static str {
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

fn model_variant_text_verbosity_label(verbosity: ModelVariantTextVerbosity) -> &'static str {
    match verbosity {
        ModelVariantTextVerbosity::Low => "low",
        ModelVariantTextVerbosity::Medium => "medium",
        ModelVariantTextVerbosity::High => "high",
    }
}
